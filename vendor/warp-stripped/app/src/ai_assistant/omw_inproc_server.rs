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
//!   2. `<exe_dir>/../Resources/omw-agent.mjs` — macOS .app bundle layout.
//!   3. `<exe_dir>/omw-agent.mjs` — flat bundle / Linux/Windows release.
//!   4. Workspace fallback: walk up from the running binary until we find
//!      `apps/omw-agent/bin/omw-agent.mjs`. Used during `cargo run`.
//!
//! When packaging the .dmg / .app, copy `apps/omw-agent/bin/omw-agent.mjs`
//! plus the entire `apps/omw-agent/dist/` and `apps/omw-agent/vendor/`
//! trees into `<.app>/Contents/Resources/`. Step 4 is the only path that
//! varies by build environment; the rest are stable across distros.

#![cfg(feature = "omw_local")]

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use omw_server::{serve_agent_loopback, AgentProcess, AgentProcessConfig};
use tokio::task::JoinHandle;

/// Default loopback bind address. Matches `OmwAgentState`'s
/// `DEFAULT_SERVER_URL` so the GUI dials the right port without env-var
/// configuration.
const DEFAULT_BIND: &str = "127.0.0.1:8788";

/// Process-wide handle. None until `ensure_running` succeeds once.
static SERVER_TASK: OnceLock<JoinHandle<()>> = OnceLock::new();

/// Idempotent. Spawns the agent surface on the supplied runtime if it
/// isn't already running and returns Ok. Returns Err on first-time
/// startup failure; future calls retry only after the OnceLock is
/// re-initialised (it isn't — failures are sticky for the process
/// lifetime, matching how a misconfigured binary would behave anyway).
pub fn ensure_running(runtime: &tokio::runtime::Handle) -> Result<(), String> {
    if SERVER_TASK.get().is_some() {
        return Ok(());
    }

    let kernel_path = locate_kernel_script()
        .ok_or_else(|| "omw-agent kernel script not found (set OMW_AGENT_BIN or bundle omw-agent.mjs alongside the binary)".to_string())?;

    // Spawn the kernel + bind the listener inside the runtime so axum's
    // hyper transport is happy. We synchronously block on the spawn step
    // via a oneshot to keep the API simple — the actual `axum::serve`
    // future is then detached as a background task.
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel::<Result<JoinHandle<()>, String>>(1);
    runtime.spawn(async move {
        let result = boot(kernel_path).await;
        let _ = ready_tx.send(result);
    });
    let task = ready_rx
        .recv()
        .map_err(|_| "in-process server boot channel dropped".to_string())??;

    let _ = SERVER_TASK.set(task);
    Ok(())
}

/// Async boot path. Spawns the agent stdio child and detaches the
/// `serve_agent_loopback` future from `omw-server` as a background task.
async fn boot(kernel_path: PathBuf) -> Result<JoinHandle<()>, String> {
    let kernel_path_str = kernel_path
        .to_str()
        .ok_or_else(|| format!("non-UTF-8 kernel path: {}", kernel_path.display()))?
        .to_string();

    let cfg = AgentProcessConfig {
        command: "node".into(),
        args: vec![kernel_path_str, "--serve-stdio".into()],
    };
    let agent = AgentProcess::spawn(cfg)
        .await
        .map_err(|e| format!("spawn omw-agent kernel: {e}"))?;

    let task = tokio::spawn(async move {
        if let Err(e) = serve_agent_loopback(agent, DEFAULT_BIND).await {
            log::error!("in-process omw-server exited: {e}");
        }
    });
    Ok(task)
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
            // live at <bundle>/Contents/Resources/.
            let app_resources = exe_dir.join("../Resources/omw-agent.mjs");
            if app_resources.exists() {
                return Some(app_resources);
            }
            // Flat bundle next to the binary.
            let flat = exe_dir.join("omw-agent.mjs");
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
