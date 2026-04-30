//! Handler for `GET /internal/v1/sessions/:id/pty` (WebSocket upgrade).
//!
//! Server -> client: every PTY output chunk is forwarded as a binary frame.
//! Client -> server: every binary frame is written into PTY input.

use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Path, State, WebSocketUpgrade};
use axum::http::StatusCode;
use axum::response::IntoResponse;

use crate::registry::SessionId;
use crate::SessionRegistry;

/// `GET /internal/v1/sessions/:id/pty` (WebSocket upgrade).
pub async fn ws_handler(
    State(registry): State<Arc<SessionRegistry>>,
    Path(id): Path<SessionId>,
    ws: WebSocketUpgrade,
) -> axum::response::Response {
    // Reject the upgrade with 404 if the session is unknown.
    let rx = match registry.subscribe(id) {
        Some(rx) => rx,
        None => return (StatusCode::NOT_FOUND, "session not found").into_response(),
    };

    ws.on_upgrade(move |socket| handle_socket(socket, registry, id, rx))
}

async fn handle_socket(
    socket: WebSocket,
    registry: Arc<SessionRegistry>,
    id: SessionId,
    mut rx: tokio::sync::broadcast::Receiver<bytes::Bytes>,
) {
    use futures_util::{SinkExt, StreamExt};

    let (mut sink, mut stream) = socket.split();

    // Outbound: PTY output -> WS binary frames.
    let mut outbound = tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(chunk) => {
                    if sink.send(Message::Binary(chunk.to_vec())).await.is_err() {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
        let _ = sink.close().await;
    });

    // Inbound: WS frames -> PTY input.
    let mut inbound = tokio::spawn(async move {
        while let Some(msg) = stream.next().await {
            let msg = match msg {
                Ok(m) => m,
                Err(_) => break,
            };
            match msg {
                Message::Binary(b) => {
                    if registry.write_input(id, &b).await.is_err() {
                        break;
                    }
                }
                Message::Text(t) => {
                    if registry.write_input(id, t.as_bytes()).await.is_err() {
                        break;
                    }
                }
                Message::Close(_) => break,
                Message::Ping(_) | Message::Pong(_) => {}
            }
        }
    });

    // When either half ends, abort the other so we don't leak a task.
    tokio::select! {
        _ = &mut outbound => { inbound.abort(); }
        _ = &mut inbound  => { outbound.abort(); }
    }
}
