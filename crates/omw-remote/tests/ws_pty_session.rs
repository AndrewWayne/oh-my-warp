//! Phase E — WS PTY session bridge (`specs/byorc-protocol.md` §7.2 + §7.3).
//!
//! Builds on the §7.1 handshake (covered in ws_handshake.rs) and exercises
//! per-frame behaviour over an already-upgraded WS:
//! - input frames cause shell input -> we observe the echo;
//! - outbound output frames are signed under the host pubkey;
//! - sequence numbers monotonic; replay closes WS with 4401;
//! - frame ts > 30 s skew closes WS with 4401;
//! - device revoked mid-session closes WS.

#[path = "ws_common/mod.rs"]
mod ws_common;

use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use bytes::Bytes;
use chrono::{Duration as ChronoDuration, Utc};
use futures_util::{SinkExt, StreamExt};
use omw_remote::{Frame, FrameKind, Signer};
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

use ws_common::{build_handshake_canonical, sign_canonical, spawn_server, WsFixture};

/// Open an upgraded WS connection by performing a valid signed handshake.
async fn open_ws(
    f: &WsFixture,
) -> tokio_tungstenite::WebSocketStream<
    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
> {
    let url = format!("ws://{}/ws/v1/pty/{}", f.addr, f.session_id);
    let mut req = url.into_client_request().expect("valid ws URL");

    let now = Utc::now();
    let nonce = format!(
        "nonce-{}",
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

    let (ws, _resp) = timeout(
        Duration::from_secs(5),
        tokio_tungstenite::connect_async(req),
    )
    .await
    .expect("WS connect must not hang")
    .expect("WS upgrade must succeed");
    ws
}

/// Build a signed input frame from the device.
fn signed_input_frame(f: &WsFixture, seq: u64, payload: &[u8]) -> Frame {
    let mut frame = Frame {
        v: 1,
        seq,
        ts: Utc::now(),
        kind: FrameKind::Input,
        payload: Bytes::copy_from_slice(payload),
        sig: [0u8; 64],
    };
    let priv_seed = f.device.to_bytes();
    frame.sign(&Signer { device_priv: &priv_seed });
    frame
}

#[tokio::test]
async fn input_frame_writes_to_shell() {
    let f = spawn_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    let mut ws = open_ws(&f).await;

    // Give the child a moment to apply `stty -echo` (Unix) before we send.
    tokio::time::sleep(Duration::from_millis(200)).await;

    let line: &[u8] = if cfg!(windows) { b"hello\r\n" } else { b"hello\n" };
    let frame = signed_input_frame(&f, 0, line);
    ws.send(Message::Text(frame.to_json()))
        .await
        .expect("send input frame");

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
                    if acc.windows(b"hello".len()).any(|w| w == b"hello") {
                        return true;
                    }
                }
            }
        }
        false
    })
    .await
    .expect("did not see 'hello' on the WS output stream within 5s");

    assert!(saw, "expected to see 'hello' echoed back through PTY output");
    let _ = ws.close(None).await;
}

#[tokio::test]
async fn output_frames_are_signed_with_host_key() {
    let f = spawn_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    let mut ws = open_ws(&f).await;

    tokio::time::sleep(Duration::from_millis(200)).await;
    let line: &[u8] = if cfg!(windows) { b"sig\r\n" } else { b"sig\n" };
    let frame = signed_input_frame(&f, 0, line);
    ws.send(Message::Text(frame.to_json()))
        .await
        .expect("send input frame");

    let mut verified = 0usize;
    let _ = timeout(Duration::from_secs(5), async {
        while let Some(msg) = ws.next().await {
            let msg = match msg {
                Ok(m) => m,
                Err(_) => break,
            };
            if let Message::Text(t) = msg {
                if let Ok(frame) = Frame::from_json(&t) {
                    if frame.kind == FrameKind::Output {
                        frame
                            .verify(&f.host_pubkey)
                            .expect("output frame must verify under host pubkey");
                        verified += 1;
                        if verified >= 1 {
                            return;
                        }
                    }
                }
            }
        }
    })
    .await;

    assert!(
        verified >= 1,
        "expected at least one host-signed output frame, got {verified}"
    );
    let _ = ws.close(None).await;
}

#[tokio::test]
async fn seq_monotonic_inbound() {
    let f = spawn_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    let mut ws = open_ws(&f).await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    // seq=0, 1, 2 — all accepted.
    for seq in 0u64..=2 {
        let frame = signed_input_frame(&f, seq, b"x\n");
        ws.send(Message::Text(frame.to_json()))
            .await
            .expect("send sequential frame");
    }

    // Re-send seq=1 — must be rejected as a regression. Server closes WS
    // with code 4401.
    let replay = signed_input_frame(&f, 1, b"x\n");
    ws.send(Message::Text(replay.to_json()))
        .await
        .expect("send replayed seq frame");

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
    .expect("server must close WS within 5s after seq regression");

    assert_eq!(
        close,
        Some(4401),
        "seq regression must close WS with 4401 auth_failed; got {close:?}"
    );
}

#[tokio::test]
async fn ts_skew_inbound_rejects() {
    let f = spawn_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    let mut ws = open_ws(&f).await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Build a frame whose ts is 60 s in the past and sign it (so the seq+sig
    // pass; only the ts check fires).
    let mut frame = Frame {
        v: 1,
        seq: 0,
        ts: Utc::now() - ChronoDuration::seconds(60),
        kind: FrameKind::Input,
        payload: Bytes::from_static(b"x\n"),
        sig: [0u8; 64],
    };
    let priv_seed = f.device.to_bytes();
    frame.sign(&Signer { device_priv: &priv_seed });

    ws.send(Message::Text(frame.to_json()))
        .await
        .expect("send stale-ts frame");

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
    .expect("server must close WS within 5s after ts skew");

    assert_eq!(
        close,
        Some(4401),
        "ts skew must close WS with 4401 auth_failed; got {close:?}"
    );
}

#[tokio::test]
async fn revoked_device_during_session_closes_ws() {
    let f = spawn_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    let mut ws = open_ws(&f).await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Revoke mid-session via the in-memory revocation list. The next inbound
    // frame must trigger a close (per-frame check, §7.3 step 4).
    f.revocations.revoke(&f.device_id);

    let frame = signed_input_frame(&f, 0, b"x\n");
    ws.send(Message::Text(frame.to_json()))
        .await
        .expect("send post-revoke frame");

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
    .expect("server must close WS within 5s after revoke");

    assert_eq!(
        close,
        Some(4401),
        "revoke must close WS with 4401 auth_failed; got {close:?}"
    );
}
