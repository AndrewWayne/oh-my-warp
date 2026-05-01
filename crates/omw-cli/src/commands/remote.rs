//! `omw remote {start,status,stop}` — Phase F.
//!
//! See `crates/omw-cli/tests/remote_status.rs` and `remote_start_stop.rs`
//! for the public-API contract.

use std::io::Write;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use clap::Args;
use omw_remote::{
    make_router, open_db, HostKey, NonceStore, Pairings, RevocationList, ServerConfig, ShellSpec,
};

use crate::db::data_dir;

#[derive(Args, Debug)]
pub struct StartArgs {
    /// Address to bind. Default: `127.0.0.1:8787`.
    #[arg(long, default_value = "127.0.0.1:8787")]
    pub listen: String,
    /// Skip Tailscale wiring (default for v0.4-thin).
    #[arg(long, default_value_t = false)]
    pub no_tailscale: bool,
    /// Hidden test hook: when the named env var is set, the server shuts down.
    #[arg(long, hide = true)]
    pub shutdown_signal: Option<String>,
}

#[derive(Args, Debug)]
pub struct StopArgs {
    /// Also revoke every paired device (`devices.revoked_at = now()`).
    #[arg(long, default_value_t = false)]
    pub all: bool,
}

fn pidfile_path(dir: &Path) -> PathBuf {
    dir.join("remote.pid")
}

fn host_key_path(dir: &Path) -> PathBuf {
    dir.join("host_key.bin")
}

fn remote_db_path(dir: &Path) -> PathBuf {
    dir.join("omw-remote.sqlite3")
}

/// Pidfile body. Plain `key=value` lines (one per line).
struct PidFile {
    pid: u32,
    port: u16,
    signal: Option<String>,
}

impl PidFile {
    fn render(&self) -> String {
        let mut s = format!("pid={}\nport={}\n", self.pid, self.port);
        if let Some(ref n) = self.signal {
            s.push_str(&format!("signal={}\n", n));
        }
        s
    }

    fn parse(body: &str) -> Result<Self> {
        let mut pid: Option<u32> = None;
        let mut port: Option<u16> = None;
        let mut signal: Option<String> = None;
        for line in body.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Some(rest) = line.strip_prefix("pid=") {
                pid = rest.parse().ok();
            } else if let Some(rest) = line.strip_prefix("port=") {
                port = rest.parse().ok();
            } else if let Some(rest) = line.strip_prefix("signal=") {
                signal = Some(rest.to_string());
            } else if pid.is_none() {
                // Tolerate single-int format: a bare number on the first line is the PID.
                pid = line.parse().ok();
            }
        }
        Ok(Self {
            pid: pid.ok_or_else(|| anyhow!("pidfile: missing pid"))?,
            port: port.unwrap_or(0),
            signal,
        })
    }
}

/// Probe whether a daemon is actually listening on the recorded port.
/// Used to differentiate a live process from a stale pidfile.
fn daemon_reachable(port: u16) -> bool {
    if port == 0 {
        return false;
    }
    use std::net::TcpStream;
    let addr: SocketAddr = match format!("127.0.0.1:{port}").parse() {
        Ok(a) => a,
        Err(_) => return false,
    };
    TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok()
}

pub(crate) fn start(
    args: StartArgs,
    stdout: &mut dyn Write,
    _stderr: &mut dyn Write,
) -> Result<()> {
    let dir = data_dir()?;
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("creating data dir {}", dir.display()))?;

    let bind: SocketAddr = args
        .listen
        .parse()
        .with_context(|| format!("parsing --listen {:?}", args.listen))?;

    let host_key = Arc::new(
        HostKey::load_or_create(&host_key_path(&dir))
            .with_context(|| "loading host signing key")?,
    );
    let db = open_db(&remote_db_path(&dir)).with_context(|| "opening omw-remote db")?;

    let pinned_origin = format!("https://{}", args.listen);
    let config = ServerConfig {
        bind,
        host_key,
        pinned_origins: vec![pinned_origin],
        inactivity_timeout: Duration::from_secs(60),
        revocations: RevocationList::new(),
        nonce_store: NonceStore::new(Duration::from_secs(60)),
        pairings: Some(Arc::new(Pairings::new(db))),
        shell: ShellSpec::default_for_host(),
        pty_registry: omw_server::SessionRegistry::new(),
        host_id: "omw-host".to_string(),
    };

    let signal_name = args.shutdown_signal.clone();
    let dir_for_runtime = dir.clone();
    let pidfile = pidfile_path(&dir);
    let pidfile_for_runtime = pidfile.clone();
    let signal_for_runtime = signal_name.clone();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("building tokio runtime")?;

    let result = runtime.block_on(async move {
        let listener = tokio::net::TcpListener::bind(config.bind)
            .await
            .with_context(|| format!("binding to {}", config.bind))?;
        let actual = listener.local_addr().context("local_addr")?;

        // Write pidfile NOW that we know the actual bound port.
        let pf = PidFile {
            pid: std::process::id(),
            port: actual.port(),
            signal: signal_for_runtime.clone(),
        };
        std::fs::write(&pidfile_for_runtime, pf.render())
            .with_context(|| format!("writing pidfile {}", pidfile_for_runtime.display()))?;

        // Watcher: on signal-file appearance, trip the shutdown future.
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        if let Some(name) = signal_for_runtime.clone() {
            let signal_file = dir_for_runtime.join(format!("{}.signal", name));
            tokio::spawn(async move {
                let mut tx_opt = Some(tx);
                loop {
                    if signal_file.exists() {
                        if let Some(t) = tx_opt.take() {
                            let _ = t.send(());
                        }
                        return;
                    }
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
            });
        } else {
            // No shutdown hook -> drop the sender so the receiver future
            // sleeps forever (graceful_shutdown waits on it anyway).
            drop(tx);
        }

        let router = make_router(config);
        let serve =
            axum::serve(listener, router.into_make_service()).with_graceful_shutdown(async move {
                let _ = rx.await;
            });

        serve.await.context("axum serve")
    });

    // Best-effort cleanup of pidfile on exit (success or error).
    let _ = std::fs::remove_file(&pidfile);

    writeln!(stdout, "remote daemon stopped")?;
    result
}

