//! Mac phone sharing must work with the embedded daemon's read-only pairing.

#![cfg(target_os = "macos")]

#[path = "ws_common/mod.rs"]
mod ws_common;

use std::time::Duration;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use bytes::Bytes;
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use omw_remote::{Capability, CapabilityToken, Frame, FrameKind, Signer};
use omw_server::ExternalSessionSpec;
use tokio::sync::{broadcast, mpsc};
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};

async fn check_read_only_attach(use_connect_token: bool) {
    let mut f = ws_common::spawn_server().await;
    f.cap_token = CapabilityToken::issue(
        &f.host,
        f.device.verifying_key().to_bytes(),
        f.device_id.clone(),
        vec![Capability::PtyRead],
        Duration::from_secs(60),
    );
    f.cap_token_b64 = f.cap_token.to_base64url();
    let (input_tx, mut input_rx) = mpsc::channel(8);
    let (output_tx, _output_rx) = broadcast::channel(8);
    let session_id = f
        .registry
        .register_external(ExternalSessionSpec {
            name: "readonly-phone-pane".into(),
            input_tx,
            output_tx: output_tx.clone(),
            kill: Box::new(|| {}),
            resize_handler: None,
            initial_size: omw_pty::PtySize { rows: 24, cols: 80 },
        })
        .await
        .unwrap();
    f.session_id = session_id.to_string();

    let now = Utc::now();
    let nonce = "readonly-upgrade";
    let mut req = if use_connect_token {
        let ct = ws_common::make_connect_token(
            &f.device,
            &f.cap_token_b64,
            &f.device_id,
            &f.session_id,
            now,
            nonce,
        );
        format!("ws://{}/ws/v1/pty/{}?ct={ct}", f.addr, f.session_id)
            .into_client_request()
            .unwrap()
    } else {
        let mut req = format!("ws://{}/ws/v1/pty/{}", f.addr, f.session_id)
            .into_client_request()
            .unwrap();
        let canonical = ws_common::build_handshake_canonical(&f, now, nonce);
        let sig = ws_common::sign_canonical(&f, &canonical);
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
        req
    };
    req.headers_mut()
        .insert("Origin", f.pinned_origin.parse().unwrap());
    let (mut ws, _) = timeout(
        Duration::from_secs(5),
        tokio_tungstenite::connect_async(req),
    )
    .await
    .unwrap()
    .expect("read-only phone must attach");
    output_tx
        .send(Bytes::from_static(b"readonly-live-output"))
        .unwrap();

    timeout(Duration::from_secs(5), async {
        loop {
            if let Some(Ok(Message::Text(text))) = ws.next().await {
                let frame = Frame::from_json(&text).unwrap();
                frame.verify(&f.host_pubkey).unwrap();
                if frame.kind == FrameKind::Output && frame.payload == b"readonly-live-output"[..] {
                    break;
                }
            } else {
                panic!("read-only output disconnected");
            }
        }
    })
    .await
    .unwrap();

    let sign_frame = |seq, kind, payload: &'static [u8]| {
        let mut frame = Frame {
            v: 1,
            seq,
            ts: Utc::now(),
            kind,
            payload: Bytes::from_static(payload),
            sig: [0; 64],
        };
        frame.sign(&Signer {
            device_priv: &f.device.to_bytes(),
        });
        Message::Text(frame.to_json())
    };
    ws.send(sign_frame(
        0,
        FrameKind::Control,
        br#"{"type":"resize","rows":10,"cols":20}"#,
    ))
    .await
    .unwrap();
    ws.send(sign_frame(1, FrameKind::Ping, b"readonly-heartbeat"))
        .await
        .unwrap();
    timeout(Duration::from_secs(5), async {
        loop {
            if let Some(Ok(Message::Text(text))) = ws.next().await {
                if Frame::from_json(&text).unwrap().kind == FrameKind::Pong {
                    break;
                }
            } else {
                panic!("read-only heartbeat disconnected");
            }
        }
    })
    .await
    .unwrap();
    let (_, size, _) = f
        .registry
        .subscribe_with_state_and_size(session_id)
        .unwrap();
    assert_eq!(size, (24, 80), "read-only phone must not resize the pane");

    ws.send(sign_frame(2, FrameKind::Input, b"unauthorized-input\n"))
        .await
        .unwrap();
    let code = timeout(Duration::from_secs(5), async {
        loop {
            match ws.next().await {
                Some(Ok(Message::Close(Some(frame)))) => break u16::from(frame.code),
                Some(Ok(_)) => {}
                other => panic!("expected scope rejection, got {other:?}"),
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(code, 4403);
    assert!(
        input_rx.try_recv().is_err(),
        "read-only input reached the real pane"
    );
    f.registry.kill(session_id).await.unwrap();
}

#[tokio::test]
async fn read_only_header_attach_streams_without_write_or_resize() {
    check_read_only_attach(false).await;
}

#[tokio::test]
async fn read_only_browser_attach_streams_without_write_or_resize() {
    check_read_only_attach(true).await;
}

async fn check_scope_handshake(scopes: Vec<Capability>, accepted: bool) {
    let f = ws_common::spawn_server().await;
    let cap = CapabilityToken::issue(
        &f.host,
        f.device.verifying_key().to_bytes(),
        f.device_id.clone(),
        scopes,
        Duration::from_secs(60),
    );
    let ct = ws_common::make_connect_token(
        &f.device,
        &cap.to_base64url(),
        &f.device_id,
        &f.session_id,
        Utc::now(),
        "scope-upgrade",
    );
    let mut req = format!("ws://{}/ws/v1/pty/{}?ct={ct}", f.addr, f.session_id)
        .into_client_request()
        .unwrap();
    req.headers_mut()
        .insert("Origin", f.pinned_origin.parse().unwrap());
    let result = timeout(
        Duration::from_secs(5),
        tokio_tungstenite::connect_async(req),
    )
    .await
    .unwrap();
    if accepted {
        let (mut ws, _) = result.expect("existing write-only capability must still attach");
        ws.close(None).await.unwrap();
    } else {
        let error = result.expect_err("unrelated scope must not attach");
        assert!(format!("{error}").contains("401"));
    }
}

#[tokio::test]
async fn existing_write_only_capability_still_attaches() {
    check_scope_handshake(vec![Capability::PtyWrite], true).await;
}

#[tokio::test]
async fn unrelated_agent_scope_cannot_attach() {
    check_scope_handshake(vec![Capability::AgentRead], false).await;
}
