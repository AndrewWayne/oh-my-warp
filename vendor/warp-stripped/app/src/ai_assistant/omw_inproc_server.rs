//! In-process omw-server. Bundles the agent surface inside warp-oss so the
//! end user only launches one binary.
//!
//! On first call to [`ensure_running`], spawns:
//! - The omw-agent stdio child (Node + bundled `omw-agent.mjs`).
//! - An axum server on `127.0.0.1:8788` exposing
//!   [`omw_server::agent_router`].
//!
//! Both run on the [`OmwAgentState`] tokio runtime. Subsequent calls are
//! no-ops. Failures (Node missing, port in use, kernel script not found)
//! are surfaced as `Err(String)` so the caller can put them in
//! [`OmwAgentStatus::Failed`].
//!
//! ## Locating the omw-agent kernel
//!
//! Resolution order, first hit wins:
//!   1. `OMW_AGENT_BIN` env var — explicit override (used by tests).
//!   2. `<exe_dir>/../Resources/bin/omw-agent.mjs` — macOS .app bundle
//!      layout. The .mjs entry point dynamically imports
//!      `../dist/src/serve.js`, so we keep the `bin/` parent directory
//!      under Resources/ and place `dist/`, `vendor/`, `node_modules/`,
//!      and `package.json` as siblings of `bin/` (mirroring
//!      `apps/omw-agent/` in the source tree).
//!   3. `<exe_dir>/bin/omw-agent.mjs` — flat bundle / Linux/Windows
//!      release with the same `bin/` + sibling directories layout.
//!   4. Workspace fallback: walk up from the running binary until we find
//!      `apps/omw-agent/bin/omw-agent.mjs`. Used during `cargo run`.
//!
//! `scripts/build-mac-dmg.sh` does the bundling. Step 4 is the only path
//! that varies by build environment; the rest are stable across distros.

#![cfg(feature = "omw_local")]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use omw_server::{bind_agent_loopback, serve_agent_on_listener, AgentProcess, AgentProcessConfig};
use tokio::task::JoinHandle;

/// Cached state from a successful boot. We hold both the serve task's
/// `JoinHandle` (to detect a finished serve) AND a strong reference to
/// the [`AgentProcess`] (to detect a dead kernel via `is_alive`). Either
/// being unhealthy triggers a transparent re-boot in
/// [`ensure_running`].
struct BootedServer {
    serve_task: JoinHandle<()>,
    agent: Arc<AgentProcess>,
}

impl BootedServer {
    /// Abort the serve task so the listener releases port 8788 before
    /// the next boot tries to bind it. The prior `AgentProcess`'s Drop
    /// kills the kernel child via `kill_on_drop(true)`.
    fn shutdown(self) {
        self.serve_task.abort();
        // self.agent dropped here → previous kernel killed.
    }
}

/// Default loopback bind address. Matches `OmwAgentState`'s
/// `DEFAULT_SERVER_URL` so the GUI dials the right port without env-var
/// configuration.
const DEFAULT_BIND: &str = "127.0.0.1:8788";

/// Process-wide cached boot state. None until `ensure_running` succeeds
/// once. Held in a `Mutex<Option<...>>` (not `OnceLock`) because we may
/// need to re-boot after the kernel dies — sticking a dead handle here
/// forever was exactly the bug that produced the
/// "ensure_running — already running, no-op" log followed by 502s
/// against closed stdin.
static SERVER_TASK: Mutex<Option<BootedServer>> = Mutex::new(None);

