//! `/api/v1/sessions` — signed CRUD for PTY sessions stored in the registry.
//!
//! Spawns shells via `omw_server::SessionRegistry`. Authentication uses the
//! same §4 signed-request ladder as the WS handshake; required capability
//! varies per route (PtyWrite for create/delete, PtyRead for list).

use axum::body::to_bytes;
use axum::extract::{Path, Request, State};
use axum::http::{Method, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use chrono::Utc;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use uuid::Uuid;

use crate::auth::{AuthError, CanonicalRequest, Verifier};
use crate::capability::{Capability, CapabilityToken};
use crate::server::AppState;
use omw_server::SessionSpec;

/// Body for `POST /api/v1/sessions`. All fields optional.
#[derive(Deserialize, Default)]
pub struct CreateSessionRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub shell: Option<String>,
}

pub(crate) async fn create(
    State(state): State<AppState>,
    request: Request,
) -> axum::response::Response {
    let (parts, body) = request.into_parts();
    let body_bytes = match to_bytes(body, 1024 * 64).await {
        Ok(b) => b,
        Err(_) => return err(StatusCode::BAD_REQUEST, "invalid_body"),
    };
    if let Err(resp) = verify_signed(
        &state,
        &Method::POST,
        "/api/v1/sessions",
        "",
        &body_bytes,
        &parts.headers,
        Capability::PtyWrite,
    ) {
        return *resp;
    }

    let req: CreateSessionRequest = if body_bytes.is_empty() {
        CreateSessionRequest::default()
    } else {
        match serde_json::from_slice(&body_bytes) {
            Ok(r) => r,
            Err(_) => return err(StatusCode::BAD_REQUEST, "invalid_body"),
        }
    };

    let name = req.name.unwrap_or_else(|| "session".to_string());
    let (command, args) = resolve_shell(&state, req.shell.as_deref());

    let spec = SessionSpec {
        name: name.clone(),
        command,
        args,
        cwd: None,
        env: Some(terminal_session_env()),
        cols: None,
        rows: None,
    };

    let id = match state.pty_registry.register(spec).await {
        Ok(id) => id,
        Err(e) => {
            return err_with_msg(
                StatusCode::INTERNAL_SERVER_ERROR,
                "spawn_failed",
                &e.to_string(),
            )
        }
    };
    let meta = match state.pty_registry.get(id) {
        Some(m) => m,
        None => return err(StatusCode::INTERNAL_SERVER_ERROR, "spawn_failed"),
    };

    (
        StatusCode::OK,
        Json(json!({
            "id": id.to_string(),
            "name": name,
            "created_at": meta.created_at.to_rfc3339(),
        })),
    )
        .into_response()
}

pub(crate) async fn list(
    State(state): State<AppState>,
    request: Request,
) -> axum::response::Response {
    let (parts, body) = request.into_parts();
    let body_bytes = match to_bytes(body, 1024).await {
        Ok(b) => b,
        Err(_) => return err(StatusCode::BAD_REQUEST, "invalid_body"),
    };
    if let Err(resp) = verify_signed(
        &state,
        &Method::GET,
        "/api/v1/sessions",
        "",
        &body_bytes,
        &parts.headers,
        Capability::PtyRead,
    ) {
        return *resp;
    }
    let sessions: Vec<Value> = state
        .pty_registry
        .list()
        .into_iter()
        .map(|m| {
            json!({
                "id": m.id.to_string(),
                "name": m.name,
                "created_at": m.created_at.to_rfc3339(),
                "alive": m.alive,
            })
        })
        .collect();
    Json(json!({ "sessions": sessions })).into_response()
}

pub(crate) async fn delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
    request: Request,
) -> axum::response::Response {
    let (parts, body) = request.into_parts();
    let body_bytes = match to_bytes(body, 1024).await {
        Ok(b) => b,
        Err(_) => return err(StatusCode::BAD_REQUEST, "invalid_body"),
    };
    let path = format!("/api/v1/sessions/{id}");
    if let Err(resp) = verify_signed(
        &state,
        &Method::DELETE,
        &path,
        "",
        &body_bytes,
        &parts.headers,
        Capability::PtyWrite,
    ) {
        return *resp;
    }
    let uuid = match Uuid::parse_str(&id) {
        Ok(u) => u,
        Err(_) => return err(StatusCode::NOT_FOUND, "session_not_found"),
    };
    match state.pty_registry.kill(uuid).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(_) => err(StatusCode::NOT_FOUND, "session_not_found"),
    }
}

