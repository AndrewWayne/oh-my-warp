//! Integration test for tmux-style attach (plan §5 step 9).
//!
//! Verifies that when a phone WS connects to an existing session, the
//! daemon ships the current vt100 screen state as the FIRST Output frame
//! BEFORE any live broadcast bytes — without that, an in-progress TUI
//! redraw would never appear on the freshly-attached client.
//!
//! Setup mirrors `ws_via_external_session.rs`:
//!   1. Spawn the daemon, pair a fake phone with `pty:read`+`pty:write`.
//!   2. Build an `ExternalSessionSpec` and register it via the registry.
//!   3. Push bytes through `registry.record_output` — those go into the
//!      session's parser, NOT the live broadcast (the broadcast has no
//!      subscribers yet anyway).
//!   4. Open a signed WS upgrade.
//!   5. Read frames; assert the very first Output frame round-trips back
//!      through a fresh vt100 parser to the screen state we wrote.

#[path = "http_common/mod.rs"]
mod http_common;

use std::sync::Arc;
use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use bytes::Bytes;
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use omw_remote::{CanonicalRequest, Capability, Frame, FrameKind, Signer};
use omw_server::ExternalSessionSpec;
use sha2::{Digest, Sha256};
use tokio::sync::{broadcast, mpsc};
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

use http_common::{pair_device, spawn_server};

#[tokio::test]
async fn ws_attach_ships_serialized_screen_state_as_first_frame() {
    let f = spawn_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    let (device, cap_b64, device_id) = pair_device(
        &f.pairings,
        &f.host,
        90,
        &[Capability::PtyRead, Capability::PtyWrite],
    );
    let device_priv = device.to_bytes();

    // Register an external session; the registry constructs a vt100 parser
    // sized to initial_size on registration.
    let (input_tx, _input_rx) = mpsc::channel::<Vec<u8>>(64);
    let (output_tx, _output_rx0) = broadcast::channel::<Bytes>(64);
    let killed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let killed_for_closure = killed.clone();
    let kill: Box<dyn Fn() + Send + Sync> = Box::new(move || {
        killed_for_closure.store(true, std::sync::atomic::Ordering::SeqCst);
    });
    let spec = ExternalSessionSpec {
        name: "snapshot-pane".to_string(),
        input_tx,
        output_tx: output_tx.clone(),
        kill,
        resize_handler: None,
        initial_size: omw_pty::PtySize { cols: 80, rows: 24 },
    };
    let session_id = f
        .registry
        .register_external(spec)
        .await
        .expect("register_external");

    // Push bytes through record_output BEFORE the WS attaches — these go
    // into the parser AND the (currently empty) broadcast. After attach,
    // the snapshot must reflect them.
    f.registry
        .record_output(session_id, Bytes::from_static(b"abc\r\nxyz"))
        .expect("record_output");

    // Sign the WS upgrade exactly like ws_via_external_session does.
    let url = format!("ws://{}/ws/v1/pty/{}", f.addr, session_id);
    let mut req = url.into_client_request().expect("valid ws URL");

    let now = Utc::now();
    let nonce = format!(
        "ws-attach-nonce-{}",
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

    // Read the first Output frame. It MUST be the serialized snapshot —
    // round-tripping it through a fresh parser must reproduce the grid.
    let snapshot_payload = timeout(Duration::from_secs(5), async {
        loop {
            let msg = ws.next().await.expect("WS stream ended early");
            let msg = msg.expect("WS error");
            if let Message::Text(t) = msg {
                let frame = Frame::from_json(&t).expect("frame parses");
                if frame.kind == FrameKind::Output {
                    return frame.payload;
                }
            }
        }
    })
    .await
    .expect("first Output frame did not arrive within 5s");

    assert!(
        !snapshot_payload.is_empty(),
        "snapshot frame payload must be non-empty when bytes were pushed before attach"
    );

    let mut replay = vt100::Parser::new(24, 80, 0);
    replay.process(&snapshot_payload);
    let screen = replay.screen();

    let row0: String = (0..3)
        .map(|c| screen.cell(0, c).map(|cell| cell.contents()).unwrap_or_default().to_string())
        .collect();
    let row1: String = (0..3)
        .map(|c| screen.cell(1, c).map(|cell| cell.contents()).unwrap_or_default().to_string())
        .collect();
    assert_eq!(row0, "abc", "row 0 of replayed snapshot should be 'abc'");
    assert_eq!(row1, "xyz", "row 1 of replayed snapshot should be 'xyz'");

    // Live update path also still works: pushing bytes via record_output
    // after attach must reach the WS subscriber.
    f.registry
        .record_output(session_id, Bytes::from_static(b"\r\nlive"))
        .expect("record_output post-attach");

    let saw_live = timeout(Duration::from_secs(3), async {
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
                if frame.kind == FrameKind::Output && &frame.payload[..] == b"\r\nlive" {
                    return true;
                }
            }
        }
        false
    })
    .await
    .expect("live frame timeout");
    assert!(saw_live, "expected post-attach record_output to surface as a live Output frame");

    let _ = ws.close(None).await;
    f.registry.kill(session_id).await.expect("kill ok");
    assert!(
        killed.load(std::sync::atomic::Ordering::SeqCst),
        "kill closure must fire when registry.kill is called"
    );
}