/// Idempotent against a *live* server. If the previous boot's serve
/// task has finished OR the kernel child has exited, we drop the dead
/// state and re-boot transparently. Callers see a single
/// `Result<(), String>` either way.
pub fn ensure_running(runtime: &tokio::runtime::Handle) -> Result<(), String> {
    {
        let mut guard = SERVER_TASK.lock().expect("SERVER_TASK lock poisoned");
        if let Some(state) = guard.as_ref() {
            let serve_finished = state.serve_task.is_finished();
            let kernel_alive = state.agent.is_alive();
            if !serve_finished && kernel_alive {
                log::info!("omw# inproc: ensure_running — already running, no-op");
                return Ok(());
            }
            log::warn!(
                "omw# inproc: previous boot is unhealthy (serve_finished={serve_finished} \
                 kernel_alive={kernel_alive}) — re-booting"
            );
            // Take ownership so we can abort + drop cleanly. Aborting
            // the serve task releases port 8788 so the upcoming
            // `bind_agent_loopback` doesn't trip EADDRINUSE.
            if let Some(stale) = guard.take() {
                stale.shutdown();
            }
        }
    }
    // Brief pause to let the kernel exit logs flush and the OS release
    // the bound port. tokio's TcpListener drops are synchronous from
    // userspace's perspective but the OS may still be in TIME_WAIT.
    std::thread::sleep(std::time::Duration::from_millis(100));

    let kernel_path = locate_kernel_script()
        .ok_or_else(|| "omw-agent kernel script not found (set OMW_AGENT_BIN or bundle omw-agent.mjs alongside the binary)".to_string())?;
    log::info!("omw# inproc: kernel_path={}", kernel_path.display());

    // Spawn the kernel + bind the listener inside the runtime so axum's
    // hyper transport is happy. We synchronously block on the spawn step
    // via a oneshot to keep the API simple — the actual `axum::serve`
    // future is then detached as a background task.
    let (ready_tx, ready_rx) =
        std::sync::mpsc::sync_channel::<Result<BootedServer, String>>(1);
    log::info!("omw# inproc: dispatching boot onto agent runtime");
    runtime.spawn(async move {
        let result = boot(kernel_path).await;
        let _ = ready_tx.send(result);
    });
    log::info!("omw# inproc: blocking on boot result");
    let booted = ready_rx
        .recv()
        .map_err(|_| "in-process server boot channel dropped".to_string())??;

    {
        let mut guard = SERVER_TASK.lock().expect("SERVER_TASK lock poisoned");
        *guard = Some(booted);
    }
    log::info!("omw# inproc: ensure_running OK");
    Ok(())
}

/// Async boot path. Spawns the agent stdio child, **binds the loopback
/// listener synchronously** (so the GUI's subsequent session-create POST
/// can't race the bind), then detaches the serve future as a background
/// task.
async fn boot(kernel_path: PathBuf) -> Result<BootedServer, String> {
    let kernel_path_str = kernel_path
        .to_str()
        .ok_or_else(|| format!("non-UTF-8 kernel path: {}", kernel_path.display()))?
        .to_string();

    // Resolve the keychain helper before spawning so we can inject its
    // path into the kernel's environment. Without this, the kernel's
    // `getKeychainSecret` falls back to `omw-keychain-helper` on $PATH,
    // which isn't satisfied for dev `cargo run` builds or our shipped
    // .app bundle. Spawn ENOENT in Node 25.x crashes the kernel before
    // its 'error' listener fires (see investigation notes), and the
    // next session/create POST hits closed stdin → 502 Bad Gateway.
    let mut env: Vec<(String, String)> = Vec::new();
    if let Some(helper_path) = locate_keychain_helper() {
        if let Some(s) = helper_path.to_str() {
            log::info!("omw# inproc: OMW_KEYCHAIN_HELPER -> {s}");
            env.push(("OMW_KEYCHAIN_HELPER".into(), s.into()));
        } else {
            log::warn!(
                "omw# inproc: keychain helper path not UTF-8 ({}); skipping env injection",
                helper_path.display()
            );
        }
    } else {
        log::warn!(
            "omw# inproc: omw-keychain-helper not found; kernel will fall through to PATH \
             (set OMW_KEYCHAIN_HELPER if you see ENOENT crashes)"
        );
    }

    let cfg = AgentProcessConfig {
        command: "node".into(),
        args: vec![kernel_path_str.clone(), "--serve-stdio".into()],
        env,
    };
    log::info!(
        "omw# inproc: spawning agent kernel: node {kernel_path_str} --serve-stdio"
    );
    let agent = AgentProcess::spawn(cfg)
        .await
        .map_err(|e| format!("spawn omw-agent kernel: {e}"))?;
    log::info!("omw# inproc: kernel spawned");

    // Bind first so the port is up before we return to the caller; only
    // then detach the serve future. Without this split, axum::serve runs
    // in a tokio::spawn that may not have been polled by the time the
    // GUI's first POST /api/v1/agent/sessions hits the loopback.
    log::info!("omw# inproc: binding loopback {DEFAULT_BIND}");
    let listener = bind_agent_loopback(DEFAULT_BIND).await?;
    log::info!("omw# inproc: listener bound; detaching serve task");
    let agent_for_serve = agent.clone();
    let task = tokio::spawn(async move {
        if let Err(e) = serve_agent_on_listener(listener, agent_for_serve).await {
            log::error!("omw# inproc: serve exited: {e}");
        } else {
            log::info!("omw# inproc: serve exited cleanly");
        }
    });
    Ok(BootedServer {
        serve_task: task,
        agent,
    })
}

