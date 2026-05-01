//! Multi-origin pinning (spec §8.2): the server accepts a WS upgrade whose
//! `Origin` header matches ANY entry in `ServerConfig::pinned_origins`, and
//! rejects everything else with `403 origin_mismatch`.
//!
//! Loopback + tailnet are the canonical pair (Gap 4: Tailscale Serve
//! auto-bootstrap), but the server doesn't care what the entries are — it
//! just compares strings exactly.

#[path = "ws_common/mod.rs"]
mod ws_common;

use std::sync::Arc;
use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use chrono::Utc;
use ed25519_dalek::SigningKey;
use omw_remote::{
    make_router, Capability, CapabilityToken, HostKey, NonceStore, RevocationList, ServerConfig,
    ShellSpec,
};
use omw_server::{SessionRegistry, SessionSpec};
use std::collections::HashMap;
use tokio::net::TcpListener;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

use ws_common::{
    body_hash, build_handshake_canonical, device_id_from_pubkey, echo_shell, sign_canonical,
    shell_to_session_spec, WsFixture,
};

/// Spawn a server pinned to TWO origins: loopback + tailnet hostname. Returns
/// the same `WsFixture` shape so the existing handshake helpers can reuse it.
/// `pinned_origin` on the fixture echoes the FIRST entry (loopback) — tests
/// that need the second origin pass it explicitly.
async fn spawn_server_with_origins(origins: Vec<String>) -> WsFixture {
    let host = HostKey::generate();
    let host_pubkey = host.pubkey();
    let host = Arc::new(host);

    let device = SigningKey::from_bytes(&[42u8; 32]);
    let device_pubkey: [u8; 32] = device.verifying_key().to_bytes();
    let device_id = device_id_from_pubkey(&device_pubkey);

    let cap_token = CapabilityToken::issue(
        &host,
        device_pubkey,
        device_id.clone(),
        vec![Capability::PtyRead, Capability::PtyWrite],
        Duration::from_secs(30 * 24 * 3600),
    );
    let cap_token_b64 = cap_token.to_base64url();

    let nonce_store = NonceStore::new(Duration::from_secs(60));
    let revocations = RevocationList::new();

    let shell = echo_shell();
    let registry = SessionRegistry::new();
    let session_uuid = registry
        .register(shell_to_session_spec("default", &shell))
        .await
        .expect("register echo session");
    let session_id = session_uuid.to_string();

    let pinned_origin_first = origins
        .first()
        .cloned()
        .unwrap_or_else(|| "https://omw.test".to_string());

    let cfg = ServerConfig {
        bind: "127.0.0.1:0".parse().unwrap(),
        host_key: host.clone(),
        pinned_origins: origins,
        inactivity_timeout: Duration::from_secs(60),
        revocations: revocations.clone(),
        nonce_store: nonce_store.clone(),
        pairings: None,
        shell,
        pty_registry: registry.clone(),
        host_id: "omw-host".to_string(),
    };

    let router = make_router(cfg);
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind 127.0.0.1:0");
    let addr = listener.local_addr().expect("local_addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, router.into_make_service()).await;
    });

    // Tickle warnings off helpers we don't reach in this file.
    let _ = HashMap::<String, String>::new();
    let _ = SessionSpec {
        name: String::new(),
        command: String::new(),
        args: vec![],
        cwd: None,
        env: None,
        cols: None,
        rows: None,
    };
    let _ = ShellSpec::default_for_host();

    WsFixture {
        addr,
        host,
        host_pubkey,
        device,
        device_id,
        cap_token,
        cap_token_b64,
        nonce_store,
        revocations,
        pinned_origin: pinned_origin_first,
        session_id,
        registry,
    }
}

/// Build a signed handshake request for the multi-origin fixture, using a
/// caller-supplied `Origin` header (so each test can drive a different value).
fn build_signed_request(
    f: &WsFixture,
    nonce: &str,
    origin: &str,
    now: chrono::DateTime<Utc>,
) -> http::Request<()> {
    let canonical = build_handshake_canonical(f, now, nonce);
    let sig = sign_canonical(f, &canonical);

    let url = format!("ws://{}/ws/v1/pty/{}", f.addr, f.session_id);
    let mut req = url.into_client_request().expect("valid ws URL");
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
async fn loopback_origin_accepted_when_in_pinned_list() {
    let origins = vec![
        "http://127.0.0.1:8787".to_string(),
        "https://laptop.tailnet.ts.net".to_string(),
    ];
    let f = spawn_server_with_origins(origins).await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    let now = Utc::now();
    let req = build_signed_request(&f, "nonce-multi-loopback", "http://127.0.0.1:8787", now);

    let connect = timeout(
        Duration::from_secs(5),
        tokio_tungstenite::connect_async(req),
    )
    .await
    .expect("WS connect must not hang");
    let (_ws, resp) = connect.expect("WS upgrade must succeed for loopback origin");
    assert_eq!(
        resp.status().as_u16(),
        101,
        "expected 101 for loopback origin in pinned list; got {}",
        resp.status()
    );

    // Touch helper to silence the unused-import lint on Windows.
    let _ = body_hash(b"");
}

#[tokio::test]
async fn tailnet_origin_accepted_when_in_pinned_list() {
    let origins = vec![
        "http://127.0.0.1:8787".to_string(),
        "https://laptop.tailnet.ts.net".to_string(),
    ];
    let f = spawn_server_with_origins(origins).await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    let now = Utc::now();
    let req = build_signed_request(
        &f,
        "nonce-multi-tailnet",
        "https://laptop.tailnet.ts.net",
        now,
    );

    let connect = timeout(
        Duration::from_secs(5),
        tokio_tungstenite::connect_async(req),
    )
    .await
    .expect("WS connect must not hang");
    let (_ws, resp) = connect.expect("WS upgrade must succeed for tailnet origin");
    assert_eq!(
        resp.status().as_u16(),
        101,
        "expected 101 for tailnet origin in pinned list; got {}",
        resp.status()
    );
}

#[tokio::test]
async fn unknown_origin_rejected_with_403() {
    let origins = vec![
        "http://127.0.0.1:8787".to_string(),
        "https://laptop.tailnet.ts.net".to_string(),
    ];
    let f = spawn_server_with_origins(origins).await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    let now = Utc::now();
    let req = build_signed_request(
        &f,
        "nonce-multi-attacker",
        "https://attacker.example.com",
        now,
    );

    let result = timeout(
        Duration::from_secs(5),
        tokio_tungstenite::connect_async(req),
    )
    .await
    .expect("connect must not hang");
    let err = result.expect_err("attacker origin must reject");
    let s = format!("{err}");
    assert!(
        s.contains("403") || s.contains("origin_mismatch"),
        "expected 403/origin_mismatch for attacker origin; got: {s}"
    );
}
