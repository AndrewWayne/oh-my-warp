//! Wiring 4 — end-to-end BYORC flow over the real omw-remote stack.
//!
//! Exercises the entire path a paired phone walks at runtime:
//!   1. discover the host pubkey via `GET /api/v1/host-info`,
//!   2. redeem a fresh pair token via `POST /api/v1/pair/redeem`,
//!   3. verify the returned capability token under the discovered host pubkey,
//!   4. spawn a real shell PTY via `POST /api/v1/sessions` (signed),
//!   5. attach via WS using a `?ct=` connect-token bundle (wiring 2 path),
//!   6. send a signed `Input` frame carrying `echo hello`,
//!   7. read back host-signed `Output` frames until "hello" appears,
//!   8. tear the session down via `DELETE /api/v1/sessions/:id` (signed).
//!
//! If this passes, the user can run `omw remote start`, expose it via Tailscale
//! Serve, open the web controller on a phone, pair, and get a live shell.
//!
//! Pinned to BYORC §3.2/§3.5 (pair redeem), §4 (signed requests), §7.1/§7.3
//! (WS handshake + frame ladder).

#[path = "http_common/mod.rs"]
mod http_common;

#[path = "ws_common/mod.rs"]
mod ws_common;

use std::sync::Arc;
use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use bytes::Bytes;
use chrono::Utc;
use ed25519_dalek::SigningKey;
use futures_util::{SinkExt, StreamExt};
use omw_remote::{
    make_router, open_db, CapabilityToken, Frame, FrameKind, HostKey, NonceStore, Pairings,
    RevocationList, ServerConfig, ShellSpec, Signer,
};
use omw_server::SessionRegistry;
use rand::rngs::OsRng;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

use http_common::http_request;
use ws_common::make_connect_token;

/// Build a real interactive shell for the e2e test. Unlike the fixture's
/// `echo_shell()` (which loops back input verbatim), this is a shell that
/// EVALUATES `echo hello` and prints `hello` to its stdout.
fn real_shell() -> ShellSpec {
    if cfg!(windows) {
        ShellSpec {
            program: "powershell".into(),
            args: vec!["-NoProfile".into(), "-NoLogo".into()],
        }
    } else {
        ShellSpec {
            program: "/bin/sh".into(),
            args: vec!["-i".into()],
        }
    }
}

fn device_id_from_pubkey(pk: &[u8; 32]) -> String {
    let digest = Sha256::digest(pk);
    let mut s = String::with_capacity(16);
    for b in &digest[..8] {
        s.push(char::from_digit((b >> 4) as u32, 16).unwrap());
        s.push(char::from_digit((b & 0x0F) as u32, 16).unwrap());
    }
    s
}

fn body_sha256(body: &[u8]) -> [u8; 32] {
    let h = Sha256::digest(body);
    let mut out = [0u8; 32];
    out.copy_from_slice(&h);
    out
}

struct E2eFixture {
    addr: std::net::SocketAddr,
    pinned_origin: String,
    pairings: Arc<Pairings>,
    server: JoinHandle<()>,
    _tempdir: TempDir,
}