/// Walk the resolution order. Returns the first path that exists.
fn locate_kernel_script() -> Option<PathBuf> {
    if let Some(env_path) = std::env::var_os("OMW_AGENT_BIN") {
        let p = PathBuf::from(env_path);
        if p.exists() {
            return Some(p);
        }
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            // macOS .app bundle: <bundle>/Contents/MacOS/<exe>; resources
            // live at <bundle>/Contents/Resources/. The kernel layout
            // mirrors apps/omw-agent/, so the .mjs's relative imports
            // (`../dist/src/serve.js` etc.) resolve correctly.
            let app_resources = exe_dir.join("../Resources/bin/omw-agent.mjs");
            if app_resources.exists() {
                return Some(app_resources);
            }
            // Flat bundle next to the binary (Linux/Windows release).
            let flat = exe_dir.join("bin/omw-agent.mjs");
            if flat.exists() {
                return Some(flat);
            }
            // Workspace fallback for `cargo run` / `cargo build` output.
            // Walk up looking for `apps/omw-agent/bin/omw-agent.mjs`.
            if let Some(workspace_path) = walk_up_for_workspace_kernel(exe_dir) {
                return Some(workspace_path);
            }
        }
    }
    None
}

/// Resolve the `omw-keychain-helper` binary the kernel will spawn for
/// `keychain:omw/...` lookups. Mirrors [`locate_kernel_script`]:
///   1. `OMW_KEYCHAIN_HELPER` env var override (tests / local dev).
///   2. `<exe_dir>/../Resources/omw-keychain-helper` — macOS .app bundle.
///   3. `<exe_dir>/omw-keychain-helper` — flat bundle / Linux.
///   4. Workspace fallback: walk up looking for
///      `target/{release,debug}/omw-keychain-helper` so `cargo run` finds
///      the binary you just built without manual env-var setup.
fn locate_keychain_helper() -> Option<PathBuf> {
    if let Some(env_path) = std::env::var_os("OMW_KEYCHAIN_HELPER") {
        let p = PathBuf::from(env_path);
        if p.exists() {
            return Some(p);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            let app_resources = exe_dir.join("../Resources/omw-keychain-helper");
            if app_resources.exists() {
                return Some(app_resources);
            }
            let flat = exe_dir.join("omw-keychain-helper");
            if flat.exists() {
                return Some(flat);
            }
            if let Some(p) = walk_up_for_workspace_helper(exe_dir) {
                return Some(p);
            }
        }
    }
    None
}

fn walk_up_for_workspace_helper(start: &Path) -> Option<PathBuf> {
    let mut current = start;
    for _ in 0..10 {
        for profile in ["release", "debug"] {
            let candidate = current
                .join("target")
                .join(profile)
                .join("omw-keychain-helper");
            if candidate.exists() {
                return Some(candidate);
            }
        }
        match current.parent() {
            Some(parent) => current = parent,
            None => return None,
        }
    }
    None
}

fn walk_up_for_workspace_kernel(start: &Path) -> Option<PathBuf> {
    let mut current = start;
    for _ in 0..10 {
        let candidate = current
            .join("apps")
            .join("omw-agent")
            .join("bin")
            .join("omw-agent.mjs");
        if candidate.exists() {
            return Some(candidate);
        }
        match current.parent() {
            Some(parent) => current = parent,
            None => return None,
        }
    }
    None
}
