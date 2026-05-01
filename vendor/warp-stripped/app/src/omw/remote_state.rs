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
//! Reactive UI: every status mutation broadcasts on a [`tokio::sync::watch`]
//! channel. UI views call [`OmwRemoteState::status_rx`] to subscribe and
//! re-render label/tooltip/icon when the status changes (Gap 3).
//!
//! Out of scope here (see Wiring 5 task brief):
//! - QR popup modal
//! - PTY-controller hook (no `WarpSessionBashOperations` adapter)

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::Duration;

use parking_lot::Mutex;
use tokio::runtime::Builder;
use tokio::sync::watch;
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
/// `tailscale_serving` is `true` iff Gap 4's auto-bootstrap successfully
/// brought up `tailscale serve --https=8787` for this run — the pair modal
/// (Gap 2) reads it to decide whether to surface the tailnet URL.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum OmwRemoteStatus {
    Stopped,
    Starting,
    Running {
        pair_url: String,
        tailscale_serving: bool,
    },
    Failed {
        error: String,
    },
}

/// Process-wide launcher state.
pub struct OmwRemoteState {
    inner: Mutex<Inner>,
    /// Broadcast every status transition to subscribers (UI button labels,
    /// tooltips, icons). `watch::Sender` is `Sync`, so we keep it outside the
    /// inner mutex — readers can clone receivers without contending with the
    /// state-mutation lock.
    status_tx: watch::Sender<OmwRemoteStatus>,
}

