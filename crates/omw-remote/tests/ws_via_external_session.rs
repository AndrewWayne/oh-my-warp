//! End-to-end test for the *external-session* path through the daemon.
//!
//! Background. v0.4-thin Gap 1 part C wants the phone to drive the laptop's
//! actual Warp pane (not a sibling shell spawned by `omw-pty`). The
//! mechanism: warp-oss calls `omw::pane_share::share_pane`, which packages
//! an `mpsc::Sender<Vec<u8>>` (input bytes go to the Warp event loop) and
//! a `broadcast::Sender<Bytes>` (Warp PTY output gets cloned to phone
//! subscribers) into an `omw_server::ExternalSessionSpec` and registers
//! it via `SessionRegistry::register_external`.
//!
//! Three previous attempts to wire this from warp-oss crashed warp-oss on
//! Phone click. To know whether the daemon's external-session path itself
//! is sound — i.e., whether the next attempt's bug is on the warp side or
//! the omw-remote side — this test pretends to *be* the Warp pane:
//!
//!  1. Spawn the daemon with the standard fixture.
//!  2. Pair a fake "phone" device.
//!  3. Build an `ExternalSessionSpec` with our own input mpsc + output
//!     broadcast (no real PTY involved). Register it directly via
//!     `f.registry.register_external`.
//!  4. Open a signed WebSocket to that session id (same handshake as
//!     `ws_via_registry.rs`).
//!  5. Send an `Input` frame from the WS side; assert the bytes appear on
//!     our input-mpsc receiver.
//!  6. Push bytes onto our broadcast sender (simulating the Warp pane
//!     emitting PTY output); assert the WS receives an `Output` frame
//!     carrying those bytes.
//!
//! If this test passes, the daemon's external-session bridge — registry
//! mechanics, signed-WS handshake, frame pumping — is end-to-end correct.
//! Any remaining bug in the warp-oss attempt is on the warp side
//! (pane_stack downcast, ctx.spawn timing, share_pane pump wiring), and
//! the diagnostic narrows decisively.

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
async fn external_session_round_trips_input_and_output_via_signed_ws() {
    let f = spawn_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    // 1. Pair a fake phone with pty:read + pty:write so the WS upgrade
    //    passes scope check and frame writes succeed.
    let (device, cap_b64, device_id) = pair_device(
        &f.pairings,
        &f.host,
        90,
        &[Capability::PtyRead, Capability::PtyWrite],
    );
    let device_priv = device.to_bytes();

    // 2. Build the external-session channel pair the same shape `share_pane`
    //    builds for a Warp pane. The mpsc is "registry -> Warp event loop";
    //    the broadcast is "Warp PTY output -> registry subscribers".
    let (input_tx, mut input_rx) = mpsc::channel::<Vec<u8>>(64);
    let (output_tx, _output_rx0) = broadcast::channel::<Bytes>(64);

    // Track that the kill closure fires when the test cleans up. The closure
    // body is a flag flip — share_pane's real closure aborts pumps; here we
    // just observe it ran.
    let killed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let killed_for_closure = killed.clone();
    let kill: Box<dyn Fn() + Send + Sync> = Box::new(move || {
        killed_for_closure.store(true, std::sync::atomic::Ordering::SeqCst);
    });

    let spec = ExternalSessionSpec {
        name: "fake-warp-pane".to_string(),
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

    // 3. Sanity: HTTP GET /api/v1/sessions sees the external session — this
    //    is what the Web Controller's Sessions page would call.
    {
        use http_common::{http_request, sign_headers};
        let headers = sign_headers(
            "GET",
            "/api/v1/sessions",
            b"",
            &cap_b64,
            &device_id,
            &device_priv,
            "ext-list-nonce",
        );
        let (s, body) = http_request(f.addr, "GET", "/api/v1/sessions", vec![], &headers).await;
        assert_eq!(s, 200, "list sessions status");
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let sessions = v["sessions"].as_array().expect("sessions array");
        let names: Vec<&str> = sessions
            .iter()
            .filter_map(|s| s["name"].as_str())
            .collect();
        assert!(
            names.iter().any(|n| *n == "fake-warp-pane"),
            "expected fake-warp-pane in sessions list, got {names:?}"
        );
    }

    // 4. Sign the WS upgrade.
    let url = format!("ws://{}/ws/v1/pty/{}", f.addr, session_id);
    let mut req = url.into_client_request().expect("valid ws URL");

    let now = Utc::now();
    let nonce = format!(
        "ws-ext-nonce-{}",
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

    // 5. Send an Input frame from the WS (faux phone). Verify the bytes land
    //    on the input mpsc — proves "phone keystroke -> Warp event loop".
    let mut frame = Frame {
        v: 1,
        seq: 0,
        ts: Utc::now(),
        kind: FrameKind::Input,
        payload: Bytes::copy_from_slice(b"echo external\n"),
        sig: [0u8; 64],
    };
    frame.sign(&Signer {
        device_priv: &device_priv,
    });
    ws.send(Message::Text(frame.to_json()))
        .await
        .expect("send input");

    let received = timeout(Duration::from_secs(5), input_rx.recv())
        .await
        .expect("input mpsc recv timed out")
        .expect("input mpsc closed unexpectedly");
    assert_eq!(
        received, b"echo external\n",
        "input bytes from WS should land verbatim on the external mpsc"
    );

    // 6. Push bytes through the broadcast (faux Warp PTY output). Verify the
    //    WS receives an Output frame with those bytes.
    output_tx
        .send(Bytes::from_static(b"hello from warp pane"))
        .expect("broadcast send must reach the registry's subscriber");

    let saw_output = timeout(Duration::from_secs(5), async {
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
                if frame.kind == FrameKind::Output
                    && &frame.payload[..] == b"hello from warp pane"
                {
                    return true;
                }
            }
        }
        false
    })
    .await
    .expect("output frame timeout");

    assert!(
        saw_output,
        "expected the broadcasted bytes to surface as a WS Output frame"
    );

    // 7. Cleanup. registry.kill triggers the kill closure, which flips the
    //    flag — proves the teardown path is reachable from the WS-side.
    let _ = ws.close(None).await;
    f.registry.kill(session_id).await.expect("kill ok");
    assert!(
        killed.load(std::sync::atomic::Ordering::SeqCst),
        "kill closure must fire when registry.kill is called"
    );
}

fn empty_sha256() -> [u8; 32] {
    let h = Sha256::digest(b"");
    let mut out = [0u8; 32];
    out.copy_from_slice(&h);
    out
}
