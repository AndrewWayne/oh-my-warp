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
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::response::IntoResponse;
use bytes::Bytes;
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio::time::Instant;

use crate::capability::CapabilityToken;
use crate::server::AppState;
use crate::ws::auth::WsSessionAuth;
use crate::ws::frame::{Frame, FrameKind};
use omw_pty::{Pty, PtyCommand};

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
        #[cfg(windows)]
        {
            ShellSpec {
                program: "cmd.exe".into(),
                args: vec!["/Q".into()],
            }
        }
        #[cfg(not(windows))]
        {
            ShellSpec {
                program: "/bin/sh".into(),
                args: vec![],
            }
        }
    }
}

/// `GET /ws/v1/pty/:session_id` handler. Performs the §7.1 handshake checks,
/// then on success delegates to `handle_socket` which runs the bridge loop.
///
/// Note: the live route in [`crate::server::make_router`] performs the §7.1
/// signed-request + origin checks and then calls [`handle_authed_socket`]
/// directly. This thin wrapper exists for the public re-export path; it
/// rejects with 500 because authentication context isn't available without
/// the full router state.
pub async fn ws_handler(_ws: WebSocketUpgrade) -> impl IntoResponse {
    axum::http::StatusCode::INTERNAL_SERVER_ERROR
}

/// Bridge a fully-authenticated WS socket to a freshly spawned shell PTY.
///
/// Visible only within the crate; called from `server::ws_handler` after
/// the §7.1 + §8.2 handshake checks pass.
pub(crate) async fn handle_authed_socket(
    socket: WebSocket,
    state: AppState,
    capability: CapabilityToken,
    device_id: String,
) {
    // Spawn a shell PTY for this session.
    let mut cmd = PtyCommand::new(state.shell.program.clone());
    cmd = cmd.args(state.shell.args.clone());
    let mut pty = match Pty::spawn(cmd).await {
        Ok(p) => p,
        Err(_) => {
            // Couldn't spawn the shell; close with auth_failed-shaped code so
            // the client sees a clean shutdown.
            let _ = close_socket(socket, 4500, "pty_spawn_failed").await;
            return;
        }
    };
    let mut pty_reader = pty.reader().expect("reader available");
    let mut pty_writer = pty.writer().expect("writer available");

    let auth = Arc::new(WsSessionAuth {
        last_inbound_seq: AtomicU64::new(u64::MAX),
        device_id,
        capability,
        revocations: state.revocations.clone(),
        ts_skew_seconds: 30,
        host_pubkey: state.host_pubkey,
    });

    let host_key = state.host_key.clone();
    let inactivity_timeout = state.inactivity_timeout;

    let (mut ws_sink, mut ws_stream) = socket.split();

    // Channel from outbound producers (PTY reader, ping responder, inbound
    // task on close) to the WS sink. Lets us serialize all writes through a
    // single task, which simplifies sequencing and close-frame handling.
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

    // ---- PTY -> outbound task ----
    let reader_tx = out_tx.clone();
    let mut pty_to_ws = tokio::spawn(async move {
        let mut buf = vec![0u8; 4096];
        loop {
            match pty_reader.read(&mut buf).await {
                Ok(0) => break, // EOF
                Ok(n) => {
                    let frame = Frame {
                        v: 1,
                        seq: 0, // assigned by writer task
                        ts: Utc::now(),
                        kind: FrameKind::Output,
                        payload: Bytes::copy_from_slice(&buf[..n]),
                        sig: [0u8; 64],
                    };
                    if reader_tx.send(Outbound::Frame(frame)).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    // ---- Inbound task ----
    let inbound_tx = out_tx.clone();
    let inbound_auth = auth.clone();
    let inbound_last = last_inbound.clone();
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
                            if pty_writer.write_all(&frame.payload).await.is_err() {
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
                        FrameKind::Pong | FrameKind::Output | FrameKind::Control => {
                            // Pong/control acceptable; output from device is
                            // not expected but also not a hard error.
                        }
                    }
                }
                Message::Binary(_) => {
                    // Spec only specifies text-JSON envelopes. Reject.
                    let _ = inbound_tx.send(Outbound::Close(4400, "binary_unsupported".into()));
                    return;
                }
                Message::Close(_) => break,
                Message::Ping(_) | Message::Pong(_) => {
                    // Transport-level pings; not part of the §7.5 signed
                    // heartbeat. Axum auto-replies to pings.
                }
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

    // Drop our local sender clone so the writer task exits when all producers
    // close.
    drop(out_tx);

    // Wait for any branch to finish, then tear down the rest. The PTY drop
    // will kill the child shell.
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

    // Ensure the PTY is killed even if read/write loops exited cleanly.
    let _ = pty.kill();
}

async fn close_socket(socket: WebSocket, code: u16, reason: &str) -> Result<(), ()> {
    let (mut sink, _stream) = socket.split();
    let close = CloseFrame {
        code,
        reason: reason.to_string().into(),
    };
    let _ = sink.send(Message::Close(Some(close))).await;
    let _ = sink.close().await;
    Ok(())
}

/// Tiny atomic-Instant shim. `std::sync::atomic` doesn't carry `Instant`
/// directly; we use a `Mutex<Instant>` here because PartialOrd matters and
/// the contention is one writer + one reader per session.
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
