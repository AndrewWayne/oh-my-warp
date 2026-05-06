//! Handler for `POST /api/v1/audit/append`.
//!
//! Single-writer audit endpoint per [PRD §8.3](../../../../PRD.md#83-component-ownership-map):
//! every component (omw-agent over the WS, omw-remote over HTTP, the
//! GUI for control-flow events) posts here; the handler serializes
//! through one `AuditWriter` behind a `tokio::sync::Mutex`.
//!
//! Body shape (must match `apps/omw-agent/src/audit-emit.ts` and
//! `crates/omw-server/src/agent/process.rs`):
//!
//! ```json
//! {
//!   "kind": "tool_call_requested",
//!   "session_id": "f8e6...uuid...",
//!   "fields": { "tool": "bash", "command": "ls", ... }
//! }
//! ```
//!
//! Returns `201 Created` with `{ "hash": "<sha256-hex>" }` on success.
//! Errors map to `400 Bad Request` (malformed body) or `500 Internal
//! Server Error` (filesystem failure).

use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use omw_audit::AuditWriter;
use serde::Deserialize;
use serde_json::json;
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct AppendRequest {
    pub kind: String,
    pub session_id: Uuid,
    #[serde(default)]
    pub fields: serde_json::Value,
}

/// Shared audit writer state passed via axum `State`.
pub type AuditState = Arc<Mutex<AuditWriter>>;

pub async fn append(
    State(audit): State<AuditState>,
    Json(body): Json<AppendRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let mut writer = audit.lock().await;
    let hash = writer
        .append(&body.kind, body.session_id, body.fields)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("audit append: {e}")))?;
    Ok((StatusCode::CREATED, Json(json!({ "hash": hash }))))
}
