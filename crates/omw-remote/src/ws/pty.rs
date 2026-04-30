//! WS PTY handler — `GET /ws/v1/pty/:session_id`.
//!
//! On accepted handshake (signed-request auth + origin check), spawns a
//! shell PTY and bridges it to the WebSocket using the `Frame` envelope
//! defined in §7.2.
//!
//! - Inbound `Frame { kind: Input, payload: bytes }` -> PTY stdin.
//! - PTY output bytes are wrapped as `Frame { kind: Output, payload: bytes }`,
//!   signed with the host pairing key, and sent.
//! - `Frame { kind: Ping }` -> server replies with signed `Pong`.
//! - 60 s of inbound silence (configurable via `ServerConfig::inactivity_timeout`)
//!   -> server closes WS with code 4401.

use std::ffi::OsString;

use axum::extract::WebSocketUpgrade;
use axum::response::IntoResponse;

/// How to spawn the shell child for a WS PTY session.
///
/// Cross-platform shell selection lives here so tests can pin a deterministic
/// child (e.g. `printf`-only loops) without going through whatever the host's
/// default shell happens to be.
#[derive(Clone, Debug)]
pub struct ShellSpec {
    pub program: OsString,
    pub args: Vec<OsString>,
}

impl ShellSpec {
    /// Default shell for the current platform: `/bin/sh` on Unix, `cmd.exe`
    /// on Windows.
    pub fn default_for_host() -> Self {
        unimplemented!("Phase E executor: pick /bin/sh on unix, cmd.exe on windows")
    }
}

/// `GET /ws/v1/pty/:session_id` handler. Performs the §7.1 handshake checks,
/// then on success delegates to `handle_socket` which runs the bridge loop.
pub async fn ws_handler(_ws: WebSocketUpgrade) -> impl IntoResponse {
    unimplemented!("Phase E executor: verify signed handshake, accept upgrade, spawn PTY, run bridge");
    // Unreachable but lets the function have an inferred return type
    // matching the trait bound.
    #[allow(unreachable_code)]
    axum::http::StatusCode::INTERNAL_SERVER_ERROR
}
