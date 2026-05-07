//! `omw-server` — local-loopback backend shim.
//!
//! Phase C of v0.4-thin + Phase 2 of the inline-agent stack. Provides:
//! - An axum [`Router`] factory exposing the internal session registry on
//!   a `127.0.0.1` HTTP loopback (no auth — assumes in-process trust).
//! - A second axum [`Router`] factory ([`agent_router`]) exposing the
//!   agent surface (`/api/v1/agent/sessions` + `/ws/v1/agent/:id`).
//! - A [`SessionRegistry`] tracking live PTY sessions: register, list,
//!   look up by id, write input, subscribe to output, kill on drop.
//! - An [`AgentProcess`] owning the omw-agent stdio child and brokering
//!   JSON-RPC frames between GUI clients and the kernel.
//!
//! All registries are in-memory only; there is no persistence in v0.4-thin.
//!
//! See [PRD §8.2](../../../PRD.md#82-components) and
//! [PRD §9.1](../../../PRD.md#91-omw-server-loopback-only).

pub mod agent;
pub mod error;
pub mod handlers;
pub mod registry;

pub use agent::{AgentProcess, AgentProcessConfig, AgentProcessError};
pub use error::{Error, Result};
pub use registry::{
    ExternalSessionSpec, Session, SessionId, SessionMeta, SessionRegistry, SessionSpec,
};

use std::sync::Arc;

use axum::routing::{get, post};
use axum::Router;

/// Build the axum [`Router`] for the internal session registry surface.
///
/// Routes (all under `/internal/v1`):
/// - `POST   /sessions`            — register a new session, spawning a PTY.
/// - `GET    /sessions`            — list active sessions.
/// - `GET    /sessions/:id`        — get one session's metadata, or 404.
/// - `POST   /sessions/:id/input`  — write base64-encoded input bytes.
/// - `GET    /sessions/:id/pty`    — WebSocket bidirectional PTY frames.
/// - `DELETE /sessions/:id`        — kill a session.
pub fn router(registry: Arc<SessionRegistry>) -> Router {
    Router::new()
        .route(
            "/internal/v1/sessions",
            post(handlers::sessions::create).get(handlers::sessions::list),
        )
        .route(
            "/internal/v1/sessions/:id",
            get(handlers::sessions::get).delete(handlers::sessions::delete),
        )
        .route(
            "/internal/v1/sessions/:id/input",
            post(handlers::input::write),
        )
        .route(
            "/internal/v1/sessions/:id/pty",
            get(handlers::ws_pty::ws_handler),
        )
        .with_state(registry)
}

/// Build the axum [`Router`] for the agent surface.
///
/// Routes:
/// - `POST /api/v1/agent/sessions` — create an agent session, returns
///   `{ sessionId }`. Body matches the kernel's `session/create` params.
/// - `WS   /ws/v1/agent/:id`       — bidirectional event stream.
///
/// Compose with [`router`] using axum's `Router::merge`:
/// ```ignore
/// let app = router(registry).merge(agent_router(agent));
/// ```
/// Write a line to /tmp/omw-debug.log unconditionally. Bypasses the log
/// facade so we can verify code paths even if env_logger's filter or
/// target routing is dropping our log:: calls. Best-effort — errors are
/// silently ignored so debug instrumentation never breaks the request
/// path.
pub(crate) fn omw_debug(line: impl AsRef<str>) {
    use std::io::Write;
    let now = chrono::Local::now().format("%H:%M:%S%.3f");
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/omw-debug.log")
        .and_then(|mut f| writeln!(f, "{now} {}", line.as_ref()));
}

pub fn agent_router(agent: Arc<AgentProcess>) -> Router {
    log::error!(
        "omw# server: agent_router built (probe — confirms omw_server log:: routes to warp logger)"
    );
    omw_debug("omw# server: agent_router built (probe — direct file write)");
    Router::new()
        .route(
            "/api/v1/agent/sessions",
            post(handlers::agent::create_session),
        )
        .route("/ws/v1/agent/:id", get(handlers::agent::ws_handler))
        .with_state(agent)
}

/// Build the axum [`Router`] for the audit-append surface.
///
/// Single route: `POST /api/v1/audit/append`. The shared
/// [`omw_audit::AuditWriter`] is passed in as state. Compose with
/// [`router`] / [`agent_router`] via [`axum::Router::merge`].
pub fn audit_router(audit: handlers::audit::AuditState) -> Router {
    Router::new()
        .route("/api/v1/audit/append", post(handlers::audit::append))
        .with_state(audit)
}

/// Bind the loopback listener and serve the agent surface to completion.
///
/// Used by the bundled-in-warp-oss in-process server so the GUI doesn't
/// need a sidecar process. The function binds `bind_addr` (typically
/// `127.0.0.1:8788`) inline and serves [`agent_router`] against the
/// supplied [`AgentProcess`] until the future is dropped or the underlying
/// listener errors.
///
/// Lives in `omw-server` (not in warp-stripped) so it uses this crate's
/// own axum 0.7 — warp-stripped's axum 0.8.4 has a different
/// `axum::serve` signature and would not link against this crate's
/// `Router`.
pub async fn serve_agent_loopback(
    agent: Arc<AgentProcess>,
    bind_addr: &str,
) -> std::result::Result<(), String> {
    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .map_err(|e| format!("bind {bind_addr}: {e}"))?;
    serve_agent_on_listener(listener, agent).await
}

/// Bind only — return a [`tokio::net::TcpListener`] so the caller can
/// confirm the port is up before doing anything else (e.g. dialing the
/// loopback themselves). Pair with [`serve_agent_on_listener`] to drive
/// the server to completion.
pub async fn bind_agent_loopback(
    bind_addr: &str,
) -> std::result::Result<tokio::net::TcpListener, String> {
    tokio::net::TcpListener::bind(bind_addr)
        .await
        .map_err(|e| format!("bind {bind_addr}: {e}"))
}

/// Serve the agent surface against a pre-bound listener. Used by the
/// in-process server so the bind step finishes synchronously (eliminating
/// the listener-not-yet-up race against the GUI's first session POST).
pub async fn serve_agent_on_listener(
    listener: tokio::net::TcpListener,
    agent: Arc<AgentProcess>,
) -> std::result::Result<(), String> {
    let app = agent_router(agent);
    axum::serve(listener, app.into_make_service())
        .await
        .map_err(|e| format!("axum serve: {e}"))
}