/// Bring up an in-process omw-remote on `127.0.0.1:0`, wired with a real
/// shell, real pairings store, and an empty registry.
async fn spawn_e2e_server() -> E2eFixture {
    let host = Arc::new(HostKey::generate());
    let tempdir = TempDir::new().expect("tempdir");
    let conn = open_db(&tempdir.path().join("omw.sqlite")).expect("open db");
    let pairings = Arc::new(Pairings::new(conn));
    let registry = SessionRegistry::new();
    let nonce_store = NonceStore::new(Duration::from_secs(60));
    let revocations = RevocationList::new();
    let pinned_origin = "https://omw.test".to_string();

    let cfg = ServerConfig {
        bind: "127.0.0.1:0".parse().unwrap(),
        host_key: host,
        pinned_origins: vec![pinned_origin.clone()],
        inactivity_timeout: Duration::from_secs(60),
        revocations,
        nonce_store,
        pairings: Some(pairings.clone()),
        shell: real_shell(),
        pty_registry: registry,
        host_id: "omw-host".to_string(),
    };

    let router = make_router(cfg);
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, router.into_make_service()).await;
    });

    E2eFixture {
        addr,
        pinned_origin,
        pairings,
        server,
        _tempdir: tempdir,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn e2e_full_byorc_flow() {
    let f = spawn_e2e_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    // -------- Step 1: GET /api/v1/host-info -> learn host_pubkey. --------
    let (status, body) = http_request(f.addr, "GET", "/api/v1/host-info", vec![], &[]).await;
    assert_eq!(status, 200, "host-info status; body={body:?}");
    let v: Value = serde_json::from_slice(&body).expect("host-info JSON");
    let host_pubkey_b64 = v["host_pubkey"]
        .as_str()
        .expect("host_pubkey present")
        .to_string();
    let host_pubkey_vec = URL_SAFE_NO_PAD
        .decode(&host_pubkey_b64)
        .expect("host_pubkey decodes");
    assert_eq!(host_pubkey_vec.len(), 32, "host_pubkey is 32 bytes");
    let mut host_pubkey = [0u8; 32];
    host_pubkey.copy_from_slice(&host_pubkey_vec);

    // -------- Step 2: issue a pair token (out-of-band, like a QR pairing). --------
    let pair_token = f
        .pairings
        .issue(Duration::from_secs(600))
        .expect("issue pair token");
    let pair_token_b32 = pair_token.to_base32();

    // -------- Step 3: POST /api/v1/pair/redeem with a fresh device key. --------
    let device = SigningKey::generate(&mut OsRng);
    let device_pubkey: [u8; 32] = device.verifying_key().to_bytes();
    let pk_b64 = URL_SAFE_NO_PAD.encode(device_pubkey);

    let redeem_body = serde_json::to_vec(&json!({
        "v": 1,
        "pairing_token": pair_token_b32,
        "device_pubkey": pk_b64,
        "device_name": "e2e-phone",
        "platform": "test",
        "client_nonce": "AAAAAAAAAAAAAAAAAAAAAA",
    }))
    .expect("redeem body");
    let (rs, rb) = http_request(
        f.addr,
        "POST",
        "/api/v1/pair/redeem",
        redeem_body,
        &[("content-type", "application/json".to_string())],
    )
    .await;
    assert_eq!(rs, 200, "pair/redeem status; body={rb:?}");
    let rv: Value = serde_json::from_slice(&rb).expect("redeem JSON");
    let device_id = rv["device_id"]
        .as_str()
        .expect("device_id string")
        .to_string();
    assert_eq!(
        device_id,
        device_id_from_pubkey(&device_pubkey),
        "device_id must be sha256(pk)[..16-hex]"
    );
    let cap_b64 = rv["capability_token"]
        .as_str()
        .expect("capability_token string")
        .to_string();
    let redeemed_host_pubkey_b64 = rv["host_pubkey"]
        .as_str()
        .expect("redeem returned host_pubkey");
    assert_eq!(
        redeemed_host_pubkey_b64, host_pubkey_b64,
        "pair-redeem must echo the same host_pubkey discovery returned"
    );
    let cap_names: Vec<&str> = rv["capabilities"]
        .as_array()
        .expect("capabilities array")
        .iter()
        .map(|c| c.as_str().unwrap())
        .collect();
    assert!(cap_names.contains(&"pty:read"));
    assert!(cap_names.contains(&"pty:write"));

    // -------- Step 4: verify the returned capability under the discovered key. --------
    let cap = CapabilityToken::from_base64url(&cap_b64).expect("cap parses");
    cap.verify(&host_pubkey, Utc::now())
        .expect("capability token must verify under host_pubkey from host-info");
    assert_eq!(
        cap.device_pubkey, device_pubkey,
        "cap.device_pubkey must equal the device key we redeemed with"
    );

    // -------- Step 5: POST /api/v1/sessions (signed) to spawn a shell. --------
    let device_priv = device.to_bytes();
    let create_body = b"{\"name\":\"e2e\"}".to_vec();
    let create_headers = sign_request_headers(
        "POST",
        "/api/v1/sessions",
        &create_body,
        &cap_b64,
        &device_id,
        &device_priv,
        "e2e-create-nonce",
    );
    let (cs, cb) = http_request(
        f.addr,
        "POST",
        "/api/v1/sessions",
        create_body,
        &create_headers,
    )
    .await;
    assert_eq!(cs, 200, "create-session status; body={cb:?}");
    let cv: Value = serde_json::from_slice(&cb).expect("create-session JSON");
    let session_id = cv["id"].as_str().expect("session id string").to_string();
    uuid::Uuid::parse_str(&session_id).expect("session id is a UUID");

    // -------- Step 6: build a `?ct=` connect-token (wiring 2 path). --------
    let now = Utc::now();
    let ct = make_connect_token(
        &device,
        &cap_b64,
        &device_id,
        &session_id,
        now,
        "e2e-ws-nonce",
    );

    // -------- Step 7: open the WS attach. --------
    let url = format!("ws://{}/ws/v1/pty/{}?ct={}", f.addr, session_id, ct);
    let mut req = url.into_client_request().expect("valid ws URL");
    req.headers_mut()
        .insert("Origin", f.pinned_origin.parse().unwrap());

    let (mut ws, resp) = timeout(
        Duration::from_secs(5),
        tokio_tungstenite::connect_async(req),
    )
    .await
    .expect("WS connect must not hang")
    .expect("WS upgrade must succeed");
    assert_eq!(
        resp.status().as_u16(),
        101,
        "expected 101 Switching Protocols"
    );

    // Give the shell a beat to print its prompt + initialize before we type.
    tokio::time::sleep(Duration::from_millis(500)).await;

    // -------- Step 8: send a signed Input frame carrying `echo hello`. --------
    let line: &[u8] = if cfg!(windows) {
        b"echo hello\r\n"
    } else {
        b"echo hello\n"
    };
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
        .expect("send input frame");

    // -------- Step 9: read host-signed Output frames; assert "hello" appears. --------
    let saw_hello = timeout(Duration::from_secs(10), async {
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
                    // Every output frame is signed under the host pubkey.
                    frame
                        .verify(&host_pubkey)
                        .expect("output frame must verify under host pubkey");
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
    .expect("did not receive 'hello' on the WS within 10s");
    assert!(
        saw_hello,
        "expected 'hello' to appear in PTY output (echo + shell evaluation)"
    );

    // -------- Step 10: close the WS. --------
    let _ = ws.close(None).await;

    // -------- Step 11: DELETE /api/v1/sessions/:id (signed) -> 204. --------
    let delete_path = format!("/api/v1/sessions/{session_id}");
    let delete_headers = sign_request_headers(
        "DELETE",
        &delete_path,
        b"",
        &cap_b64,
        &device_id,
        &device_priv,
        "e2e-delete-nonce",
    );
    let (ds, _db) = http_request(f.addr, "DELETE", &delete_path, vec![], &delete_headers).await;
    assert_eq!(ds, 204, "delete-session must be 204");

    // -------- Step 12: shut the server task down. --------
    f.server.abort();
}

/// Local copy of `http_common::sign_headers` that owns its tuple `&'static
/// str` keys — kept inline so this file doesn't depend on a helper signature
/// that may evolve. Mirrors §4.1 canonical-string layout.
fn sign_request_headers(
    method: &str,
    path: &str,
    body: &[u8],
    cap_b64: &str,
    device_id: &str,
    device_priv: &[u8; 32],
    nonce: &str,
) -> Vec<(&'static str, String)> {
    let now = Utc::now().to_rfc3339();
    let canonical = omw_remote::CanonicalRequest {
        method: method.to_string(),
        path: path.to_string(),
        query: String::new(),
        ts: now.clone(),
        nonce: nonce.to_string(),
        body_sha256: body_sha256(body),
        device_id: device_id.to_string(),
        protocol_version: 1,
    };
    let sig = Signer { device_priv }.sign(&canonical);
    vec![
        ("authorization", format!("Bearer {cap_b64}")),
        ("x-omw-signature", URL_SAFE_NO_PAD.encode(sig)),
        ("x-omw-nonce", nonce.to_string()),
        ("x-omw-ts", now),
        ("content-type", "application/json".to_string()),
    ]
}