pub(crate) fn status(stdout: &mut dyn Write, _stderr: &mut dyn Write) -> Result<()> {
    let dir = data_dir()?;
    let pidfile = pidfile_path(&dir);

    if !pidfile.exists() {
        writeln!(stdout, "not running")?;
        return Ok(());
    }
    let body = std::fs::read_to_string(&pidfile)
        .with_context(|| format!("reading pidfile {}", pidfile.display()))?;
    let pf = match PidFile::parse(&body) {
        Ok(p) => p,
        Err(_) => {
            writeln!(stdout, "not running (unreadable pidfile)")?;
            return Ok(());
        }
    };

    if !daemon_reachable(pf.port) {
        writeln!(stdout, "not running (stale pidfile)")?;
        return Ok(());
    }

    let count: i64 = match open_db(&remote_db_path(&dir)) {
        Ok(conn) => conn
            .query_row(
                "SELECT COUNT(*) FROM devices WHERE revoked_at IS NULL",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0),
        Err(_) => 0,
    };
    writeln!(
        stdout,
        "running on 127.0.0.1:{} (pid {}), {} paired device(s)",
        pf.port, pf.pid, count
    )?;
    Ok(())
}

pub(crate) fn stop(args: StopArgs, stdout: &mut dyn Write, _stderr: &mut dyn Write) -> Result<()> {
    let dir = data_dir()?;
    let pidfile = pidfile_path(&dir);

    // Always honor --all, even if the daemon isn't running. The brief calls
    // this out: --all is a destructive admin op that should not depend on
    // daemon liveness.
    if args.all {
        let db_path = remote_db_path(&dir);
        let conn = open_db(&db_path)
            .with_context(|| format!("opening omw-remote db at {}", db_path.display()))?;
        let now = Utc::now().to_rfc3339();
        let n = conn
            .execute(
                "UPDATE devices SET revoked_at = ?1 WHERE revoked_at IS NULL",
                rusqlite::params![now],
            )
            .with_context(|| "revoking all devices")?;
        writeln!(stdout, "revoked {} device(s)", n)?;
    }

    if !pidfile.exists() {
        writeln!(stdout, "not running")?;
        return Ok(());
    }

    let body = std::fs::read_to_string(&pidfile)
        .with_context(|| format!("reading pidfile {}", pidfile.display()))?;
    let pf = PidFile::parse(&body).context("parsing pidfile")?;

    // Preferred shutdown path: write the named signal file the running
    // daemon's watcher polls for.
    if let Some(name) = pf.signal.as_deref() {
        let signal_file = dir.join(format!("{}.signal", name));
        std::fs::write(&signal_file, b"stop\n")
            .with_context(|| format!("writing signal file {}", signal_file.display()))?;
        // Wait briefly for the daemon to clean up its pidfile.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while pidfile.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        writeln!(stdout, "stop signaled")?;
        return Ok(());
    }

    // Fallback: no signal name in pidfile. On Unix we could SIGTERM the PID;
    // on Windows there's no portable approach without winapi. v0.4-thin
    // limitation — tests always exercise the signal-file path.
    writeln!(
        stdout,
        "no shutdown channel recorded in pidfile; please stop the daemon manually"
    )?;
    Ok(())
}
