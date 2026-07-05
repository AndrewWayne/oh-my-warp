//! Wiring for the `request_log` audit trail on the WS upgrade surface
//! (spec §4.2 step 10 + §6.3): an accepted handshake logs `accepted=true`,
//! and a pre-auth origin-mismatch reject logs `accepted=false`.

#[path = "ws_common/mod.rs"]
mod ws_common;

use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use chrono::Utc;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

use ws_common::{build_handshake_canonical, sign_canonical, spawn_server, WsFixture};

/// Build a signed WS upgrade request with a caller-chosen `Origin` and a
/// unique nonce, mirroring the native-client header path.
fn signed_request(f: &WsFixture, origin: &str) -> http::Request<()> {
    let url = format!("ws://{}/ws/v1/pty/{}", f.addr, f.session_id);
    let mut req = url.into_client_request().expect("valid ws URL");

    let now = Utc::now();
    let nonce = format!(
        "rl-nonce-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let canonical = build_handshake_canonical(f, now, &nonce);
    let sig = sign_canonical(f, &canonical);

    let h = req.headers_mut();
    h.insert(
        "Authorization",
        format!("Bearer {}", f.cap_token_b64).parse().unwrap(),
    );
    h.insert(
        "X-Omw-Signature",
        URL_SAFE_NO_PAD.encode(sig).parse().unwrap(),
    );
    h.insert("X-Omw-Nonce", nonce.parse().unwrap());
    h.insert("X-Omw-Ts", now.to_rfc3339().parse().unwrap());
    h.insert("Origin", origin.parse().unwrap());
    req
}

#[tokio::test]
async fn ws_accept_appends_row() {
    let f = spawn_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    let req = signed_request(&f, &f.pinned_origin);
    let (ws, _resp) = timeout(
        Duration::from_secs(5),
        tokio_tungstenite::connect_async(req),
    )
    .await
    .expect("connect must not hang")
    .expect("valid signed handshake must upgrade");

    let route = format!("/ws/v1/pty/{}", f.session_id);
    let rows = f.request_log.tail(10).expect("tail");
    let row = rows
        .iter()
        .find(|r| r.route == route)
        .expect("a ws-upgrade row was persisted");
    assert!(row.accepted, "accepted handshake must log accepted=true");
    assert_eq!(row.actor_device_id.as_deref(), Some(f.device_id.as_str()));
    assert!(row.reason.is_none());

    drop(ws);
}

#[tokio::test]
async fn ws_origin_mismatch_reject_appends_row() {
    let f = spawn_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Signed correctly, but the Origin is not pinned -> rejected at step 1.
    let req = signed_request(&f, "https://evil.example");
    let result = timeout(
        Duration::from_secs(5),
        tokio_tungstenite::connect_async(req),
    )
    .await
    .expect("connect must not hang");
    assert!(result.is_err(), "origin mismatch must reject the upgrade");

    let route = format!("/ws/v1/pty/{}", f.session_id);
    let rows = f.request_log.tail(10).expect("tail");
    let row = rows
        .iter()
        .find(|r| r.route == route)
        .expect("a ws-upgrade row was persisted");
    assert!(!row.accepted);
    assert_eq!(row.reason.as_deref(), Some("origin_mismatch"));
}
