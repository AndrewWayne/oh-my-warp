//! WS PTY handler — `GET /ws/v1/pty/:session_id`.
//!
//! On accepted handshake (signed-request auth + origin check), looks up the
//! session in the shared `omw_server::SessionRegistry` and bridges its PTY to
//! the WebSocket using the `Frame` envelope defined in §7.2.
//!
//! - Inbound `Frame { kind: Input, payload: bytes }` -> `registry.write_input`.
//! - PTY output bytes (from the session's broadcast channel) -> signed
//!   `Frame { kind: Output, ... }`.
//! - `Frame { kind: Ping }` -> server replies with signed `Pong`.
//! - 60 s of inbound silence (configurable via `ServerConfig::inactivity_timeout`)
//!   -> server closes WS with code 4408.

use std::ffi::OsString;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{CloseFrame, Message, WebSocket};
use bytes::Bytes;
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::sync::{broadcast, mpsc};
use tokio::time::Instant;
use uuid::Uuid;

use crate::capability::CapabilityToken;
use crate::server::AppState;
use crate::ws::auth::WsSessionAuth;
use crate::ws::frame::{Frame, FrameKind};

/// Inbound `Control` frame payload. Only `resize` is currently handled;
/// other control types are accepted but ignored. Field names match the
/// shape `apps/web-controller/src/pages/Terminal.tsx::sendControl` emits.
#[derive(Deserialize)]
struct ControlPayload {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    rows: Option<u16>,
    #[serde(default)]
    cols: Option<u16>,
}

/// How to spawn the shell child for a PTY session.
#[derive(Clone, Debug)]
pub struct ShellSpec {
    pub program: OsString,
    pub args: Vec<OsString>,
}

impl ShellSpec {
    /// Default shell for the current platform: `$SHELL` on Unix when set,
    /// `cmd.exe` on Windows.
    pub fn default_for_host() -> Self {
        #[cfg(windows)]
        {
            ShellSpec {
                program: "cmd.exe".into(),
                args: vec!["/Q".into()],
            }
        }
        #[cfg(not(windows))]
        {
            default_unix_shell_from_env(std::env::var_os("SHELL"))
        }
    }
}

#[cfg(not(windows))]
fn default_unix_shell_from_env(shell_env: Option<OsString>) -> ShellSpec {
    let program = shell_env
        .filter(|value| !value.as_os_str().is_empty())
        .unwrap_or_else(default_unix_shell_fallback);

    ShellSpec {
        program,
        args: vec![],
    }
}

#[cfg(all(not(windows), target_os = "macos"))]
fn default_unix_shell_fallback() -> OsString {
    "/bin/zsh".into()
}

#[cfg(all(not(windows), not(target_os = "macos")))]
fn default_unix_shell_fallback() -> OsString {
    "/bin/sh".into()
}

#[cfg(test)]
#[cfg(not(windows))]
mod tests {
    use super::*;

    #[test]
    fn unix_default_shell_prefers_non_empty_shell_env() {
        let spec = default_unix_shell_from_env(Some("/opt/homebrew/bin/fish".into()));

        assert_eq!(spec.program, OsString::from("/opt/homebrew/bin/fish"));
        assert!(spec.args.is_empty());
    }

    #[test]
    fn unix_default_shell_uses_platform_fallback_when_shell_env_is_missing_or_empty() {
        let expected: OsString = {
            #[cfg(target_os = "macos")]
            {
                "/bin/zsh".into()
            }
            #[cfg(not(target_os = "macos"))]
            {
                "/bin/sh".into()
            }
        };

        assert_eq!(default_unix_shell_from_env(None).program, expected);
        assert_eq!(
            default_unix_shell_from_env(Some(OsString::new())).program,
            expected
        );
    }
}