/// Signed-request verification shared by all session routes. Returns Ok with
/// the resolved device id on success, or Err with the HTTP response to send.
fn verify_signed(
    state: &AppState,
    method: &Method,
    path: &str,
    query: &str,
    body: &[u8],
    headers: &axum::http::HeaderMap,
    required: Capability,
) -> Result<String, Box<axum::response::Response>> {
    let auth_header = match headers.get("authorization").and_then(|h| h.to_str().ok()) {
        Some(s) => s,
        None => return Err(boxed_err(StatusCode::UNAUTHORIZED, "missing_authorization")),
    };
    let cap_b64 = match auth_header.strip_prefix("Bearer ") {
        Some(s) => s.trim(),
        None => return Err(boxed_err(StatusCode::UNAUTHORIZED, "bad_authorization")),
    };
    let sig_b64 = match headers.get("x-omw-signature").and_then(|h| h.to_str().ok()) {
        Some(s) => s,
        None => return Err(boxed_err(StatusCode::UNAUTHORIZED, "missing_signature")),
    };
    let nonce = match headers.get("x-omw-nonce").and_then(|h| h.to_str().ok()) {
        Some(s) => s,
        None => return Err(boxed_err(StatusCode::UNAUTHORIZED, "missing_nonce")),
    };
    let ts = match headers.get("x-omw-ts").and_then(|h| h.to_str().ok()) {
        Some(s) => s,
        None => return Err(boxed_err(StatusCode::UNAUTHORIZED, "missing_ts")),
    };

    let sig_bytes = match URL_SAFE_NO_PAD.decode(sig_b64) {
        Ok(b) if b.len() == 64 => b,
        _ => {
            return Err(boxed_err(
                StatusCode::UNAUTHORIZED,
                "bad_signature_encoding",
            ))
        }
    };
    let mut sig = [0u8; 64];
    sig.copy_from_slice(&sig_bytes);

    let body_sha256 = sha256(body);

    let cap_token = match CapabilityToken::from_base64url(cap_b64) {
        Ok(t) => t,
        Err(_) => return Err(boxed_err(StatusCode::UNAUTHORIZED, "capability_invalid")),
    };

    let canonical = CanonicalRequest {
        method: method.as_str().to_string(),
        path: path.to_string(),
        query: query.to_string(),
        ts: ts.to_string(),
        nonce: nonce.to_string(),
        body_sha256,
        device_id: cap_token.device_id.clone(),
        protocol_version: 1,
    };

    let verifier = Verifier::new(state.host_pubkey, state.nonce_store.clone());
    let now = Utc::now();
    match verifier.verify(&canonical, &sig, cap_b64, required, now) {
        Ok(id) => {
            if state.revocations.is_revoked(&id) {
                return Err(boxed_err(StatusCode::UNAUTHORIZED, "device_revoked"));
            }
            Ok(id)
        }
        Err(e) => {
            let (status, code) = match e {
                AuthError::CapabilityScope => (StatusCode::FORBIDDEN, "capability_scope"),
                AuthError::CapabilityExpired => (StatusCode::UNAUTHORIZED, "capability_expired"),
                AuthError::CapabilityInvalid => (StatusCode::UNAUTHORIZED, "capability_invalid"),
                AuthError::SignatureInvalid => (StatusCode::UNAUTHORIZED, "signature_invalid"),
                AuthError::DeviceRevoked => (StatusCode::UNAUTHORIZED, "device_revoked"),
                AuthError::TsSkew => (StatusCode::UNAUTHORIZED, "ts_skew"),
                AuthError::NonceReplayed => (StatusCode::UNAUTHORIZED, "nonce_replayed"),
                AuthError::InvalidBody => (StatusCode::BAD_REQUEST, "invalid_body"),
            };
            Err(boxed_err(status, code))
        }
    }
}

fn resolve_shell(state: &AppState, requested: Option<&str>) -> (String, Vec<String>) {
    match requested {
        None | Some("default") | Some("") => {
            let prog = state.shell.program.to_string_lossy().into_owned();
            let args = state
                .shell
                .args
                .iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect();
            (prog, args)
        }
        Some(other) => (other.to_string(), Vec::new()),
    }
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    let h = Sha256::digest(bytes);
    let mut out = [0u8; 32];
    out.copy_from_slice(&h);
    out
}

fn terminal_session_env() -> HashMap<String, String> {
    HashMap::from([
        ("TERM".to_string(), "xterm-256color".to_string()),
        ("COLORTERM".to_string(), "truecolor".to_string()),
        ("TERM_PROGRAM".to_string(), "omw".to_string()),
    ])
}

/// Build an error response. Sized as `Response` for direct return; wrap in
/// `Box::new` at signed-request call sites to keep the boxed `Result` shape.
fn err(status: StatusCode, code: &str) -> axum::response::Response {
    (status, Json(json!({ "error": code }))).into_response()
}

fn boxed_err(status: StatusCode, code: &str) -> Box<axum::response::Response> {
    Box::new(err(status, code))
}

fn err_with_msg(status: StatusCode, code: &str, msg: &str) -> axum::response::Response {
    (status, Json(json!({ "error": code, "message": msg }))).into_response()
}

#[cfg(test)]
mod tests {
    use super::terminal_session_env;

    #[test]
    fn terminal_session_env_advertises_color_terminal_capabilities() {
        let env = terminal_session_env();
        assert_eq!(env.get("TERM").map(String::as_str), Some("xterm-256color"));
        assert_eq!(env.get("COLORTERM").map(String::as_str), Some("truecolor"));
        assert_eq!(env.get("TERM_PROGRAM").map(String::as_str), Some("omw"));
    }
}
