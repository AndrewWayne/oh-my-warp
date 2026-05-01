//! omw-remote launcher state — process-wide singleton accessed from the UI.
//!
//! Wiring 5 scope: the agent-footer "Remote Control" button calls
//! [`OmwRemoteState::shared`] then [`OmwRemoteState::toggle`] to start or stop
//! an embedded `omw-remote` daemon.
//!
//! The daemon runs on its own dedicated tokio runtime in a background thread,
//! so we don't have to assume the caller is in a tokio context. The runtime is
//! created lazily on first `start()`.
//!
//! Out of scope here (see Wiring 5 task brief):
//! - QR popup modal
//! - PTY-controller hook (no `WarpSessionBashOperations` adapter)
//! - Reactive UI binding (we re-read status on each button render)

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::Duration;

use parking_lot::Mutex;
use tokio::runtime::Builder;
use tokio::task::JoinHandle;

/// Default loopback bind for the embedded daemon.
const DEFAULT_BIND: &str = "127.0.0.1:8787";

/// Pinned origin matching `DEFAULT_BIND` (per BYORC §8.1, the daemon rejects
/// any WS upgrade whose `Origin` header doesn't match).
const DEFAULT_PINNED_ORIGIN: &str = "http://127.0.0.1:8787";

/// Pair token TTL when the user clicks "Remote Control" (BYORC default: 10 min).
const PAIR_TTL: Duration = Duration::from_secs(10 * 60);

/// Inactivity timeout for the WS PTY bridge (BYORC default: 60 s).
const INACTIVITY_TIMEOUT: Duration = Duration::from_secs(60);

/// Nonce store retention window (BYORC default: 60 s).
const NONCE_WINDOW: Duration = Duration::from_secs(60);

/// Public status surface for the button label.
///
/// `pair_url` and `error` are populated for the Debug print that the toggle
/// handler emits (and for future use); they aren't read by name yet.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum OmwRemoteStatus {
    Stopped,
    Starting,
    Running { pair_url: String },
    Failed { error: String },
}

/// Process-wide launcher state.
pub struct OmwRemoteState {
    inner: Mutex<Inner>,
}

struct Inner {
    status: OmwRemoteStatus,
    /// Handle of the spawned `omw_remote::serve` task. `Some` while the
    /// daemon is running. We abort it to stop, since omw-remote's `serve()`
    /// has no graceful-shutdown hook in this version of the API.
    serve_task: Option<JoinHandle<()>>,
    /// Handle to the dedicated runtime thread. Created lazily; reused across
    /// start/stop cycles. We keep it warm rather than tearing it down on stop
    /// so the second start doesn't have to spin up a new runtime.
    runtime_handle: Option<tokio::runtime::Handle>,
    runtime_thread: Option<thread::JoinHandle<()>>,
}

static SHARED: OnceLock<Arc<OmwRemoteState>> = OnceLock::new();

impl OmwRemoteState {
    /// Process-wide accessor. Lazily constructs on first call.
    pub fn shared() -> Arc<Self> {
        SHARED
            .get_or_init(|| {
                Arc::new(Self {
                    inner: Mutex::new(Inner {
                        status: OmwRemoteStatus::Stopped,
                        serve_task: None,
                        runtime_handle: None,
                        runtime_thread: None,
                    }),
                })
            })
            .clone()
    }

    /// Snapshot of the current status. Cheap; the button can call this on
    /// every render.
    pub fn status(&self) -> OmwRemoteStatus {
        self.inner.lock().status.clone()
    }

    /// True if the daemon is currently running. Convenience for the button
    /// label/tooltip toggle (not used yet — the button only re-renders after
    /// a click in this wiring pass).
    #[allow(dead_code)]
    pub fn is_running(&self) -> bool {
        matches!(self.status(), OmwRemoteStatus::Running { .. })
    }

    /// Toggle the daemon. Returns the new status after the transition. If the
    /// daemon was already starting, this is a no-op and the current status is
    /// returned unchanged.
    pub fn toggle(&self) -> OmwRemoteStatus {
        match self.status() {
            OmwRemoteStatus::Running { .. } => {
                let _ = self.stop();
                self.status()
            }
            OmwRemoteStatus::Stopped | OmwRemoteStatus::Failed { .. } => {
                if let Err(e) = self.start() {
                    let mut g = self.inner.lock();
                    g.status = OmwRemoteStatus::Failed { error: e };
                    return g.status.clone();
                }
                self.status()
            }
            OmwRemoteStatus::Starting => self.status(),
        }
    }

    /// Start the embedded daemon. Idempotent: a second call while running
    /// returns `Ok(())` without doing anything.
    ///
    /// Blocks the caller until the daemon has finished its async init (bind +
    /// pair token issuance). Typical wall time: a few ms.
    pub fn start(&self) -> Result<(), String> {
        // Fast path: already running.
        {
            let g = self.inner.lock();
            if matches!(g.status, OmwRemoteStatus::Running { .. }) {
                return Ok(());
            }
        }

        // Mark as Starting so the UI can reflect that. We hold the lock only
        // briefly here; the actual init happens with the lock released.
        {
            let mut g = self.inner.lock();
            g.status = OmwRemoteStatus::Starting;
        }

        // Bring up (or reuse) the runtime thread.
        let handle = self.ensure_runtime()?;

        // Block on init from the calling thread. The init future returns the
        // pair URL on success and a string error on failure.
        let (init_tx, init_rx) =
            std::sync::mpsc::sync_channel::<Result<(String, JoinHandle<()>), String>>(1);
        let runtime_handle = handle.clone();
        handle.spawn(async move {
            let result = bring_up_daemon(runtime_handle).await;
            let _ = init_tx.send(result);
        });

        match init_rx
            .recv()
            .map_err(|e| format!("init channel closed: {e}"))?
        {
            Ok((pair_url, serve_task)) => {
                eprintln!("omw-remote running. Pair URL: {pair_url}");
                let mut g = self.inner.lock();
                g.status = OmwRemoteStatus::Running { pair_url };
                g.serve_task = Some(serve_task);
                Ok(())
            }
            Err(e) => {
                let mut g = self.inner.lock();
                g.status = OmwRemoteStatus::Failed {
                    error: e.clone(),
                };
                Err(e)
            }
        }
    }

