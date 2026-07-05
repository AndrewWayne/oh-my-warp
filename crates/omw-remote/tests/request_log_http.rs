//! Wiring for the `request_log` audit trail on the HTTP surfaces
//! (spec §4.2 step 10 + §6.3): pair-redeem and signed session requests each
//! append exactly one accept/reject row.

#[path = "http_common/mod.rs"]
mod http_common;

use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use ed25519_dalek::SigningKey;
use serde_json::json;

use http_common::{http_request, pair_device, sign_headers, spawn_server};
use omw_remote::Capability;

const REDEEM_ROUTE: &str = "/api/v1/pair/redeem";
const SESSIONS_ROUTE: &str = "/api/v1/sessions";

fn redeem_body(token_b32: &str, pk_b64: &str, nonce: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "v": 1,
        "pairing_token": token_b32,
        "device_pubkey": pk_b64,
        "device_name": "test-device",
        "platform": "test",
        "client_nonce": nonce,
    }))
    .unwrap()
}

fn device_pubkey_b64(seed: u8) -> String {
    let device = SigningKey::from_bytes(&[seed; 32]);
    URL_SAFE_NO_PAD.encode(device.verifying_key().to_bytes())
}

#[tokio::test]
async fn pair_redeem_accept_appends_row() {
    let f = spawn_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    let token = f.pairings.issue(Duration::from_secs(600)).expect("issue");
    let body = redeem_body(&token.to_base32(), &device_pubkey_b64(7), "redeem-nonce-ok");
    let (status, _) = http_request(
        f.addr,
        "POST",
        REDEEM_ROUTE,
        body,
        &[("content-type", "application/json".to_string())],
    )
    .await;
    assert_eq!(status, 200, "valid redeem must be 200");

    let rows = f.request_log.tail(10).expect("tail");
    let row = rows
        .iter()
        .find(|r| r.route == REDEEM_ROUTE)
        .expect("a pair-redeem row was persisted");
    assert!(row.accepted, "accepted redeem must log accepted=true");
    assert!(row.reason.is_none(), "accepted row carries no reason");
    assert!(
        row.actor_device_id.is_some(),
        "accepted redeem records the new device id"
    );
    assert_eq!(row.nonce.as_deref(), Some("redeem-nonce-ok"));
}

#[tokio::test]
async fn pair_redeem_reuse_reject_appends_reason() {
    let f = spawn_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    let token = f.pairings.issue(Duration::from_secs(600)).expect("issue");
    let token_b32 = token.to_base32();

    // First redeem consumes the token (accept row).
    let (s1, _) = http_request(
        f.addr,
        "POST",
        REDEEM_ROUTE,
        redeem_body(&token_b32, &device_pubkey_b64(8), "redeem-nonce-1"),
        &[("content-type", "application/json".to_string())],
    )
    .await;
    assert_eq!(s1, 200);

    // Second redeem of the same token is rejected as already-used.
    let (s2, _) = http_request(
        f.addr,
        "POST",
        REDEEM_ROUTE,
        redeem_body(&token_b32, &device_pubkey_b64(9), "redeem-nonce-2"),
        &[("content-type", "application/json".to_string())],
    )
    .await;
    assert_eq!(s2, 409, "reused token must be 409");

    let rows = f.request_log.tail(10).expect("tail");
    let reject = rows
        .iter()
        .find(|r| r.route == REDEEM_ROUTE && !r.accepted)
        .expect("a rejected pair-redeem row was persisted");
    assert_eq!(reject.reason.as_deref(), Some("token_already_used"));
    assert_eq!(reject.nonce.as_deref(), Some("redeem-nonce-2"));
}

#[tokio::test]
async fn signed_session_accept_appends_row() {
    let f = spawn_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    // pair_device redeems off the HTTP path, so the only logged row is the
    // signed session request below.
    let (device, cap_b64, device_id) = pair_device(
        &f.pairings,
        &f.host,
        11,
        &[Capability::PtyRead, Capability::PtyWrite],
    );
    let body = b"{\"name\":\"main\"}".to_vec();
    let headers = sign_headers(
        "POST",
        SESSIONS_ROUTE,
        &body,
        &cap_b64,
        &device_id,
        &device.to_bytes(),
        "nonce-sess-ok",
    );
    let (status, resp) = http_request(f.addr, "POST", SESSIONS_ROUTE, body, &headers).await;
    assert_eq!(status, 200, "signed create must be 200; body={resp:?}");

    let rows = f.request_log.tail(10).expect("tail");
    let row = rows
        .iter()
        .find(|r| r.route == SESSIONS_ROUTE)
        .expect("a session row was persisted");
    assert!(row.accepted);
    assert_eq!(row.actor_device_id.as_deref(), Some(device_id.as_str()));
    assert_eq!(row.nonce.as_deref(), Some("nonce-sess-ok"));
    assert!(row.reason.is_none());

    if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&resp) {
        if let Some(id) = v["id"].as_str() {
            if let Ok(uuid) = uuid::Uuid::parse_str(id) {
                let _ = f.registry.kill(uuid).await;
            }
        }
    }
}

#[tokio::test]
async fn unsigned_session_reject_appends_reason() {
    let f = spawn_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    let (status, _) = http_request(
        f.addr,
        "POST",
        SESSIONS_ROUTE,
        b"{}".to_vec(),
        &[("content-type", "application/json".to_string())],
    )
    .await;
    assert_eq!(status, 401, "unsigned create must be 401");

    let rows = f.request_log.tail(10).expect("tail");
    let row = rows
        .iter()
        .find(|r| r.route == SESSIONS_ROUTE)
        .expect("a session row was persisted");
    assert!(!row.accepted);
    assert_eq!(row.reason.as_deref(), Some("missing_authorization"));
    assert!(
        row.actor_device_id.is_none(),
        "no Authorization header -> no device id"
    );
}
