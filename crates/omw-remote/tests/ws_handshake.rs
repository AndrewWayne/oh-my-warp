//! Phase E — WS handshake (`specs/byorc-protocol.md` §7.1 + §8.2).
//!
//! Pins the §4 signed-request ladder + §8.2 origin pinning at upgrade time:
//! - happy path: properly signed Authorization + sig + nonce + ts + Origin -> 101 upgrade.
//! - missing Authorization -> 401.
//! - bad signature -> 401.
//! - expired capability -> 401.
//! - origin mismatch -> 403.

#[path = "ws_common/mod.rs"]
mod ws_common;

use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use chrono::Utc;
use ed25519_dalek::SigningKey;
use omw_remote::{Capability, CapabilityToken, HostKey, Signer};
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

use ws_common::{
    body_hash, build_handshake_canonical, device_id_from_pubkey, sign_canonical, spawn_server,
};

/// Build the handshake request: ws:// URL + signed Authorization/sig/nonce/ts
/// headers + Origin. Returns a `Request` ready for `connect_async`.
fn build_signed_request(
    addr: std::net::SocketAddr,
    session_id: &str,
    cap_token_b64: &str,
    sig_b64: &str,
    nonce: &str,
    ts: &str,
    origin: &str,
) -> http::Request<()> {
    let url = format!("ws://{addr}/ws/v1/pty/{session_id}");
    let mut req = url.into_client_request().expect("valid ws URL");
    let h = req.headers_mut();
    h.insert(
        "Authorization",
        format!("Bearer {cap_token_b64}").parse().unwrap(),
    );
    h.insert("X-Omw-Signature", sig_b64.parse().unwrap());
    h.insert("X-Omw-Nonce", nonce.parse().unwrap());
    h.insert("X-Omw-Ts", ts.parse().unwrap());
    h.insert("Origin", origin.parse().unwrap());
    req
}

#[tokio::test]
async fn handshake_with_valid_signed_request_succeeds() {
    let f = spawn_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    let now = Utc::now();
    let req = build_handshake_canonical(&f, now, "nonce-handshake-ok");
    let sig = sign_canonical(&f, &req);

    let http_req = build_signed_request(
        f.addr,
        &f.session_id,
        &f.cap_token_b64,
        &URL_SAFE_NO_PAD.encode(sig),
        "nonce-handshake-ok",
        &now.to_rfc3339(),
        &f.pinned_origin,
    );

    let connect = timeout(
        Duration::from_secs(5),
        tokio_tungstenite::connect_async(http_req),
    )
    .await
    .expect("WS connect must not hang");

    let (_ws, resp) = connect.expect("WS upgrade must succeed");
    assert_eq!(
        resp.status().as_u16(),
        101,
        "expected 101 Switching Protocols on valid signed handshake; got {}",
        resp.status()
    );
}

#[tokio::test]
async fn handshake_without_authorization_rejects() {
    let f = spawn_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Build a request with no Authorization header. tungstenite's
    // IntoClientRequest still gives us valid Sec-WebSocket-* headers.
    let url = format!("ws://{}/ws/v1/pty/{}", f.addr, f.session_id);
    let mut req = url.into_client_request().expect("valid ws URL");
    req.headers_mut()
        .insert("Origin", f.pinned_origin.parse().unwrap());

    let result = timeout(
        Duration::from_secs(5),
        tokio_tungstenite::connect_async(req),
    )
    .await
    .expect("connect must not hang");

    let err = result.expect_err("missing Authorization must reject the upgrade");
    let s = format!("{err}");
    assert!(
        s.contains("401") || s.contains("Unauthorized"),
        "expected 401-shaped reject; got: {s}"
    );
}

#[tokio::test]
async fn handshake_with_bad_signature_rejects() {
    let f = spawn_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Sign with a different key, then present that signature alongside the
    // legitimate capability token. Server's signature check must fail.
    let bogus_signer = SigningKey::from_bytes(&[1u8; 32]);
    let bogus_priv = bogus_signer.to_bytes();
    let now = Utc::now();
    let req = build_handshake_canonical(&f, now, "nonce-bad-sig");
    let bogus_sig = Signer { device_priv: &bogus_priv }.sign(&req);

    let http_req = build_signed_request(
        f.addr,
        &f.session_id,
        &f.cap_token_b64,
        &URL_SAFE_NO_PAD.encode(bogus_sig),
        "nonce-bad-sig",
        &now.to_rfc3339(),
        &f.pinned_origin,
    );

    let result = timeout(
        Duration::from_secs(5),
        tokio_tungstenite::connect_async(http_req),
    )
    .await
    .expect("connect must not hang");

    let err = result.expect_err("bad signature must reject");
    let s = format!("{err}");
    assert!(
        s.contains("401") || s.contains("Unauthorized"),
        "expected 401 for bad signature; got: {s}"
    );
}

#[tokio::test]
async fn handshake_with_expired_capability_rejects() {
    // Mint an already-expired token with TTL 0 so the verifier rejects on
    // `capability_expired` (code 401).
    let f = spawn_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    let device_pubkey: [u8; 32] = f.device.verifying_key().to_bytes();
    let host = HostKey::generate();
    // Build a token signed by a host that doesn't match the server — this
    // would be capability_invalid; instead use the *real* host but a 1ns TTL
    // by re-issuing through f.host. We can't access f.host's signing key
    // directly here, so cap-expired is exercised by setting a sub-second TTL
    // and sleeping past it.
    let _ = host;

    let cap_token = CapabilityToken::issue(
        &f.host,
        device_pubkey,
        device_id_from_pubkey(&device_pubkey),
        vec![Capability::PtyRead, Capability::PtyWrite],
        Duration::from_millis(1),
    );
    tokio::time::sleep(Duration::from_millis(10)).await;
    let expired_b64 = cap_token.to_base64url();

    let now = Utc::now();
    let req = build_handshake_canonical(&f, now, "nonce-cap-expired");
    let sig = sign_canonical(&f, &req);

    let http_req = build_signed_request(
        f.addr,
        &f.session_id,
        &expired_b64,
        &URL_SAFE_NO_PAD.encode(sig),
        "nonce-cap-expired",
        &now.to_rfc3339(),
        &f.pinned_origin,
    );

    let result = timeout(
        Duration::from_secs(5),
        tokio_tungstenite::connect_async(http_req),
    )
    .await
    .expect("connect must not hang");

    let err = result.expect_err("expired capability must reject");
    let s = format!("{err}");
    assert!(
        s.contains("401") || s.contains("capability_expired"),
        "expected 401/capability_expired; got: {s}"
    );
}

#[tokio::test]
async fn handshake_with_origin_mismatch_rejects() {
    let f = spawn_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    let now = Utc::now();
    let req = build_handshake_canonical(&f, now, "nonce-origin-mismatch");
    let sig = sign_canonical(&f, &req);

    let http_req = build_signed_request(
        f.addr,
        &f.session_id,
        &f.cap_token_b64,
        &URL_SAFE_NO_PAD.encode(sig),
        "nonce-origin-mismatch",
        &now.to_rfc3339(),
        "https://evil.example.com",
    );

    let result = timeout(
        Duration::from_secs(5),
        tokio_tungstenite::connect_async(http_req),
    )
    .await
    .expect("connect must not hang");

    let err = result.expect_err("origin mismatch must reject");
    let s = format!("{err}");
    assert!(
        s.contains("403") || s.contains("origin_mismatch"),
        "expected 403/origin_mismatch; got: {s}"
    );

    // Touch unused-by-this-path bindings to keep the lint quiet on Windows.
    let _ = body_hash(b"");
}