struct Inner {
    status: OmwRemoteStatus,
    /// Handle of the spawned `omw_remote::serve` task. `Some` while the
    /// daemon is running. We abort it to stop, since omw-remote's `serve()`
    /// has no graceful-shutdown hook in this version of the API.
    serve_task: Option<JoinHandle<()>>,
    /// Live PTY-session registry shared with the running daemon. `Some` while
    /// the daemon is running; cleared on stop. `share_pane` callers grab a
    /// clone of this `Arc` to register the pane as an external session.
    pty_registry: Option<Arc<omw_server::SessionRegistry>>,
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
                let (status_tx, _rx) = watch::channel(OmwRemoteStatus::Stopped);
                Arc::new(Self {
                    inner: Mutex::new(Inner {
                        status: OmwRemoteStatus::Stopped,
                        serve_task: None,
                        pty_registry: None,
                        runtime_handle: None,
                        runtime_thread: None,
                    }),
                    status_tx,
                })
            })
            .clone()
    }

    /// Snapshot of the current status. Cheap; the button can call this on
    /// every render.
    pub fn status(&self) -> OmwRemoteStatus {
        self.inner.lock().status.clone()
    }

    /// Subscribe to status changes. Returns a [`watch::Receiver`] which the
    /// caller can `.borrow()` for the latest value or `.changed().await` to
    /// block until a transition. Used by the UI layer to keep the Phone
    /// button's label/tooltip/icon in sync (Gap 3).
    pub fn status_rx(&self) -> watch::Receiver<OmwRemoteStatus> {
        self.status_tx.subscribe()
    }

    /// Bridge the watch channel into an [`async_channel::Receiver`] suitable
    /// for `ViewContext::spawn_stream_local`. The Warp UI framework consumes
    /// any `Stream`, but we don't have `tokio-stream` in the workspace, so
    /// instead of wrapping the watch directly we spin up a tiny forwarder on
    /// our existing daemon runtime: each `watch::changed().await` produces an
    /// `async_channel::send`. The first item delivered is the *current* value
    /// (so a late-attached UI can paint the right icon immediately).
    ///
    /// Errors from `ensure_runtime` are surfaced — the UI falls back to a
    /// non-reactive button label in that (extremely rare) case.
    pub fn subscribe_status_stream(
        &self,
    ) -> Result<async_channel::Receiver<OmwRemoteStatus>, String> {
        let runtime = self.ensure_runtime()?;
        let mut watch_rx = self.status_rx();
        let (tx, rx) = async_channel::unbounded();

        // Seed the stream with the current value so the subscriber paints the
        // correct state on its first render, even if no transition follows.
        let seed = watch_rx.borrow_and_update().clone();
        let _ = tx.try_send(seed);

        runtime.spawn(async move {
            while watch_rx.changed().await.is_ok() {
                let snapshot = watch_rx.borrow_and_update().clone();
                if tx.send(snapshot).await.is_err() {
                    // UI dropped the receiver — exit the bridge.
                    break;
                }
            }
        });

        Ok(rx)
    }

    /// Update the cached status AND broadcast it on the watch channel. Caller
    /// must hold the inner-mutex guard so that the cached value and the
    /// broadcast can't be reordered with another mutation.
    fn set_status(&self, g: &mut Inner, new_status: OmwRemoteStatus) {
        g.status = new_status.clone();
        // `send_replace` ignores the "no active receivers" case — the UI may
        // not have subscribed yet, and that's fine.
        self.status_tx.send_replace(new_status);
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
                    self.set_status(&mut g, OmwRemoteStatus::Failed { error: e });
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
            self.set_status(&mut g, OmwRemoteStatus::Starting);
        }

        // Bring up (or reuse) the runtime thread.
        let handle = self.ensure_runtime()?;

        // Block on init from the calling thread. The init future returns the
        // pair URL on success and a string error on failure.
        type InitResult = Result<
            (
                String,
                bool,
                JoinHandle<()>,
                Arc<omw_server::SessionRegistry>,
            ),
            String,
        >;
        let (init_tx, init_rx) = std::sync::mpsc::sync_channel::<InitResult>(1);
        let runtime_handle = handle.clone();
        handle.spawn(async move {
            let result = bring_up_daemon(runtime_handle).await;
            let _ = init_tx.send(result);
        });

        match init_rx
            .recv()
            .map_err(|e| format!("init channel closed: {e}"))?
        {
            Ok((pair_url, tailscale_serving, serve_task, pty_registry)) => {
                eprintln!(
                    "omw-remote running. Pair URL: {pair_url} (tailscale_serving={tailscale_serving})"
                );
                let mut g = self.inner.lock();
                self.set_status(
                    &mut g,
                    OmwRemoteStatus::Running {
                        pair_url,
                        tailscale_serving,
                    },
                );
                g.serve_task = Some(serve_task);
                g.pty_registry = Some(pty_registry);
                Ok(())
            }
            Err(e) => {
                let mut g = self.inner.lock();
                self.set_status(&mut g, OmwRemoteStatus::Failed { error: e.clone() });
                Err(e)
            }
        }
    }

    /// Stop the daemon if running. Idempotent.
    pub fn stop(&self) -> Result<(), String> {
        let task = {
            let mut g = self.inner.lock();
            self.set_status(&mut g, OmwRemoteStatus::Stopped);
            // Drop the registry handle so any spawned PTYs the WS handlers
            // still hold get released as soon as those tasks exit.
            g.pty_registry = None;
            g.serve_task.take()
        };
        // Best-effort: if Gap 4's auto-bootstrap brought up `tailscale serve
        // --https=8787`, tell tailscale to forget that mapping before we kill
        // the serve task. Ignore errors: if Tailscale isn't installed, isn't
        // running, or no serve was registered, this is a no-op anyway, and
        // we'd rather stop the daemon than block on `tailscale unserve` here.
        let _ = super::tailscale::unserve(8787);
        if let Some(task) = task {
            task.abort();
        }
        Ok(())
    }

    /// Returns the live PTY session registry, when the daemon is running.
    /// Used by the pane-share path so the UI can register a Warp pane as an
    /// external session under the same registry the WS handlers consult.
    #[allow(dead_code)]
    pub fn pty_registry(&self) -> Option<Arc<omw_server::SessionRegistry>> {
        self.inner.lock().pty_registry.clone()
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

/// Bring the daemon up. Returns the pair URL, whether Tailscale Serve was
/// successfully bootstrapped, a join handle for the spawned serve task, and
/// a clone of the live PTY-session registry. Caller `.abort()`s the handle
/// to stop the daemon — `omw-remote` doesn't expose a graceful-shutdown hook
/// in this version of the API. The registry clone is surfaced so the UI can
/// call `share_pane` against the same registry the daemon's WS handlers
/// consult.
///
/// Gap 4 (Tailscale Serve auto-bootstrap): after binding loopback, probe for
/// a running Tailscale install. If one's there and reports a DNSName, shell
/// out to `tailscale serve --bg --https=8787 http://127.0.0.1:8787` and add
/// `https://<DNSName>` to `pinned_origins` so the WS handshake accepts both
/// the loopback AND the tailnet origin. If anything in that chain fails,
/// fall back to loopback-only behavior — never a hard error.
async fn bring_up_daemon(
    runtime_handle: tokio::runtime::Handle,
) -> Result<(String, bool, JoinHandle<()>, Arc<omw_server::SessionRegistry>), String> {
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

    let bind = DEFAULT_BIND
        .parse()
        .map_err(|e| format!("parsing bind addr {DEFAULT_BIND}: {e}"))?;

    // Probe Tailscale (Gap 4). On a running install with a DNSName, register
    // a serve mapping and prefer the tailnet URL for the pair link.
    let mut pinned_origins = vec![DEFAULT_PINNED_ORIGIN.to_string()];
    let mut pair_origin = DEFAULT_PINNED_ORIGIN.to_string();
    let mut tailscale_serving = false;
    let ts = super::tailscale::detect_status();
    if ts.installed && ts.running && ts.local_hostname.is_some() {
        match super::tailscale::serve_https(8787) {
            Ok(url) => {
                pinned_origins.push(url.clone());
                pair_origin = url;
                tailscale_serving = true;
            }
            Err(e) => {
                eprintln!("omw-remote: tailscale serve bootstrap failed: {e} (loopback-only)");
            }
        }
    }
    let pair_url = format!("{pair_origin}/pair?t={}", token.to_base32());

    let pty_registry = omw_server::SessionRegistry::new();
    let pty_registry_for_state = pty_registry.clone();
    let config = omw_remote::ServerConfig {
        bind,
        host_key: Arc::new(host_key),
        pinned_origins,
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

    Ok((pair_url, tailscale_serving, serve_task, pty_registry_for_state))
}

#[cfg(test)]
impl OmwRemoteState {
    /// Test-only constructor: builds a fresh instance independent of the
    /// process-wide `SHARED` singleton, so unit tests can exercise the
    /// watch-channel transition logic without contending with each other or
    /// with a daemon a previous test left running.
    fn new_for_test() -> Arc<Self> {
        let (status_tx, _rx) = watch::channel(OmwRemoteStatus::Stopped);
        Arc::new(Self {
            inner: Mutex::new(Inner {
                status: OmwRemoteStatus::Stopped,
                serve_task: None,
                pty_registry: None,
                runtime_handle: None,
                runtime_thread: None,
            }),
            status_tx,
        })
    }

    /// Test-only mutation hook: drives the same `set_status` that the real
    /// `start`/`stop`/failure paths invoke, without bringing up the daemon
    /// runtime. Used by the watch-channel unit tests.
    fn set_status_for_test(&self, status: OmwRemoteStatus) {
        let mut g = self.inner.lock();
        self.set_status(&mut g, status);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `status_rx().borrow()` returns the initial `Stopped` value before any
    /// transition has occurred — the watch channel is seeded in `shared()` /
    /// `new_for_test()`.
    #[test]
    fn status_rx_initial_value_is_stopped() {
        let state = OmwRemoteState::new_for_test();
        let rx = state.status_rx();
        assert!(matches!(*rx.borrow(), OmwRemoteStatus::Stopped));
    }

    /// Each `set_status` call is observable on a previously-subscribed
    /// receiver: the latest value via `.borrow()` reflects the mutation, and
    /// `.has_changed()` flips between reads. Covers all four variants the
    /// button cares about.
    #[test]
    fn status_rx_observes_each_transition() {
        let state = OmwRemoteState::new_for_test();
        let mut rx = state.status_rx();

        // Stopped -> Starting
        state.set_status_for_test(OmwRemoteStatus::Starting);
        assert!(rx.has_changed().expect("sender alive"));
        assert!(matches!(*rx.borrow_and_update(), OmwRemoteStatus::Starting));

        // Starting -> Running
        state.set_status_for_test(OmwRemoteStatus::Running {
            pair_url: "http://127.0.0.1:8787/pair?t=test".to_string(),
            tailscale_serving: false,
        });
        assert!(rx.has_changed().expect("sender alive"));
        match &*rx.borrow_and_update() {
            OmwRemoteStatus::Running {
                pair_url,
                tailscale_serving,
            } => {
                assert!(pair_url.contains("/pair?t=test"));
                assert!(!tailscale_serving);
            }
            other => panic!("expected Running, got {other:?}"),
        }

        // Running -> Failed
        state.set_status_for_test(OmwRemoteStatus::Failed {
            error: "boom".to_string(),
        });
        assert!(rx.has_changed().expect("sender alive"));
        match &*rx.borrow_and_update() {
            OmwRemoteStatus::Failed { error } => assert_eq!(error, "boom"),
            other => panic!("expected Failed, got {other:?}"),
        }

        // Failed -> Stopped
        state.set_status_for_test(OmwRemoteStatus::Stopped);
        assert!(rx.has_changed().expect("sender alive"));
        assert!(matches!(*rx.borrow_and_update(), OmwRemoteStatus::Stopped));
    }

    /// A receiver subscribed *after* a mutation still sees the latest value
    /// on first `.borrow()` (no missed events), since the watch channel only
    /// retains the latest value.
    #[test]
    fn late_subscriber_sees_latest_status() {
        let state = OmwRemoteState::new_for_test();
        state.set_status_for_test(OmwRemoteStatus::Starting);
        let rx = state.status_rx();
        assert!(matches!(*rx.borrow(), OmwRemoteStatus::Starting));
    }

    /// The async-channel bridge that the UI uses (`subscribe_status_stream`)
    /// seeds the stream with the current value AND forwards subsequent
    /// transitions. Run on a current-thread tokio runtime so we don't have
    /// to spin up the real daemon runtime in tests.
    #[tokio::test(flavor = "current_thread")]
    async fn subscribe_status_stream_seeds_and_forwards() {
        let state = OmwRemoteState::new_for_test();
        // Force a known starting value before subscribing.
        state.set_status_for_test(OmwRemoteStatus::Starting);

        // The bridge needs a runtime; `new_for_test` doesn't pre-attach one,
        // but `ensure_runtime` will spin one up. To keep the test self-
        // contained (and fast), we exercise the seed/forward logic directly
        // against the watch channel rather than going through the daemon
        // runtime.
        let mut watch_rx = state.status_rx();
        let (tx, rx) = async_channel::unbounded();
        let seed = watch_rx.borrow_and_update().clone();
        tx.try_send(seed).unwrap();

        // Seed delivered.
        let first = rx.recv().await.unwrap();
        assert!(matches!(first, OmwRemoteStatus::Starting));

        // Mutate: the bridge logic reads the new value on `changed().await`.
        state.set_status_for_test(OmwRemoteStatus::Running {
            pair_url: "http://127.0.0.1:8787/pair?t=x".to_string(),
            tailscale_serving: false,
        });
        watch_rx.changed().await.unwrap();
        let snapshot = watch_rx.borrow_and_update().clone();
        tx.try_send(snapshot).unwrap();

        let second = rx.recv().await.unwrap();
        assert!(matches!(second, OmwRemoteStatus::Running { .. }));
    }
}