#[tokio::test]
async fn ws_attach_resize_control_updates_parser_size() {
    let f = spawn_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    let (device, cap_b64, device_id) = pair_device(
        &f.pairings,
        &f.host,
        90,
        &[Capability::PtyRead, Capability::PtyWrite],
    );
    let device_priv = device.to_bytes();

    let (input_tx, _input_rx) = mpsc::channel::<Vec<u8>>(64);
    let (output_tx, _output_rx0) = broadcast::channel::<Bytes>(64);
    let kill: Box<dyn Fn() + Send + Sync> = Box::new(|| {});
    let spec = ExternalSessionSpec {
        name: "resize-pane".to_string(),
        input_tx,
        output_tx,
        kill,
        resize_handler: None,
        initial_size: omw_pty::PtySize { cols: 80, rows: 24 },
    };
    let session_id = f
        .registry
        .register_external(spec)
        .await
        .expect("register_external");

    let url = format!("ws://{}/ws/v1/pty/{}", f.addr, session_id);
    let mut req = url.into_client_request().expect("valid ws URL");
    let now = Utc::now();
    let nonce = format!(
        "ws-resize-nonce-{}",
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
    .expect("connect")
    .expect("upgrade");

    // Drain any initial snapshot frame.
    let _ = timeout(Duration::from_millis(200), ws.next()).await;

    // Send a resize Control frame: 40 cols × 12 rows.
    let payload = serde_json::json!({"type": "resize", "rows": 12, "cols": 40});
    let mut frame = Frame {
        v: 1,
        seq: 0,
        ts: Utc::now(),
        kind: FrameKind::Control,
        payload: Bytes::from(serde_json::to_vec(&payload).unwrap()),
        sig: [0u8; 64],
    };
    frame.sign(&Signer {
        device_priv: &device_priv,
    });
    ws.send(Message::Text(frame.to_json()))
        .await
        .expect("send control");

    // Give the inbound task a moment to process.
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Push bytes that span the new column width to confirm the parser
    // wraps at 40 — at col 38, "abcd" wraps to row 1.
    f.registry
        .record_output(session_id, Bytes::from_static(b"\x1b[1;39Habcd"))
        .expect("record_output after resize");

    let (snapshot, _rx) = f
        .registry
        .subscribe_with_state(session_id)
        .expect("session");
    let mut replay = vt100::Parser::new(12, 40, 0);
    replay.process(&snapshot);
    let screen = replay.screen();
    assert_eq!(
        screen.cell(0, 38).map(|c| c.contents().to_string()),
        Some("a".to_string())
    );
    assert_eq!(
        screen.cell(0, 39).map(|c| c.contents().to_string()),
        Some("b".to_string())
    );
    assert_eq!(
        screen.cell(1, 0).map(|c| c.contents().to_string()),
        Some("c".to_string())
    );
    assert_eq!(
        screen.cell(1, 1).map(|c| c.contents().to_string()),
        Some("d".to_string())
    );

    let _ = ws.close(None).await;
    f.registry.kill(session_id).await.expect("kill");
}

fn empty_sha256() -> [u8; 32] {
    let h = Sha256::digest(b"");
    let mut out = [0u8; 32];
    out.copy_from_slice(&h);
    out
}
