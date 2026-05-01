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

use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::response::IntoResponse;
use bytes::Bytes;
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::{broadcast, mpsc};
use tokio::time::Instant;
use uuid::Uuid;

use crate::capability::CapabilityToken;
use crate::server::AppState;
use crate::ws::auth::WsSessionAuth;
use crate::ws::frame::{Frame, FrameKind};

/// How to spawn the shell child for a PTY session.
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

/// Public re-export shim — see [`crate::server::make_router`] for the live route.
pub async fn ws_handler(_ws: WebSocketUpgrade) -> impl IntoResponse {
    axum::http::StatusCode::INTERNAL_SERVER_ERROR
}

/// Bridge a fully-authenticated WS socket to a registered PTY session.
pub(crate) async fn handle_authed_socket(
    socket: WebSocket,
    state: AppState,
    capability: CapabilityToken,
    device_id: String,
    session_id: Uuid,
    mut pty_rx: broadcast::Receiver<Bytes>,
) {
    let auth = Arc::new(WsSessionAuth {
        last_inbound_seq: AtomicU64::new(u64::MAX),
        device_id,
        capability,
        revocations: state.revocations.clone(),
        ts_skew_seconds: 30,
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
                        FrameKind::Pong | FrameKind::Output | FrameKind::Control => {}
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
