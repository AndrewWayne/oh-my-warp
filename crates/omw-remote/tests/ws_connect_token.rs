//! Wiring 2 — server-side support for the URL connect-token (`?ct=`) WS auth
//! scheme. Mirrors the bundle shape produced by
//! `apps/web-controller/src/lib/pty-ws.ts::buildConnectToken` and pins the
//! same §4.2 ladder the HTTP-header path runs.

#[path = "ws_common/mod.rs"]
mod ws_common;

use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use chrono::Utc;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

use ws_common::{make_connect_token, spawn_server};

/// Build a WS upgrade request with `?ct=<value>` and the pinned Origin.
fn build_ct_request(
    addr: std::net::SocketAddr,
    session_id: &str,
    ct: &str,
    origin: &str,
) -> http::Request<()> {
    let url = format!("ws://{addr}/ws/v1/pty/{session_id}?ct={ct}");
    let mut req = url.into_client_request().expect("valid ws URL");
    req.headers_mut().insert("Origin", origin.parse().unwrap());
    req
}

#[tokio::test]
async fn valid_connect_token_succeeds() {
    let f = spawn_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    let ct = make_connect_token(
        &f.device,
        &f.cap_token_b64,
        &f.device_id,
        &f.session_id,
        Utc::now(),
        "ct-nonce-ok",
    );
    let http_req = build_ct_request(f.addr, &f.session_id, &ct, &f.pinned_origin);

    let connect = timeout(
        Duration::from_secs(5),
        tokio_tungstenite::connect_async(http_req),
    )
    .await
    .expect("WS connect must not hang");

    let (_ws, resp) = connect.expect("WS upgrade with valid ?ct= must succeed");
    assert_eq!(
        resp.status().as_u16(),
        101,
        "expected 101 Switching Protocols on valid connect-token; got {}",
        resp.status()
    );
}

#[tokio::test]
async fn bogus_base64_in_ct_rejects_400() {
    let f = spawn_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    // `not-base64!` has `!` which isn't in base64url's alphabet -> decode fails.
    let http_req = build_ct_request(f.addr, &f.session_id, "not-base64!", &f.pinned_origin);

    let result = timeout(
        Duration::from_secs(5),
        tokio_tungstenite::connect_async(http_req),
    )
    .await
    .expect("connect must not hang");

    let err = result.expect_err("malformed ct must reject the upgrade");
    let s = format!("{err}");
    assert!(
        s.contains("400") || s.contains("Bad Request") || s.contains("connect_token_invalid"),
        "expected 400/connect_token_invalid; got: {s}"
    );
}

#[tokio::test]
async fn invalid_signature_in_ct_rejects_401() {
    let f = spawn_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Build a normal connect-token, then tamper its `sig` field by flipping
    // the last byte before re-encoding.
    let ct = make_connect_token(
        &f.device,
        &f.cap_token_b64,
        &f.device_id,
        &f.session_id,
        Utc::now(),
        "ct-nonce-bad-sig",
    );
    let json_bytes = URL_SAFE_NO_PAD.decode(&ct).expect("decode test ct");
    let mut bundle: serde_json::Value = serde_json::from_slice(&json_bytes).expect("parse test ct");
    let sig_b64 = bundle["sig"].as_str().expect("sig string").to_string();
    let mut sig_bytes = URL_SAFE_NO_PAD.decode(&sig_b64).expect("decode sig");
    sig_bytes[63] ^= 0x01;
    bundle["sig"] = serde_json::Value::String(URL_SAFE_NO_PAD.encode(&sig_bytes));
    let tampered_ct =
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&bundle).expect("re-serialize tampered"));

    let http_req = build_ct_request(f.addr, &f.session_id, &tampered_ct, &f.pinned_origin);

    let result = timeout(
        Duration::from_secs(5),
        tokio_tungstenite::connect_async(http_req),
    )
    .await
    .expect("connect must not hang");

    let err = result.expect_err("tampered sig must reject");
    let s = format!("{err}");
    assert!(
        s.contains("401") || s.contains("signature_invalid") || s.contains("Unauthorized"),
        "expected 401/signature_invalid; got: {s}"
    );
}

#[tokio::test]
async fn expired_ts_in_ct_rejects_401() {
    let f = spawn_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    // ts is 60 s in the past (skew window is 30 s) -> ts_skew failure.
    let stale_ts = Utc::now() - chrono::Duration::seconds(60);
    let ct = make_connect_token(
        &f.device,
        &f.cap_token_b64,
        &f.device_id,
        &f.session_id,
        stale_ts,
        "ct-nonce-stale-ts",
    );
    let http_req = build_ct_request(f.addr, &f.session_id, &ct, &f.pinned_origin);

    let result = timeout(
        Duration::from_secs(5),
        tokio_tungstenite::connect_async(http_req),
    )
    .await
    .expect("connect must not hang");

    let err = result.expect_err("expired ts must reject");
    let s = format!("{err}");
    assert!(
        s.contains("401") || s.contains("ts_skew") || s.contains("Unauthorized"),
        "expected 401/ts_skew; got: {s}"
    );
}

#[tokio::test]
async fn replayed_nonce_in_ct_rejects_403() {
    let f = spawn_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    let ct = make_connect_token(
        &f.device,
        &f.cap_token_b64,
        &f.device_id,
        &f.session_id,
        Utc::now(),
        "ct-nonce-replay",
    );

    // First upgrade: must succeed and consume the nonce.
    let req1 = build_ct_request(f.addr, &f.session_id, &ct, &f.pinned_origin);
    let connect1 = timeout(
        Duration::from_secs(5),
        tokio_tungstenite::connect_async(req1),
    )
    .await
    .expect("first connect must not hang");
    let (_ws1, resp1) = connect1.expect("first connect-token upgrade must succeed");
    assert_eq!(resp1.status().as_u16(), 101, "first upgrade should be 101");

    // Second upgrade with the same `ct` (same nonce): must reject with 403.
    let req2 = build_ct_request(f.addr, &f.session_id, &ct, &f.pinned_origin);
    let result = timeout(
        Duration::from_secs(5),
        tokio_tungstenite::connect_async(req2),
    )
    .await
    .expect("second connect must not hang");

    let err = result.expect_err("replayed nonce must reject");
    let s = format!("{err}");
    assert!(
        s.contains("403") || s.contains("nonce_replayed") || s.contains("Forbidden"),
        "expected 403/nonce_replayed; got: {s}"
    );
}
