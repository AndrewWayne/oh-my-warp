//! Phase E — WS heartbeats + inactivity (`specs/byorc-protocol.md` §7.5).
//!
//! - signed ping -> signed pong;
//! - unsigned ping -> close (4401);
//! - 60 s of no inbound frame -> close. Tests use `inactivity_timeout = 2s`
//!   so we don't actually wait a minute.

#[path = "ws_common/mod.rs"]
mod ws_common;

use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use bytes::Bytes;
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use omw_remote::{Frame, FrameKind, Signer};
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

use ws_common::{
    build_handshake_canonical, sign_canonical, spawn_server, spawn_server_with_inactivity,
    WsFixture,
};

async fn open_ws(
    f: &WsFixture,
) -> tokio_tungstenite::WebSocketStream<
    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
> {
    let url = format!("ws://{}/ws/v1/pty/{}", f.addr, f.session_id);
    let mut req = url.into_client_request().expect("valid ws URL");

    let now = Utc::now();
    let nonce = format!(
        "nonce-hb-{}",
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
    h.insert("Origin", f.pinned_origin.parse().unwrap());

    let (ws, _resp) = timeout(Duration::from_secs(5), tokio_tungstenite::connect_async(req))
        .await
        .expect("WS connect must not hang")
        .expect("WS upgrade must succeed");
    ws
}

#[tokio::test]
async fn ping_gets_pong() {
    let f = spawn_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    let mut ws = open_ws(&f).await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    let mut ping = Frame {
        v: 1,
        seq: 0,
        ts: Utc::now(),
        kind: FrameKind::Ping,
        payload: Bytes::from_static(b"hb"),
        sig: [0u8; 64],
    };
    let priv_seed = f.device.to_bytes();
    ping.sign(&Signer { device_priv: &priv_seed });
    ws.send(Message::Text(ping.to_json()))
        .await
        .expect("send signed ping");

    let saw_pong = timeout(Duration::from_secs(5), async {
        while let Some(msg) = ws.next().await {
            let msg = match msg {
                Ok(m) => m,
                Err(_) => return false,
            };
            if let Message::Text(t) = msg {
                if let Ok(frame) = Frame::from_json(&t) {
                    if frame.kind == FrameKind::Pong {
                        // Server-signed under host pubkey.
                        if frame.verify(&f.host_pubkey).is_ok() {
                            return true;
                        }
                    }
                }
            }
        }
        false
    })
    .await
    .expect("server must respond with pong within 5s");

    assert!(saw_pong, "expected a host-signed pong frame");
    let _ = ws.close(None).await;
}

#[tokio::test]
async fn unsigned_ping_rejects() {
    let f = spawn_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    let mut ws = open_ws(&f).await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    // sig=zeros — never a valid Ed25519 signature for nontrivial payload.
    let unsigned = Frame {
        v: 1,
        seq: 0,
        ts: Utc::now(),
        kind: FrameKind::Ping,
        payload: Bytes::from_static(b"hb"),
        sig: [0u8; 64],
    };
    ws.send(Message::Text(unsigned.to_json()))
        .await
        .expect("send unsigned ping");

    let close = timeout(Duration::from_secs(5), async {
        loop {
            match ws.next().await {
                Some(Ok(Message::Close(Some(close)))) => return Some(u16::from(close.code)),
                Some(Ok(_)) => continue,
                Some(Err(_)) | None => return None,
            }
        }
    })
    .await
    .expect("server must close WS within 5s on unsigned ping");

    assert_eq!(
        close,
        Some(4401),
        "unsigned ping must close WS with 4401 auth_failed; got {close:?}"
    );
}

#[tokio::test]
async fn inactivity_timeout_closes() {
    // Server config has inactivity_timeout = 2 s; do nothing after handshake
    // and observe the close. Test-side timeout is 6 s to leave headroom.
    let f = spawn_server_with_inactivity(Duration::from_secs(2)).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    let mut ws = open_ws(&f).await;

    let close = timeout(Duration::from_secs(6), async {
        loop {
            match ws.next().await {
                Some(Ok(Message::Close(_))) => return true,
                Some(Ok(_)) => continue,
                Some(Err(_)) | None => return true, // EOF / I/O close also acceptable
            }
        }
    })
    .await
    .expect("server must close WS within 6s on 2s inactivity");

    assert!(close, "server must tear down idle WS after inactivity_timeout");
}
