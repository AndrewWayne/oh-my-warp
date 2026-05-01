//! `POST /api/v1/pair/redeem` — unauthenticated pairing redemption.
//!
//! Wraps [`crate::pairing::Pairings::redeem`] in HTTP, mapping `RedeemError`
//! to spec §3.5 status codes and emitting the §3.2 wire response shape.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use serde::Deserialize;
use serde_json::json;

use crate::capability::Capability;
use crate::pairing::{PairToken, RedeemError};
use crate::server::AppState;

#[derive(Deserialize)]
pub struct PairRedeemRequest {
    #[serde(default)]
    pub v: Option<u8>,
    pub pairing_token: String,
    pub device_pubkey: String,
    pub device_name: String,
    #[serde(default)]
    pub platform: Option<String>,
    #[serde(default)]
    pub client_nonce: Option<String>,
}

pub(crate) async fn handler(
    State(state): State<AppState>,
    body: Result<Json<PairRedeemRequest>, axum::extract::rejection::JsonRejection>,
) -> axum::response::Response {
    let req = match body {
        Ok(Json(r)) => r,
        Err(_) => return error_response(StatusCode::BAD_REQUEST, "invalid_body"),
    };

    let token = match PairToken::from_base32(&req.pairing_token) {
        Ok(t) => t,
        Err(_) => return error_response(StatusCode::BAD_REQUEST, "invalid_body"),
    };

    let pubkey_bytes = match URL_SAFE_NO_PAD.decode(&req.device_pubkey) {
        Ok(b) => b,
        Err(_) => return error_response(StatusCode::BAD_REQUEST, "invalid_pubkey"),
    };
    if pubkey_bytes.len() != 32 {
        return error_response(StatusCode::BAD_REQUEST, "invalid_pubkey");
    }
    let mut device_pubkey = [0u8; 32];
    device_pubkey.copy_from_slice(&pubkey_bytes);

    let pairings = match &state.pairings {
        Some(p) => p,
        None => return error_response(StatusCode::INTERNAL_SERVER_ERROR, "internal"),
    };

    // Default capabilities for v0.4-thin: read-only set + pty:write so the
    // terminal works.
    let caps = [
        Capability::PtyRead,
        Capability::PtyWrite,
        Capability::AgentRead,
        Capability::AuditRead,
    ];

    let resp = match pairings.redeem(
        &token,
        &device_pubkey,
        &req.device_name,
        &state.host_key,
        &caps,
    ) {
        Ok(r) => r,
        Err(e) => {
            let (status, code) = match e {
                RedeemError::TokenUnknown => (StatusCode::NOT_FOUND, "token_unknown"),
                RedeemError::TokenExpired => (StatusCode::GONE, "token_expired"),
                RedeemError::TokenAlreadyUsed => (StatusCode::CONFLICT, "token_already_used"),
                RedeemError::InvalidPubkey => (StatusCode::BAD_REQUEST, "invalid_pubkey"),
                RedeemError::InvalidBody => (StatusCode::BAD_REQUEST, "invalid_body"),
                RedeemError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal"),
            };
            return error_response(status, code);
        }
    };

    // Build wire response per §3.2. Note: `PairRedeemResponse` does not
    // currently carry host_id/host_name/issued_at/expires_at; we synthesize
    // them here so we don't churn the Phase D struct.
    let cap_token = &resp.capability_token;
    // The issued_at/expires_at surfaced here mirror the capability token's
    // window (clients use this to know when to re-pair).
    let issued_at = cap_token.issued_at;
    let expires_at = cap_token.expires_at;

    let body = json!({
        "v": 1,
        "device_id": resp.device_id,
        "capabilities": cap_strings(&resp.capabilities),
        "capability_token": cap_token.to_base64url(),
        "host_pubkey": URL_SAFE_NO_PAD.encode(resp.host_pubkey),
        "host_id": state.host_id,
        "host_name": state.host_id,
        "issued_at": issued_at.to_rfc3339(),
        "expires_at": expires_at.to_rfc3339(),
    });
    (StatusCode::OK, Json(body)).into_response()
}

fn cap_strings(caps: &[Capability]) -> Vec<&'static str> {
    caps.iter()
        .map(|c| match c {
            Capability::PtyRead => "pty:read",
            Capability::PtyWrite => "pty:write",
            Capability::AgentRead => "agent:read",
            Capability::AgentWrite => "agent:write",
            Capability::AuditRead => "audit:read",
            Capability::PairAdmin => "pair:admin",
        })
        .collect()
}

fn error_response(status: StatusCode, code: &str) -> axum::response::Response {
    (status, Json(json!({ "error": code }))).into_response()
}
