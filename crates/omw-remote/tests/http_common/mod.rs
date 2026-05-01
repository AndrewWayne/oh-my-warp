//! Shared helpers for HTTP route tests in `omw-remote`.

#![allow(dead_code)]

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use chrono::Utc;
use ed25519_dalek::SigningKey;
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::Request;
use hyper_util::rt::TokioIo;
use omw_remote::{
    make_router, open_db, CanonicalRequest, Capability, HostKey, NonceStore, PairToken, Pairings,
    RevocationList, ServerConfig, ShellSpec, Signer,
};
use omw_server::{SessionRegistry, SessionSpec};
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use tokio::net::TcpListener;

pub struct HttpFixture {
    pub addr: SocketAddr,
    pub host: Arc<HostKey>,
    pub host_pubkey: [u8; 32],
    pub pairings: Arc<Pairings>,
    pub registry: Arc<SessionRegistry>,
    pub nonce_store: Arc<NonceStore>,
    pub revocations: Arc<RevocationList>,
    pub pinned_origin: String,
    pub host_id: String,
    pub _tempdir: TempDir,
}

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

pub async fn spawn_server() -> HttpFixture {
    let host = Arc::new(HostKey::generate());
    let host_pubkey = host.pubkey();

    let tempdir = TempDir::new().expect("tempdir");
    let db_path = tempdir.path().join("omw.sqlite");
    let conn = open_db(&db_path).expect("open db");
    let pairings = Arc::new(Pairings::new(conn));

    let registry = SessionRegistry::new();
    let nonce_store = NonceStore::new(Duration::from_secs(60));
    let revocations = RevocationList::new();
    let pinned_origin = "https://omw.test".to_string();
    let host_id = "omw-host".to_string();

    let cfg = ServerConfig {
        bind: "127.0.0.1:0".parse().unwrap(),
        host_key: host.clone(),
        pinned_origins: vec![pinned_origin.clone()],
        inactivity_timeout: Duration::from_secs(60),
        revocations: revocations.clone(),
        nonce_store: nonce_store.clone(),
        pairings: Some(pairings.clone()),
        shell: echo_shell(),
        pty_registry: registry.clone(),
        host_id: host_id.clone(),
    };

    let router = make_router(cfg);
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind 127.0.0.1:0");
    let addr = listener.local_addr().expect("local_addr");

    tokio::spawn(async move {
        let _ = axum::serve(listener, router.into_make_service()).await;
    });

    HttpFixture {
        addr,
        host,
        host_pubkey,
        pairings,
        registry,
        nonce_store,
        revocations,
        pinned_origin,
        host_id,
        _tempdir: tempdir,
    }
}

pub fn body_hash(body: &[u8]) -> [u8; 32] {
    let h = Sha256::digest(body);
    let mut out = [0u8; 32];
    out.copy_from_slice(&h);
    out
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

/// Issue a fresh pair token, redeem it under a deterministic device key,
/// return (device key, capability token base64url, device_id).
pub fn pair_device(
    pairings: &Pairings,
    host: &HostKey,
    seed: u8,
    caps: &[Capability],
) -> (SigningKey, String, String) {
    let token = pairings.issue(Duration::from_secs(600)).expect("issue");
    let device = SigningKey::from_bytes(&[seed; 32]);
    let device_pubkey = device.verifying_key().to_bytes();
    let resp = pairings
        .redeem(&token, &device_pubkey, "test-device", host, caps)
        .expect("redeem");
    let cap_b64 = resp.capability_token.to_base64url();
    (device, cap_b64, resp.device_id)
}

/// Send a (possibly signed) HTTP request, return (status, body bytes).
pub async fn http_request(
    addr: SocketAddr,
    method: &str,
    path: &str,
    body: Vec<u8>,
    headers: &[(&str, String)],
) -> (u16, Vec<u8>) {
    let stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
    let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .expect("handshake");
    tokio::spawn(async move {
        let _ = conn.await;
    });

    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .header("host", "127.0.0.1");
    for (k, v) in headers {
        req = req.header(*k, v);
    }
    let req = req.body(Full::new(Bytes::from(body))).expect("build req");
    let resp = sender.send_request(req).await.expect("send");
    let status = resp.status().as_u16();
    let body_bytes = resp
        .into_body()
        .collect()
        .await
        .expect("collect")
        .to_bytes()
        .to_vec();
    (status, body_bytes)
}

/// Build signed-request headers for a given method/path/body. Returns the
/// list of (header, value) tuples to pass to `http_request`.
pub fn sign_headers(
    method: &str,
    path: &str,
    body: &[u8],
    cap_b64: &str,
    device_id: &str,
    device_priv: &[u8; 32],
    nonce: &str,
) -> Vec<(&'static str, String)> {
    let now = Utc::now().to_rfc3339();
    let canonical = CanonicalRequest {
        method: method.to_string(),
        path: path.to_string(),
        query: String::new(),
        ts: now.clone(),
        nonce: nonce.to_string(),
        body_sha256: body_hash(body),
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

/// Convenience: redeem a fresh pair token outside the HTTP path so the test
/// can exercise routes that need an already-paired device.
pub fn fresh_token(pairings: &Pairings) -> PairToken {
    pairings.issue(Duration::from_secs(600)).expect("issue")
}
