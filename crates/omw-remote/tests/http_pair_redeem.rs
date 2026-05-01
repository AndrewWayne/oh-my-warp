//! Tests for `POST /api/v1/pair/redeem`. Pins error-code mapping per
//! BYORC §3.5 + happy-path response shape per §3.2.

#[path = "http_common/mod.rs"]
mod http_common;

use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use chrono::{TimeZone, Utc};
use ed25519_dalek::SigningKey;
use omw_remote::{open_db, CapabilityToken, Pairings};
use serde_json::{json, Value};
use tempfile::TempDir;

use http_common::{http_request, spawn_server};

fn body_for(token_b32: &str, pk_b64: &str, name: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "v": 1,
        "pairing_token": token_b32,
        "device_pubkey": pk_b64,
        "device_name": name,
        "platform": "test",
        "client_nonce": "AAAAAAAAAAAAAAAAAAAAAA",
    }))
    .unwrap()
}

#[tokio::test]
async fn pair_redeem_happy_path() {
    let f = spawn_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    let token = f.pairings.issue(Duration::from_secs(600)).expect("issue");
    let device = SigningKey::from_bytes(&[7u8; 32]);
    let device_pubkey = device.verifying_key().to_bytes();
    let pk_b64 = URL_SAFE_NO_PAD.encode(device_pubkey);

    let body = body_for(&token.to_base32(), &pk_b64, "test-device");
    let (status, body) = http_request(
        f.addr,
        "POST",
        "/api/v1/pair/redeem",
        body,
        &[("content-type", "application/json".to_string())],
    )
    .await;

    assert_eq!(status, 200, "expected 200; got {status}; body={body:?}");
    let v: Value = serde_json::from_slice(&body).expect("valid JSON");
    assert_eq!(v["v"], 1);
    assert_eq!(v["host_id"], f.host_id);
    assert_eq!(
        v["host_pubkey"],
        URL_SAFE_NO_PAD.encode(f.host_pubkey),
        "host_pubkey must echo the configured host key"
    );
    let device_id = v["device_id"].as_str().expect("device_id string");
    assert_eq!(device_id.len(), 16, "device_id is 16 hex chars");

    let cap_b64 = v["capability_token"]
        .as_str()
        .expect("capability_token string");
    let cap = CapabilityToken::from_base64url(cap_b64).expect("cap parses");
    cap.verify(&f.host_pubkey, Utc::now())
        .expect("cap verifies under host pubkey");
    assert_eq!(cap.device_pubkey, device_pubkey);

    let caps = v["capabilities"].as_array().expect("capabilities array");
    let cap_names: Vec<&str> = caps.iter().map(|c| c.as_str().unwrap()).collect();
    assert!(cap_names.contains(&"pty:read"));
    assert!(cap_names.contains(&"pty:write"));
}

#[tokio::test]
async fn pair_redeem_unknown_token_404() {
    let f = spawn_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    // A 32-byte all-zero token, valid base32 but never issued.
    let bogus = base32::encode(base32::Alphabet::Crockford, &[0u8; 32]);
    let device = SigningKey::from_bytes(&[8u8; 32]);
    let pk_b64 = URL_SAFE_NO_PAD.encode(device.verifying_key().to_bytes());
    let body = body_for(&bogus, &pk_b64, "x");

    let (status, _body) = http_request(
        f.addr,
        "POST",
        "/api/v1/pair/redeem",
        body,
        &[("content-type", "application/json".to_string())],
    )
    .await;
    assert_eq!(status, 404, "unknown token must be 404");
}