    /// Stop the daemon if running. Idempotent.
    pub fn stop(&self) -> Result<(), String> {
        let task = {
            let mut g = self.inner.lock();
            g.status = OmwRemoteStatus::Stopped;
            g.serve_task.take()
        };
        if let Some(task) = task {
            task.abort();
        }
        Ok(())
    }

    /// Spin up (or return) the dedicated runtime thread.
    fn ensure_runtime(&self) -> Result<tokio::runtime::Handle, String> {
        let mut g = self.inner.lock();
        if let Some(h) = &g.runtime_handle {
            return Ok(h.clone());
        }
        let (handle_tx, handle_rx) = std::sync::mpsc::sync_channel::<tokio::runtime::Handle>(1);
        let thread_handle = thread::Builder::new()
            .name("omw-remote-rt".into())
            .spawn(move || {
                let rt = match Builder::new_multi_thread()
                    .enable_all()
                    .worker_threads(2)
                    .thread_name("omw-remote-worker")
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => {
                        eprintln!("omw-remote: failed to build tokio runtime: {e}");
                        return;
                    }
                };
                let _ = handle_tx.send(rt.handle().clone());
                // Hold the runtime alive for the lifetime of this thread.
                rt.block_on(std::future::pending::<()>());
            })
            .map_err(|e| format!("spawning omw-remote-rt thread: {e}"))?;

        let handle = handle_rx
            .recv()
            .map_err(|e| format!("runtime handle channel closed: {e}"))?;
        g.runtime_handle = Some(handle.clone());
        g.runtime_thread = Some(thread_handle);
        Ok(handle)
    }
}

/// Resolve the `<OMW_DATA_DIR>` per the same convention used by `omw-cli`.
/// Order: `OMW_DATA_DIR`, `XDG_DATA_HOME/omw`, `$HOME/.local/share/omw` (or
/// `%USERPROFILE%\.local\share\omw`).
fn data_dir() -> Result<PathBuf, String> {
    if let Some(p) = std::env::var_os("OMW_DATA_DIR") {
        if !p.is_empty() {
            return Ok(PathBuf::from(p));
        }
    }
    if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
        if !xdg.is_empty() {
            return Ok(PathBuf::from(xdg).join("omw"));
        }
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| "neither HOME nor USERPROFILE is set".to_string())?;
    Ok(home.join(".local").join("share").join("omw"))
}

/// Bring the daemon up. Returns the pair URL and a join handle for the spawned
/// serve task. Caller `.abort()`s the handle to stop the daemon — `omw-remote`
/// doesn't expose a graceful-shutdown hook in this version of the API.
async fn bring_up_daemon(
    runtime_handle: tokio::runtime::Handle,
) -> Result<(String, JoinHandle<()>), String> {
    let dir = data_dir()?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir {}: {e}", dir.display()))?;

    let host_key_path = dir.join("host_key.bin");
    let host_key = omw_remote::HostKey::load_or_create(&host_key_path)
        .map_err(|e| format!("loading host key {}: {e}", host_key_path.display()))?;

    let db_path = dir.join("omw-remote.sqlite3");
    let conn = omw_remote::open_db(&db_path)
        .map_err(|e| format!("opening db {}: {e}", db_path.display()))?;
    let pairings = Arc::new(omw_remote::Pairings::new(conn));

    // Issue a single pair token now so the user has a URL to scan immediately
    // when they click the button.
    let token = pairings
        .issue(PAIR_TTL)
        .map_err(|e| format!("issuing pair token: {e}"))?;
    let pair_url = format!("{DEFAULT_PINNED_ORIGIN}/pair?t={}", token.to_base32());

    let bind = DEFAULT_BIND
        .parse()
        .map_err(|e| format!("parsing bind addr {DEFAULT_BIND}: {e}"))?;

    let pty_registry = omw_server::SessionRegistry::new();
    let config = omw_remote::ServerConfig {
        bind,
        host_key: Arc::new(host_key),
        pinned_origin: DEFAULT_PINNED_ORIGIN.to_string(),
        inactivity_timeout: INACTIVITY_TIMEOUT,
        revocations: omw_remote::RevocationList::new(),
        nonce_store: omw_remote::NonceStore::new(NONCE_WINDOW),
        pairings: Some(pairings),
        shell: omw_remote::ShellSpec::default_for_host(),
        pty_registry,
        host_id: "warp-host".to_string(),
    };

    let serve_task = runtime_handle.spawn(async move {
        if let Err(e) = omw_remote::serve(config).await {
            eprintln!("omw-remote: serve loop ended with error: {e}");
        }
    });

    Ok((pair_url, serve_task))
}
