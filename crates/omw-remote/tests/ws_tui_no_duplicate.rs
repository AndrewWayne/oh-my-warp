//! End-to-end check: Claude-Code-style mode 2026 incremental updates do
//! NOT produce duplicate-rendered content on the phone side.
//!
//! Reproduces the failure mode observed during smoke testing: `pane_share`
//! is started AFTER a TUI app is already running, the laptop's child uses
//! `\x1b[?2026h … \x1b[?2026l` synchronized-output frames to redraw, and
//! the phone xterm sees content accumulate (each redraw piles on top of
//! the prior one) instead of replacing in place.
//!
//! Strategy: simulate the laptop side by feeding bytes through
//! `record_output` exactly as `pane_share`'s pump does. Open a real signed
//! WebSocket, collect every `Output` frame the phone receives, replay them
//! into a fresh `vt100::Parser`, and compare cell-by-cell to the
//! authoritative parser the registry maintains for the session.
//!
//! If the phone's reconstructed grid matches the registry's parser grid,
//! the byte path is correct. If they diverge, the duplicate is in our
//! pipeline and the diff will pinpoint where.

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

/// Simulate Claude-Code-shaped TUI bytes:
/// 1. Initial header (no sync output): two lines of plain text.
/// 2. A series of mode-2026 atomic frames each updating the spinner row.
///    The spinner row is row 23 (the bottom of an 80-col 24-row grid),
///    a small region — matches Claude Code's incremental-update pattern.
/// 3. The final spinner state should be the only one visible if the phone
///    renders correctly. Anything else means accumulation.
fn claude_like_byte_stream() -> Vec<Bytes> {
    let mut chunks = Vec::new();
    // Initial state: clear and write two header rows on rows 0-1.
    chunks.push(Bytes::from_static(
        b"\x1b[2J\x1b[H\x1b[1;1HClaude Code v2.1.126\r\n\x1b[2;1HOpus 4.7\r\n",
    ));
    // 5 spinner-tick frames each redrawing row 23 with the current state.
    for state in &["Levitating 1s", "Levitating 2s", "Levitating 3s", "Levitating 4s", "Baked"] {
        let mut frame = Vec::new();
        frame.extend_from_slice(b"\x1b[?2026h");
        frame.extend_from_slice(b"\x1b[24;1H"); // row 24, col 1 = bottom row in 1-indexed
        frame.extend_from_slice(b"\x1b[2K");     // clear that line
        frame.extend_from_slice(state.as_bytes());
        frame.extend_from_slice(b"\x1b[?2026l");
        chunks.push(Bytes::from(frame));
    }
    chunks
}

