//! `omw-server` — local-loopback backend shim.
//!
//! Phase C of v0.4-thin. Provides:
//! - An axum [`Router`] factory exposing the internal session registry on a
//!   `127.0.0.1` HTTP loopback (no auth — assumes in-process trust).
//! - A [`SessionRegistry`] tracking live PTY sessions: register, list, look up
//!   by id, write input, subscribe to output, kill on drop.
//!
//! The registry is in-memory only; there is no persistence in v0.4-thin.
//!
//! See [PRD §8.2](../../../PRD.md#82-components) and
//! [PRD §9.1](../../../PRD.md#91-omw-server-loopback-only).

pub mod error;
pub mod handlers;
pub mod registry;

pub use error::{Error, Result};
pub use registry::{Session, SessionId, SessionMeta, SessionRegistry, SessionSpec};

use std::sync::Arc;

/// Build the axum [`Router`] for the internal session registry surface.
///
/// Routes (all under `/internal/v1`):
/// - `POST   /sessions`            — register a new session, spawning a PTY.
/// - `GET    /sessions`            — list active sessions.
/// - `GET    /sessions/:id`        — get one session's metadata, or 404.
/// - `POST   /sessions/:id/input`  — write base64-encoded input bytes.
/// - `GET    /sessions/:id/pty`    — WebSocket bidirectional PTY frames.
/// - `DELETE /sessions/:id`        — kill a session.
pub fn router(registry: Arc<SessionRegistry>) -> axum::Router {
    let _ = registry;
    unimplemented!("router(): wire SessionRegistry into axum Router (Executor)")
}