/// Bridge a fully-authenticated WS socket to a registered PTY session.
///
/// `snapshot` carries the ANSI-encoded current screen state from the
/// per-session vt100 parser (see `omw_server::SessionRegistry::subscribe_with_state`).
/// When non-empty it is shipped as the FIRST outbound `Output` frame, before
/// the live broadcast pump starts — that's how a freshly-attached client
/// renders the current TUI grid directly instead of replaying byte history.
pub(crate) async fn handle_authed_socket(
    socket: WebSocket,
    state: AppState,
    capability: CapabilityToken,
    device_id: String,
    session_id: Uuid,
    snapshot: Bytes,
    parser_size: (u16, u16),
    mut pty_rx: broadcast::Receiver<Bytes>,
) {
    eprintln!(
        "[omw-debug] handle_authed_socket: upgrade closure entered, session={session_id}, snapshot={} bytes",
        snapshot.len()
    );
    let auth = Arc::new(WsSessionAuth {
        last_inbound_seq: AtomicU64::new(u64::MAX),
        device_id,
        capability,
        revocations: state.revocations.clone(),
        // Match the WS-handshake skew bumped to 300 s — 30 s is too tight for
        // mobile clients.
        ts_skew_seconds: 300,
        host_pubkey: state.host_pubkey,
    });

    let host_key = state.host_key.clone();
    let registry = state.pty_registry.clone();
    let inactivity_timeout = state.inactivity_timeout;

    let (mut ws_sink, mut ws_stream) = socket.split();

    // Channel from outbound producers to the WS sink.
    enum Outbound {
        Frame(Frame),
        Close(u16, String),
    }
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Outbound>();

    // Outbound seq counter (server -> client).
    let server_seq = Arc::new(AtomicU64::new(0));

    // Last-inbound timestamp for inactivity tracking.
    let last_inbound = Arc::new(parking_lot_like::AtomicInstant::new(Instant::now()));

    // ---- Outbound writer task ----
    let writer_host = host_key.clone();
    let writer_seq = server_seq.clone();
    let mut writer_task = tokio::spawn(async move {
        while let Some(msg) = out_rx.recv().await {
            match msg {
                Outbound::Frame(mut frame) => {
                    let seq = writer_seq.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    frame.seq = seq;
                    frame.ts = Utc::now();
                    frame.sign_with_host(&writer_host);
                    let json = frame.to_json();
                    if ws_sink.send(Message::Text(json)).await.is_err() {
                        break;
                    }
                }
                Outbound::Close(code, reason) => {
                    let close = CloseFrame {
                        code,
                        reason: reason.into(),
                    };
                    let _ = ws_sink.send(Message::Close(Some(close))).await;
                    let _ = ws_sink.close().await;
                    break;
                }
            }
        }
    });

    // ---- Size hint Control frame ----
    // Send the laptop pane's actual (rows, cols) to the phone BEFORE the
    // snapshot/live byte stream. The phone must call `xterm.resize(cols,
    // rows)` to match — otherwise cursor-positioning bytes targeting rows
    // up to N (the laptop's height) clamp to the phone's smaller grid and
    // multiple writes pile up at the boundary, producing the duplicate-
    // render bug confirmed by the xterm-mid-stream-attach.test.ts fixture
    // test ("phone xterm SMALLER than laptop pane causes accumulation").
    {
        let (rows, cols) = parser_size;
        let payload = serde_json::json!({"type": "size", "rows": rows, "cols": cols});
        let frame = Frame {
            v: 1,
            seq: 0,
            ts: Utc::now(),
            kind: FrameKind::Control,
            payload: Bytes::from(serde_json::to_vec(&payload).unwrap_or_default()),
            sig: [0u8; 64],
        };
        let _ = out_tx.send(Outbound::Frame(frame));
    }

    // ---- Snapshot frame (tmux-style attach) ----
    // Ship the current vt100 screen state as the first Output frame BEFORE
    // wiring the live broadcast pump, so the client renders the present grid
    // directly instead of replaying byte history. The snapshot is empty for
    // sessions that have never received output (or for the initial 80×24
    // empty grid right after register, in which case it is just clear+home);
    // either way send it so the client always starts from a known state.
    if !snapshot.is_empty() {
        let frame = Frame {
            v: 1,
            seq: 0,
            ts: Utc::now(),
            kind: FrameKind::Output,
            payload: snapshot,
            sig: [0u8; 64],
        };
        let _ = out_tx.send(Outbound::Frame(frame));
    }

    // ---- Registry-broadcast -> outbound task ----
    let reader_tx = out_tx.clone();
    let mut pty_to_ws = tokio::spawn(async move {
        loop {
            match pty_rx.recv().await {
                Ok(chunk) => {
                    let frame = Frame {
                        v: 1,
                        seq: 0,
                        ts: Utc::now(),
                        kind: FrameKind::Output,
                        payload: chunk,
                        sig: [0u8; 64],
                    };
                    if reader_tx.send(Outbound::Frame(frame)).is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    // ---- Inbound task ----
    let inbound_tx = out_tx.clone();
    let inbound_auth = auth.clone();
    let inbound_last = last_inbound.clone();
    let inbound_registry = registry.clone();
    let mut inbound_task = tokio::spawn(async move {
        while let Some(msg) = ws_stream.next().await {
            let msg = match msg {
                Ok(m) => m,
                Err(_) => break,
            };
            match msg {
                Message::Text(t) => {
                    inbound_last.store(Instant::now());
                    let frame = match Frame::from_json(&t) {
                        Ok(f) => f,
                        Err(_) => {
                            let _ = inbound_tx.send(Outbound::Close(4400, "bad_frame".into()));
                            return;
                        }
                    };
                    let now = Utc::now();
                    if inbound_auth.verify_frame(&frame, now).is_err() {
                        let _ = inbound_tx.send(Outbound::Close(4401, "auth_failed".into()));
                        return;
                    }
                    match frame.kind {
                        FrameKind::Input => {
                            if inbound_registry
                                .write_input(session_id, &frame.payload)
                                .await
                                .is_err()
                            {
                                let _ = inbound_tx.send(Outbound::Close(4500, "pty_io".into()));
                                return;
                            }
                        }
                        FrameKind::Ping => {
                            let pong = Frame {
                                v: 1,
                                seq: 0,
                                ts: Utc::now(),
                                kind: FrameKind::Pong,
                                payload: frame.payload.clone(),
                                sig: [0u8; 64],
                            };
                            let _ = inbound_tx.send(Outbound::Frame(pong));
                        }
                        FrameKind::Control => {
                            // Resize control: { "type": "resize", "rows": N, "cols": N }.
                            // Updates the per-session vt100 parser's screen
                            // size so subsequent snapshots match the phone's
                            // viewport. Other control types are ignored —
                            // the wire format is small but extensible.
                            if let Ok(payload) =
                                serde_json::from_slice::<ControlPayload>(&frame.payload)
                            {
                                if payload.kind == "resize" {
                                    let _ = inbound_registry.resize(
                                        session_id,
                                        payload.rows.unwrap_or(24),
                                        payload.cols.unwrap_or(80),
                                    );
                                }
                            }
                        }
                        FrameKind::Pong | FrameKind::Output => {}
                    }
                }
                Message::Binary(_) => {
                    let _ = inbound_tx.send(Outbound::Close(4400, "binary_unsupported".into()));
                    return;
                }
                Message::Close(_) => break,
                Message::Ping(_) | Message::Pong(_) => {}
            }
        }
    });

    // ---- Inactivity watchdog ----
    let inactivity_tx = out_tx.clone();
    let inactivity_last = last_inbound.clone();
    let mut inactivity_task = tokio::spawn(async move {
        let tick = Duration::from_millis(200);
        loop {
            tokio::time::sleep(tick).await;
            let elapsed = Instant::now().duration_since(inactivity_last.load());
            if elapsed >= inactivity_timeout {
                let _ = inactivity_tx.send(Outbound::Close(4408, "inactivity_timeout".into()));
                return;
            }
        }
    });

    drop(out_tx);

    tokio::select! {
        _ = &mut inbound_task => {
            pty_to_ws.abort();
            inactivity_task.abort();
            let _ = writer_task.await;
        }
        _ = &mut pty_to_ws => {
            inbound_task.abort();
            inactivity_task.abort();
            let _ = writer_task.await;
        }
        _ = &mut inactivity_task => {
            inbound_task.abort();
            pty_to_ws.abort();
            let _ = writer_task.await;
        }
        _ = &mut writer_task => {
            inbound_task.abort();
            pty_to_ws.abort();
            inactivity_task.abort();
        }
    }
}

/// Tiny atomic-Instant shim.
mod parking_lot_like {
    use std::sync::Mutex;
    use tokio::time::Instant;

    pub struct AtomicInstant(Mutex<Instant>);
    impl AtomicInstant {
        pub fn new(t: Instant) -> Self {
            Self(Mutex::new(t))
        }
        pub fn load(&self) -> Instant {
            *self.0.lock().expect("atomic instant poisoned")
        }
        pub fn store(&self, t: Instant) {
            *self.0.lock().expect("atomic instant poisoned") = t;
        }
    }
}
