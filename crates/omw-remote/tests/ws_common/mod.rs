//! Shared test harness for Phase E WS tests.
//!
//! Exposes a fixture that:
//! - generates a host pairing key + a deterministic device key,
//! - mints a capability token with `pty:read + pty:write`,
//! - builds a `ServerConfig` with an in-memory revocation list, nonce store,
//!   and a freshly-spawned echo session in the shared registry,
//! - binds the omw-remote router on `127.0.0.1:0` and returns the address,
//! - provides helpers to build signed WS handshake headers and frames.

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use chrono::{DateTime, Utc};
use ed25519_dalek::SigningKey;
use omw_remote::{
    make_router, CanonicalRequest, Capability, CapabilityToken, HostKey, NonceStore,
    RevocationList, ServerConfig, ShellSpec, Signer,
};
use omw_server::{SessionRegistry, SessionSpec};
use sha2::{Digest, Sha256};
use tokio::net::TcpListener;

/// Test fixture spun up per test.
pub struct WsFixture {
    pub addr: std::net::SocketAddr,
    pub host: Arc<HostKey>,
    pub host_pubkey: [u8; 32],
    pub device: SigningKey,
    pub device_id: String,
    pub cap_token: CapabilityToken,
    pub cap_token_b64: String,
    pub nonce_store: Arc<NonceStore>,
    pub revocations: Arc<RevocationList>,
    pub pinned_origin: String,
    /// Registered session id (UUID string) — pre-spawned by `spawn_server`.
    pub session_id: String,
    pub registry: Arc<SessionRegistry>,
}

pub fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0x0F) as usize] as char);
    }
    s
}

pub fn device_id_from_pubkey(pk: &[u8; 32]) -> String {
    let digest = Sha256::digest(pk);
    hex_lower(&digest[..8])
}

pub fn body_hash(body: &[u8]) -> [u8; 32] {
    let h = Sha256::digest(body);
    let mut out = [0u8; 32];
    out.copy_from_slice(&h);
    out
}

/// Build a `ShellSpec` that runs a deterministic echo loop. Used so the
/// PTY-session tests don't depend on whatever the host's `$SHELL` does.
pub fn echo_shell() -> ShellSpec {
    if cfg!(windows) {
        let script =
            "while ($true) { $line = Read-Host; if ($null -eq $line) { break }; Write-Host $line }";
        ShellSpec {
            program: "powershell".into(),
            args: vec!["-NoProfile".into(), "-Command".into(), script.into()],
        }
    } else {
        ShellSpec {
            program: "/bin/sh".into(),
            args: vec![
                "-c".into(),
                "stty -echo 2>/dev/null; while IFS= read -r line; do printf '%s\\n' \"$line\"; done"
                    .into(),
            ],
        }
    }
}

/// Convert a [`ShellSpec`] (OsString) to the registry's [`SessionSpec`]
/// (String). Lossy on non-UTF-8 OS strings — fine for tests.
pub fn shell_to_session_spec(name: &str, shell: &ShellSpec) -> SessionSpec {
    SessionSpec {
        name: name.to_string(),
        command: shell.program.to_string_lossy().into_owned(),
        args: shell
            .args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect(),
        cwd: None,
        env: Some(HashMap::new()),
        cols: None,
        rows: None,
    }
}

/// Bind the omw-remote router on `127.0.0.1:0` with default config; return
/// fixture state. Pre-registers one echo session whose id is exposed as
/// `WsFixture::session_id`.
pub async fn spawn_server() -> WsFixture {
    spawn_server_with_inactivity(Duration::from_secs(60)).await
}

/// Like `spawn_server`, but with a custom inactivity timeout.
pub async fn spawn_server_with_inactivity(inactivity_timeout: Duration) -> WsFixture {
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
    let pinned_origin = "https://omw.test".to_string();

    let shell = echo_shell();

    let registry = SessionRegistry::new();
    // Pre-register an echo session so tests can WS-attach.
    let session_uuid = registry
        .register(shell_to_session_spec("default", &shell))
        .await
        .expect("register echo session");
    let session_id = session_uuid.to_string();

    let cfg = ServerConfig {
        bind: "127.0.0.1:0".parse().unwrap(),
        host_key: host.clone(),
        pinned_origin: pinned_origin.clone(),
        inactivity_timeout,
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
        pinned_origin,
        session_id,
        registry,
    }
}

/// Build a canonical request for the WS upgrade itself (the handshake is a
/// signed HTTP request per spec §7.1).
pub fn build_handshake_canonical(
    f: &WsFixture,
    now: DateTime<Utc>,
    nonce: &str,
) -> CanonicalRequest {
    CanonicalRequest {
        method: "GET".into(),
        path: format!("/ws/v1/pty/{}", f.session_id),
        query: String::new(),
        ts: now.to_rfc3339(),
        nonce: nonce.into(),
        body_sha256: body_hash(b""),
        device_id: f.device_id.clone(),
        protocol_version: 1,
    }
}

/// Sign a canonical request with the fixture's device key.
pub fn sign_canonical(f: &WsFixture, req: &CanonicalRequest) -> [u8; 64] {
    let priv_seed = f.device.to_bytes();
    Signer {
        device_priv: &priv_seed,
    }
    .sign(req)
}

/// Build a `?ct=<base64url>` connect-token bundle that mirrors the JSON shape
/// emitted by `apps/web-controller/src/lib/pty-ws.ts::buildConnectToken`.
///
/// Returns the bundle's base64url string (suitable for splicing into a WS URL
/// as `?ct=<value>`).
///
/// `ts` and `nonce` are exposed as parameters so individual tests can build
/// expired-ts or replayed-nonce variants without re-implementing the bundle.
pub fn make_connect_token(
    device: &SigningKey,
    cap_token_b64: &str,
    device_id: &str,
    session_id: &str,
    ts: DateTime<Utc>,
    nonce: &str,
) -> String {
    let canonical = CanonicalRequest {
        method: "GET".into(),
        path: format!("/ws/v1/pty/{session_id}"),
        query: String::new(),
        ts: ts.to_rfc3339(),
        nonce: nonce.into(),
        body_sha256: body_hash(b""),
        device_id: device_id.into(),
        protocol_version: 1,
    };
    let priv_seed = device.to_bytes();
    let sig = Signer {
        device_priv: &priv_seed,
    }
    .sign(&canonical);

    let bundle = serde_json::json!({
        "v": 1,
        "device_id": device_id,
        "ts": ts.to_rfc3339(),
        "nonce": nonce,
        "sig": URL_SAFE_NO_PAD.encode(sig),
        "capability_token": cap_token_b64,
    });
    let bytes = serde_json::to_vec(&bundle).expect("bundle serializes");
    URL_SAFE_NO_PAD.encode(&bytes)
}