#[tokio::test]
async fn phone_ws_renders_match_parser_after_mode_2026_updates() {
    let f = spawn_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    let (device, cap_b64, device_id) = pair_device(
        &f.pairings,
        &f.host,
        90,
        &[Capability::PtyRead, Capability::PtyWrite],
    );
    let device_priv = device.to_bytes();

    // Match the laptop pane's typical size.
    let rows: u16 = 24;
    let cols: u16 = 80;

    let (input_tx, _input_rx) = mpsc::channel::<Vec<u8>>(64);
    let (output_tx, _output_rx0) = broadcast::channel::<Bytes>(64);
    let killed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let killed_for_closure = killed.clone();
    let kill: Box<dyn Fn() + Send + Sync> = Box::new(move || {
        killed_for_closure.store(true, std::sync::atomic::Ordering::SeqCst);
    });
    let spec = ExternalSessionSpec {
        name: "tui-test-pane".to_string(),
        input_tx,
        output_tx: output_tx.clone(),
        kill,
        initial_size: omw_pty::PtySize { rows, cols },
    };
    let session_id = f
        .registry
        .register_external(spec)
        .await
        .expect("register_external");

    // Open a signed WS connection BEFORE pushing bytes — this models the
    // user's intended workflow where the phone is attached, then the laptop
    // emits TUI redraws.
    let url = format!("ws://{}/ws/v1/pty/{}", f.addr, session_id);
    let mut req = url.into_client_request().expect("valid ws URL");

    let now = Utc::now();
    let nonce = format!(
        "ws-tui-nonce-{}",
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
    let sig = Signer { device_priv: &device_priv }.sign(&canonical);
    let h = req.headers_mut();
    h.insert("Authorization", format!("Bearer {cap_b64}").parse().unwrap());
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
    .expect("connect must not hang")
    .expect("WS upgrade must succeed");
    let (mut ws_sink, mut ws_stream) = ws.split();

    // Spawn a collector task: every Output frame's payload is appended to a
    // shared buffer. This is what would land in the phone's xterm.js — we
    // replay it into a fresh vt100 parser at the end to mimic what xterm
    // would render.
    let phone_bytes: Arc<tokio::sync::Mutex<Vec<u8>>> =
        Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let collector = phone_bytes.clone();
    let collector_task = tokio::spawn(async move {
        while let Some(msg) = ws_stream.next().await {
            let msg = match msg {
                Ok(m) => m,
                Err(_) => return,
            };
            if let Message::Text(t) = msg {
                if let Ok(frame) = Frame::from_json(&t) {
                    if frame.kind == FrameKind::Output {
                        collector.lock().await.extend_from_slice(&frame.payload);
                    }
                }
            }
        }
    });

    // Push the simulated TUI byte stream through record_output exactly as
    // pane_share's pump would. record_output drives both the parser and the
    // broadcast under the registry mutex — so the WS subscriber sees the
    // bytes in order.
    for chunk in claude_like_byte_stream() {
        f.registry
            .record_output(session_id, chunk)
            .expect("record_output");
        // Small sleep so the WS receive task picks up frames in order with
        // some breathing room. Not strictly required (broadcast preserves
        // order) but makes the test less brittle on slow machines.
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    // Give the WS pipeline 500ms to drain.
    tokio::time::sleep(Duration::from_millis(500)).await;
    collector_task.abort();
    let _ = collector_task.await;

    // What did the phone receive?
    let phone_received = phone_bytes.lock().await.clone();
    eprintln!("phone received {} bytes", phone_received.len());
    eprintln!("phone bytes (utf8 lossy): {:?}", String::from_utf8_lossy(&phone_received));

    // Replay phone's bytes into a fresh parser — proxy for xterm.js.
    let mut phone_parser = vt100::Parser::new(rows, cols, 0);
    phone_parser.process(&phone_received);

    // Authoritative state: the registry's own parser, fed via record_output.
    let (auth_snapshot, _rx) = f
        .registry
        .subscribe_with_state(session_id)
        .expect("session id resolves");
    let mut auth_parser = vt100::Parser::new(rows, cols, 0);
    auth_parser.process(&auth_snapshot);

    // Compare cells row-by-row. The bottom row should show "Baked" (the
    // last spinner state) on both sides. If the phone shows previous states
    // accumulated (e.g., "Levitating 4sBaked" or rows of distinct states),
    // we fail with a diff.
    let mut diffs = Vec::new();
    for r in 0..rows {
        let phone_row = row_str(phone_parser.screen(), r, cols);
        let auth_row = row_str(auth_parser.screen(), r, cols);
        if phone_row != auth_row {
            diffs.push(format!(
                "row {r}:\n  auth:  {:?}\n  phone: {:?}",
                auth_row.trim_end(),
                phone_row.trim_end(),
            ));
        }
    }

    let bottom = row_str(phone_parser.screen(), rows - 1, cols);
    eprintln!("phone bottom row: {:?}", bottom.trim_end());
    let auth_bottom = row_str(auth_parser.screen(), rows - 1, cols);
    eprintln!("auth bottom row:  {:?}", auth_bottom.trim_end());

    let _ = ws_sink.close().await;
    f.registry.kill(session_id).await.expect("kill ok");
    assert!(
        killed.load(std::sync::atomic::Ordering::SeqCst),
        "kill closure must fire"
    );

    if !diffs.is_empty() {
        panic!(
            "phone xterm reconstructed grid differs from authoritative parser:\n{}",
            diffs.join("\n")
        );
    }
}

fn row_str(screen: &vt100::Screen, row: u16, width: u16) -> String {
    let mut s = String::with_capacity(width as usize);
    for c in 0..width {
        let cell = screen
            .cell(row, c)
            .map(|cell| cell.contents().to_string())
            .unwrap_or_default();
        if cell.is_empty() {
            s.push(' ');
        } else {
            s.push_str(&cell);
        }
    }
    s
}

fn empty_sha256() -> [u8; 32] {
    let h = Sha256::digest(b"");
    let mut out = [0u8; 32];
    out.copy_from_slice(&h);
    out
}
