//! v0.4-thin Stage C: per-session scrollback replays on WS subscribe.
//!
//! User scenario this guards: phone runs a long command, user puts the phone
//! down for a few minutes, then reconnects. Without scrollback, the new WS
//! subscription only sees output from the moment of reconnection forward —
//! the user lands on a blank xterm.js even though the laptop pane has been
//! producing visible output the whole time.
//!
//! With Stage C, `omw_server::SessionRegistry::record_output` writes each
//! chunk to a per-session ring buffer in addition to broadcasting. The WS
//! handler calls `subscribe_with_scrollback`, which atomically snapshots the
//! ring AND subscribes to the live broadcast — so the subscriber sees the
//! recent past first, then live updates, with no chunk seen twice and no
//! chunk lost.
//!
//! Two assertions:
//!   1. Bytes recorded BEFORE the WS upgrade arrive as Output frames in
//!      order, before any live bytes.
//!   2. Bytes recorded AFTER the upgrade also arrive (no regression in the
//!      live path).

#[path = "http_common/mod.rs"]
mod http_common;

use std::sync::Arc;
use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use bytes::Bytes;
use chrono::Utc;
use futures_util::StreamExt;
use omw_remote::{CanonicalRequest, Capability, Frame, FrameKind, Signer};
use omw_server::ExternalSessionSpec;
use sha2::{Digest, Sha256};
use tokio::sync::{broadcast, mpsc};
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

use http_common::{pair_device, spawn_server};

#[tokio::test]
async fn external_session_scrollback_replays_on_subscribe() {
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

    let killed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let killed_for_closure = killed.clone();
    let kill: Box<dyn Fn() + Send + Sync> = Box::new(move || {
        killed_for_closure.store(true, std::sync::atomic::Ordering::SeqCst);
    });

    let spec = ExternalSessionSpec {
        name: "scrollback-pane".to_string(),
        input_tx,
        output_tx,
        kill,
        initial_size: omw_pty::PtySize { cols: 80, rows: 24 },
    };

    let session_id = f
        .registry
        .register_external(spec)
        .await
        .expect("register_external");

    // Record some past output BEFORE the WS upgrade. The user scenario is
    // "command ran while the phone was disconnected" — at upgrade time these
    // bytes already live in the registry's scrollback ring.
    f.registry
        .record_output(session_id, Bytes::from_static(b"past-line-1\n"))
        .expect("record_output past 1");
    f.registry
        .record_output(session_id, Bytes::from_static(b"past-line-2\n"))
        .expect("record_output past 2");

    // Sign + open the WS upgrade.
    let url = format!("ws://{}/ws/v1/pty/{}", f.addr, session_id);
    let mut req = url.into_client_request().expect("valid ws URL");
    let now = Utc::now();
    let nonce = format!(
        "ws-scrollback-nonce-{}",
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

    // Wait briefly so the upgrade closure has run far enough to enqueue the
    // scrollback frames into the writer task. The handler enqueues them
    // before spawning the live pump, so the ordering is fixed; this sleep
    // just ensures the writer has had a moment to flush before we record
    // the live chunk below (so we can prove past-then-live ordering).
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Now record one more chunk via record_output (the live path).
    f.registry
        .record_output(session_id, Bytes::from_static(b"live-line\n"))
        .expect("record_output live");

    // Read frames until we've seen all three Output payloads, in order.
    let expected: &[&[u8]] = &[b"past-line-1\n", b"past-line-2\n", b"live-line\n"];
    let mut idx = 0;
    let saw_all = timeout(Duration::from_secs(5), async {
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
                if frame.kind != FrameKind::Output {
                    continue;
                }
                assert_eq!(
                    &frame.payload[..],
                    expected[idx],
                    "scrollback/live frame {idx} mismatch"
                );
                idx += 1;
                if idx == expected.len() {
                    return true;
                }
            }
        }
        false
    })
    .await
    .expect("scrollback replay timed out");

    assert!(saw_all, "expected all 3 Output frames in order");

    let _ = ws.close(None).await;
    f.registry.kill(session_id).await.expect("kill ok");
    assert!(
        killed.load(std::sync::atomic::Ordering::SeqCst),
        "kill closure must fire"
    );
}

fn empty_sha256() -> [u8; 32] {
    let h = Sha256::digest(b"");
    let mut out = [0u8; 32];
    out.copy_from_slice(&h);
    out
}