#[tokio::test]
async fn pair_redeem_expired_410() {
    // Build a fixture whose Pairings has a clock injected past the TTL
    // *after* issuing. Spin up a separate server with our forced-expired
    // pairings.
    use omw_remote::{make_router, HostKey, NonceStore, RevocationList, ServerConfig, ShellSpec};
    use omw_server::SessionRegistry;
    use std::sync::Arc;
    use tokio::net::TcpListener;

    let host = Arc::new(HostKey::generate());
    let tempdir = TempDir::new().expect("tempdir");
    let db_path = tempdir.path().join("expired.sqlite");
    let conn = open_db(&db_path).expect("open db");
    let mut pairings = Pairings::new(conn);
    let token = pairings.issue(Duration::from_secs(60)).expect("issue");
    fn future_now() -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2099, 1, 1, 0, 0, 0).unwrap()
    }
    pairings.set_clock(future_now);
    let pairings = Arc::new(pairings);

    let registry = SessionRegistry::new();
    let nonce_store = NonceStore::new(Duration::from_secs(60));
    let revocations = RevocationList::new();
    let cfg = ServerConfig {
        bind: "127.0.0.1:0".parse().unwrap(),
        host_key: host.clone(),
        pinned_origins: vec!["https://omw.test".to_string()],
        inactivity_timeout: Duration::from_secs(60),
        revocations,
        nonce_store,
        pairings: Some(pairings),
        shell: ShellSpec {
            program: "/bin/sh".into(),
            args: vec![],
        },
        pty_registry: registry,
        host_id: "omw-host".to_string(),
    };
    let router = make_router(cfg);
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, router.into_make_service()).await;
    });
    tokio::time::sleep(Duration::from_millis(50)).await;

    let device = SigningKey::from_bytes(&[9u8; 32]);
    let pk_b64 = URL_SAFE_NO_PAD.encode(device.verifying_key().to_bytes());
    let body = body_for(&token.to_base32(), &pk_b64, "x");
    let (status, _body) = http_request(
        addr,
        "POST",
        "/api/v1/pair/redeem",
        body,
        &[("content-type", "application/json".to_string())],
    )
    .await;
    assert_eq!(status, 410, "expired token must be 410");
    drop(tempdir);
}

#[tokio::test]
async fn pair_redeem_used_409() {
    let f = spawn_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    let token = f.pairings.issue(Duration::from_secs(600)).expect("issue");
    let device = SigningKey::from_bytes(&[10u8; 32]);
    let pk_b64 = URL_SAFE_NO_PAD.encode(device.verifying_key().to_bytes());
    let body1 = body_for(&token.to_base32(), &pk_b64, "x");
    let (s1, _) = http_request(
        f.addr,
        "POST",
        "/api/v1/pair/redeem",
        body1,
        &[("content-type", "application/json".to_string())],
    )
    .await;
    assert_eq!(s1, 200, "first redeem must succeed");

    let body2 = body_for(&token.to_base32(), &pk_b64, "x");
    let (s2, _) = http_request(
        f.addr,
        "POST",
        "/api/v1/pair/redeem",
        body2,
        &[("content-type", "application/json".to_string())],
    )
    .await;
    assert_eq!(s2, 409, "second redeem must be 409");
}

#[tokio::test]
async fn pair_redeem_bad_pubkey_400() {
    let f = spawn_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    let token = f.pairings.issue(Duration::from_secs(600)).expect("issue");
    // 16-byte pubkey — wrong size.
    let pk_b64 = URL_SAFE_NO_PAD.encode([0u8; 16]);
    let body = body_for(&token.to_base32(), &pk_b64, "x");
    let (status, _) = http_request(
        f.addr,
        "POST",
        "/api/v1/pair/redeem",
        body,
        &[("content-type", "application/json".to_string())],
    )
    .await;
    assert_eq!(status, 400, "bad pubkey must be 400");
}

#[tokio::test]
async fn pair_redeem_malformed_400() {
    let f = spawn_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Missing pairing_token field.
    let body = serde_json::to_vec(&json!({
        "v": 1,
        "device_pubkey": "AAAA",
        "device_name": "x",
    }))
    .unwrap();
    let (status, _) = http_request(
        f.addr,
        "POST",
        "/api/v1/pair/redeem",
        body,
        &[("content-type", "application/json".to_string())],
    )
    .await;
    assert_eq!(status, 400, "missing required field must be 400");
}
