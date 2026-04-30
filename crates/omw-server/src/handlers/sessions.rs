//! Handlers for `/internal/v1/sessions` (list / register / get / delete).

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde_json::json;

use crate::registry::{SessionId, SessionSpec};
use crate::{Error, SessionRegistry};

/// `POST /internal/v1/sessions` — register a new session, spawning a PTY.
pub async fn create(
    State(registry): State<Arc<SessionRegistry>>,
    Json(spec): Json<SessionSpec>,
) -> Result<impl IntoResponse, Error> {
    let name = spec.name.clone();
    let id = registry.register(spec).await?;
    let meta = registry
        .get(id)
        .ok_or_else(|| Error::Io("session vanished immediately after register".into()))?;
    let body = json!({
        "id": id.to_string(),
        "name": name,
        "created_at": meta.created_at,
        "alive": meta.alive,
    });
    Ok((StatusCode::CREATED, Json(body)))
}

/// `GET /internal/v1/sessions` — list active sessions.
pub async fn list(State(registry): State<Arc<SessionRegistry>>) -> impl IntoResponse {
    let sessions = registry.list();
    Json(json!({ "sessions": sessions }))
}

/// `GET /internal/v1/sessions/:id` — one session's metadata, or 404.
pub async fn get(
    State(registry): State<Arc<SessionRegistry>>,
    Path(id): Path<SessionId>,
) -> Result<impl IntoResponse, Error> {
    let meta = registry.get(id).ok_or(Error::NotFound(id))?;
    Ok(Json(meta))
}

/// `DELETE /internal/v1/sessions/:id` — kill the session and remove.
pub async fn delete(
    State(registry): State<Arc<SessionRegistry>>,
    Path(id): Path<SessionId>,
) -> Result<impl IntoResponse, Error> {
    registry.kill(id).await?;
    Ok(StatusCode::NO_CONTENT)
}
