//! Handlers for `/api/v1/agent/*` and `/ws/v1/agent/:id`.
//!
//! - `POST /api/v1/agent/sessions` — create an agent session, returns
//!   `{ sessionId }`. Body shape mirrors the kernel's `session/create`
//!   params: `{ providerConfig, model, systemPrompt?, cwd? }`.
//! - `WS  /ws/v1/agent/:sessionId` — bidirectional event stream.
//!     Server -> client: every kernel notification scoped to the session
//!     (assistant/delta, tool/call_started, tool/call_finished,
//!     turn/finished, error, agent/crashed) is forwarded as a Text frame
//!     containing the full JSON-RPC notification.
//!     Client -> server: Text frames carrying `{ "kind": "prompt", "prompt": "..." }`
//!     or `{ "kind": "cancel" }` translate to `session/prompt` /
//!     `session/cancel` requests on the kernel.

use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Path, State, WebSocketUpgrade};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde_json::{json, Value};

use crate::agent::AgentProcess;

/// `POST /api/v1/agent/sessions` — forwards to the kernel's `session/create`
/// JSON-RPC method and returns the resulting `{ sessionId }`.
pub async fn create_session(
    State(agent): State<Arc<AgentProcess>>,
    Json(body): Json<Value>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let result = agent
        .send_method("session/create", body)
        .await
        .map_err(|e| (StatusCode::SERVICE_UNAVAILABLE, e.to_string()))?;
    let session_id = result
        .get("sessionId")
        .and_then(|v| v.as_str())
        .ok_or((
            StatusCode::BAD_GATEWAY,
            "kernel did not return sessionId".to_string(),
        ))?
        .to_string();
    Ok((StatusCode::CREATED, Json(json!({ "sessionId": session_id }))))
}

/// `WS /ws/v1/agent/:sessionId` — bridge between the GUI client and the
/// kernel. We subscribe to the per-session notification bus, fan out
/// frames as text, and translate inbound text frames into kernel
/// requests.
pub async fn ws_handler(
    State(agent): State<Arc<AgentProcess>>,
    Path(session_id): Path<String>,
    ws: WebSocketUpgrade,
) -> axum::response::Response {
    ws.on_upgrade(move |socket| handle_socket(socket, agent, session_id))
}

async fn handle_socket(socket: WebSocket, agent: Arc<AgentProcess>, session_id: String) {
    use futures_util::{SinkExt, StreamExt};

    let (mut sink, mut stream) = socket.split();
    let mut rx = agent.subscribe(&session_id);

    // Outbound: notifications from the kernel -> WS Text frames.
    let session_id_for_outbound = session_id.clone();
    let mut outbound = tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(frame) => {
                    let line = match serde_json::to_string(&frame) {
                        Ok(s) => s,
                        Err(_) => continue,
                    };
                    if sink.send(Message::Text(line)).await.is_err() {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
        let _ = sink.close().await;
        // Reference the captured session_id so the closure types check;
        // we don't drop the agent's session bus on disconnect (so a
        // reconnect can resume notifications) — kernel-side cleanup is
        // explicit via session/cancel.
        let _ = session_id_for_outbound;
    });

    // Inbound: client text frames -> kernel requests.
    let agent_for_inbound = agent.clone();
    let session_id_for_inbound = session_id.clone();
    let mut inbound = tokio::spawn(async move {
        while let Some(msg) = stream.next().await {
            let msg = match msg {
                Ok(m) => m,
                Err(_) => break,
            };
            let text = match msg {
                Message::Text(t) => t,
                Message::Binary(_) => continue, // protocol is text-only
                Message::Close(_) => break,
                Message::Ping(_) | Message::Pong(_) => continue,
            };
            let parsed: Value = match serde_json::from_str(&text) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let kind = parsed.get("kind").and_then(|v| v.as_str()).unwrap_or("");
            match kind {
                "prompt" => {
                    let prompt = parsed
                        .get("prompt")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let _ = agent_for_inbound
                        .send_method(
                            "session/prompt",
                            json!({ "sessionId": session_id_for_inbound, "prompt": prompt }),
                        )
                        .await;
                }
                "cancel" => {
                    let _ = agent_for_inbound
                        .send_method(
                            "session/cancel",
                            json!({ "sessionId": session_id_for_inbound }),
                        )
                        .await;
                }
                _ => {
                    // Unknown kind — silently ignore in v0.4. Future
                    // additions (approval/decide etc.) extend this match.
                }
            }
        }
    });

    tokio::select! {
        _ = &mut outbound => { inbound.abort(); }
        _ = &mut inbound => { outbound.abort(); }
    }
}
