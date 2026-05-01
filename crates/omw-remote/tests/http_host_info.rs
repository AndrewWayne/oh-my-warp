//! Tests for the unauthenticated `GET /api/v1/host-info` discovery route.

#[path = "http_common/mod.rs"]
mod http_common;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use serde_json::Value;

use http_common::{http_request, spawn_server};

#[tokio::test]
async fn host_info_returns_pubkey_unauthenticated() {
    let f = spawn_server().await;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let (status, body) = http_request(f.addr, "GET", "/api/v1/host-info", vec![], &[]).await;
    assert_eq!(status, 200, "expected 200; got {status}");

    let v: Value = serde_json::from_slice(&body).expect("valid JSON");
    assert_eq!(v["v"], 1);
    assert_eq!(v["protocol_version"], 1);
    assert_eq!(v["host_id"], f.host_id);

    let host_pubkey_b64 = v["host_pubkey"].as_str().expect("host_pubkey string");
    let decoded = URL_SAFE_NO_PAD
        .decode(host_pubkey_b64)
        .expect("base64url decodes");
    assert_eq!(decoded.len(), 32, "host_pubkey must be 32 bytes");
    assert_eq!(
        decoded, f.host_pubkey,
        "host_pubkey must match the configured host key"
    );

    let caps = v["capabilities_supported"]
        .as_array()
        .expect("capabilities_supported array");
    assert!(caps.iter().any(|c| c == "pty:read"));
    assert!(caps.iter().any(|c| c == "pty:write"));
}
