//! `GET /api/v1/host-info` — unauthenticated discovery endpoint.
//!
//! Lets a client learn the host pubkey before pair-redeem so it can verify
//! capability tokens it later receives.

use axum::extract::State;
use axum::Json;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use serde_json::{json, Value};

use crate::server::AppState;

pub(crate) async fn handler(State(state): State<AppState>) -> Json<Value> {
    let host_pubkey_b64 = URL_SAFE_NO_PAD.encode(state.host_pubkey);
    Json(json!({
        "v": 1,
        "host_id": state.host_id,
        "host_pubkey": host_pubkey_b64,
        "protocol_version": 1,
        "capabilities_supported": [
            "pty:read",
            "pty:write",
            "agent:read",
            "agent:write",
            "audit:read",
        ],
    }))
}
