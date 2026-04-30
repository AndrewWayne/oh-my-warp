//! Handler for `POST /internal/v1/sessions/:id/input`.
//!
//! Body shape is `{ "bytes": "<base64>" }`. Decodes and forwards to
//! `SessionRegistry::write_input`.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use base64::Engine;
use serde::Deserialize;

use crate::registry::SessionId;
use crate::{Error, SessionRegistry};

#[derive(Debug, Deserialize)]
pub struct InputBody {
    pub bytes: String,
}

/// `POST /internal/v1/sessions/:id/input`
pub async fn write(
    State(registry): State<Arc<SessionRegistry>>,
    Path(id): Path<SessionId>,
    Json(body): Json<InputBody>,
) -> Result<StatusCode, Error> {
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(body.bytes.as_bytes())
        .map_err(|e| Error::BadRequest(format!("invalid base64: {e}")))?;
    registry.write_input(id, &decoded).await?;
    Ok(StatusCode::NO_CONTENT)
}
