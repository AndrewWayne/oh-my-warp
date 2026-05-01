//! Tests pinning the registry-backed WS attach behavior.
//!
//! - WS-attach to a session created via HTTP succeeds and sees its echo output.
//! - WS-attach to an unknown session_id is rejected at upgrade time.

#[path = "http_common/mod.rs"]
mod http_common;

use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use bytes::Bytes;
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use omw_remote::{CanonicalRequest, Capability, Frame, FrameKind, Signer};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

use http_common::{http_request, pair_device, sign_headers, spawn_server};

#[tokio::test]
async fn ws_connects_to_registry_session() {
    let f = spawn_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    // 1. Pair + create session via HTTP.
    let (device, cap_b64, device_id) = pair_device(
        &f.pairings,
        &f.host,
        20,
        &[Capability::PtyRead, Capability::PtyWrite],
    );
    let device_priv = device.to_bytes();

    let create_body = b"{\"name\":\"reg\"}".to_vec();
    let create_headers = sign_headers(
        "POST",
        "/api/v1/sessions",
        &create_body,
        &cap_b64,
        &device_id,
        &device_priv,
        "nonce-ws-create",
    );
    let (cs, cb) = http_request(
        f.addr,
        "POST",
        "/api/v1/sessions",
        create_body,
        &create_headers,
    )
    .await;
    assert_eq!(cs, 200, "create status");
    let cv: Value = serde_json::from_slice(&cb).unwrap();
    let session_id = cv["id"].as_str().unwrap().to_string();

    // 2. Sign the WS upgrade request.
    let url = format!("ws://{}/ws/v1/pty/{}", f.addr, session_id);
    let mut req = url.into_client_request().expect("valid ws URL");

    let now = Utc::now();
    let nonce = format!(
        "ws-nonce-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let canonical = CanonicalRequest {
        method: "GET".into(),
        path: format!("/ws/v1/pty/{session_id}"),
        query: String::new(),
        ts: now.to_rfc3339(),
        nonce: nonce.clone(),
        body_sha256: empty_sha256(),
        device_id: device_id.clone(),
        protocol_version: 1,
    };
    let sig = Signer {
        device_priv: &device_priv,
    }
    .sign(&canonical);

    let h = req.headers_mut();
    h.insert(
        "Authorization",
        format!("Bearer {cap_b64}").parse().unwrap(),
    );
    h.insert(
        "X-Omw-Signature",
        URL_SAFE_NO_PAD.encode(sig).parse().unwrap(),
    );
    h.insert("X-Omw-Nonce", nonce.parse().unwrap());
    h.insert("X-Omw-Ts", now.to_rfc3339().parse().unwrap());
    h.insert("Origin", f.pinned_origin.parse().unwrap());

    let (mut ws, _resp) = timeout(
        Duration::from_secs(5),
        tokio_tungstenite::connect_async(req),
    )
    .await
    .expect("connect must not hang")
    .expect("WS upgrade must succeed");

    // 3. Send a signed input frame; expect to see it echoed.
    tokio::time::sleep(Duration::from_millis(200)).await;
    let line: &[u8] = if cfg!(windows) { b"hi\r\n" } else { b"hi\n" };
    let mut frame = Frame {
        v: 1,
        seq: 0,
        ts: Utc::now(),
        kind: FrameKind::Input,
        payload: Bytes::copy_from_slice(line),
        sig: [0u8; 64],
    };
    frame.sign(&Signer {
        device_priv: &device_priv,
    });
    ws.send(Message::Text(frame.to_json()))
        .await
        .expect("send input");

    let saw = timeout(Duration::from_secs(5), async {
        let mut acc = Vec::<u8>::new();
        while let Some(msg) = ws.next().await {
            let msg = match msg {
                Ok(m) => m,
                Err(_) => return false,
            };
            if let Message::Text(t) = msg {
                let frame = match Frame::from_json(&t) {
                    Ok(fr) => fr,
                    Err(_) => continue,
                };
                if frame.kind == FrameKind::Output {
                    acc.extend_from_slice(&frame.payload);
                    if acc.windows(b"hi".len()).any(|w| w == b"hi") {
                        return true;
                    }
                }
            }
        }
        false
    })
    .await
    .expect("did not see 'hi' echoed within 5s");

    assert!(saw, "expected echo of 'hi' from registry-backed PTY");

    // Cleanup.
    let uuid = uuid::Uuid::parse_str(&session_id).unwrap();
    let _ = ws.close(None).await;
    let _ = f.registry.kill(uuid).await;
}

#[tokio::test]
async fn ws_unknown_session_rejects_upgrade() {
    let f = spawn_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Pair so the signed handshake passes the §4 ladder; the failure point
    // is the unknown session id (registry lookup).
    let (device, cap_b64, device_id) = pair_device(
        &f.pairings,
        &f.host,
        21,
        &[Capability::PtyRead, Capability::PtyWrite],
    );
    let device_priv = device.to_bytes();

    let bogus = uuid::Uuid::new_v4().to_string();
    let url = format!("ws://{}/ws/v1/pty/{}", f.addr, bogus);
    let mut req = url.into_client_request().expect("valid ws URL");

    let now = Utc::now();
    let nonce = "nonce-ws-unknown".to_string();
    let canonical = CanonicalRequest {
        method: "GET".into(),
        path: format!("/ws/v1/pty/{bogus}"),
        query: String::new(),
        ts: now.to_rfc3339(),
        nonce: nonce.clone(),
        body_sha256: empty_sha256(),
        device_id,
        protocol_version: 1,
    };
    let sig = Signer {
        device_priv: &device_priv,
    }
    .sign(&canonical);
    let h = req.headers_mut();
    h.insert(
        "Authorization",
        format!("Bearer {cap_b64}").parse().unwrap(),
    );
    h.insert(
        "X-Omw-Signature",
        URL_SAFE_NO_PAD.encode(sig).parse().unwrap(),
    );
    h.insert("X-Omw-Nonce", nonce.parse().unwrap());
    h.insert("X-Omw-Ts", now.to_rfc3339().parse().unwrap());
    h.insert("Origin", f.pinned_origin.parse().unwrap());

    let result = timeout(
        Duration::from_secs(5),
        tokio_tungstenite::connect_async(req),
    )
    .await
    .expect("connect must not hang");
    let err = result.expect_err("unknown session id must reject upgrade");
    let s = format!("{err}");
    assert!(
        s.contains("404") || s.contains("session_not_found"),
        "expected 404/session_not_found; got: {s}"
    );
}

fn empty_sha256() -> [u8; 32] {
    let h = Sha256::digest(b"");
    let mut out = [0u8; 32];
    out.copy_from_slice(&h);
    out
}
