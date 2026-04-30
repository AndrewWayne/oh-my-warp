//! HTTP/WS server skeleton for `omw-remote`. Phase E exposes only the
//! `/ws/v1/pty/:session_id` route; pair-redeem and others land in later
//! phases.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, State, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use chrono::Utc;

use crate::auth::{AuthError, CanonicalRequest, Verifier};
use crate::capability::Capability;
use crate::host_key::HostKey;
use crate::pairing::Pairings;
use crate::replay::NonceStore;
use crate::revocations::RevocationList;
use crate::ws::pty::{handle_authed_socket, ShellSpec};

/// Configuration for the `omw-remote` server.
#[derive(Clone)]
pub struct ServerConfig {
    /// Address to bind. Tests pass `127.0.0.1:0` for an OS-assigned port.
    pub bind: SocketAddr,
    /// Long-lived host pairing key. Used to verify capability tokens during
    /// handshake AND to sign outbound WS frames.
    pub host_key: Arc<HostKey>,
    /// Pinned origin per spec §8.1, e.g. `https://host.tailnet.ts.net` or
    /// `https://127.0.0.1:8787`. Mismatch -> `403 origin_mismatch`.
    pub pinned_origin: String,
    /// Tear down a WS session that hasn't sent a frame for this long.
    /// Spec default: 60 s. Tests shrink this to e.g. 2 s.
    pub inactivity_timeout: Duration,
    /// Shared revocation set; checked per-frame (§7.3 step 4).
    pub revocations: Arc<RevocationList>,
    /// Shared nonce store for handshake replay defense (the WS upgrade is a
    /// signed request just like an HTTP request — see §7.1).
    pub nonce_store: Arc<NonceStore>,
    /// Pairings registry used by future pair-redeem route. Phase E doesn't
    /// route to it yet, but the server config carries it so test setup can
    /// keep one shared instance across tests.
    pub pairings: Option<Arc<Pairings>>,
    /// Shell spec for newly-spawned WS PTY sessions.
    pub shell: ShellSpec,
}

/// Internal shared state passed to axum handlers. Cloning is cheap; all
/// fields are `Arc` or small.
#[derive(Clone)]
pub(crate) struct AppState {
    pub host_key: Arc<HostKey>,
    pub host_pubkey: [u8; 32],
    pub pinned_origin: String,
    pub inactivity_timeout: Duration,
    pub revocations: Arc<RevocationList>,
    pub nonce_store: Arc<NonceStore>,
    pub shell: ShellSpec,
}

/// Run the server forever. Equivalent to `axum::serve(listener, make_router(config))`.
pub async fn serve(config: ServerConfig) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(config.bind).await?;
    axum::serve(listener, make_router(config).into_make_service()).await
}

/// Build the axum router. Exposed separately from [`serve`] so tests can
/// drive the router via `tower::ServiceExt::oneshot` or by binding to
/// `127.0.0.1:0` themselves.
pub fn make_router(config: ServerConfig) -> axum::Router {
    let host_pubkey = config.host_key.pubkey();
    let state = AppState {
        host_key: config.host_key,
        host_pubkey,
        pinned_origin: config.pinned_origin,
        inactivity_timeout: config.inactivity_timeout,
        revocations: config.revocations,
        nonce_store: config.nonce_store,
        shell: config.shell,
    };
    axum::Router::new()
        .route("/ws/v1/pty/:session_id", get(ws_handler))
        .with_state(state)
}

/// `GET /ws/v1/pty/:session_id` handler. Performs the §7.1 handshake checks,
/// then on success delegates to the per-socket bridge loop.
async fn ws_handler(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> axum::response::Response {
    // 1. Origin pinning (§8.2). Mismatch -> 403.
    let origin = headers
        .get("origin")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    if origin != state.pinned_origin {
        return (StatusCode::FORBIDDEN, "origin_mismatch").into_response();
    }

    // 2. Required signed-request headers.
    let auth = match headers.get("authorization").and_then(|h| h.to_str().ok()) {
        Some(s) => s,
        None => return (StatusCode::UNAUTHORIZED, "missing_authorization").into_response(),
    };
    let cap_b64 = match auth.strip_prefix("Bearer ") {
        Some(s) => s.trim(),
        None => return (StatusCode::UNAUTHORIZED, "bad_authorization").into_response(),
    };
    let sig_b64 = match headers.get("x-omw-signature").and_then(|h| h.to_str().ok()) {
        Some(s) => s,
        None => return (StatusCode::UNAUTHORIZED, "missing_signature").into_response(),
    };
    let nonce = match headers.get("x-omw-nonce").and_then(|h| h.to_str().ok()) {
        Some(s) => s,
        None => return (StatusCode::UNAUTHORIZED, "missing_nonce").into_response(),
    };
    let ts = match headers.get("x-omw-ts").and_then(|h| h.to_str().ok()) {
        Some(s) => s,
        None => return (StatusCode::UNAUTHORIZED, "missing_ts").into_response(),
    };

    let sig_bytes = match URL_SAFE_NO_PAD.decode(sig_b64) {
        Ok(b) if b.len() == 64 => b,
        _ => return (StatusCode::UNAUTHORIZED, "bad_signature_encoding").into_response(),
    };
    let mut sig = [0u8; 64];
    sig.copy_from_slice(&sig_bytes);

    // 3. Reconstruct canonical request from path + headers.
    //    Body is empty for the WS upgrade; SHA256("") is a known constant.
    let body_sha256 = sha256_empty();
    // We need the device_id for the canonical request, but we only learn it
    // by parsing the capability token. Parse first.
    let cap_token = match crate::capability::CapabilityToken::from_base64url(cap_b64) {
        Ok(t) => t,
        Err(_) => return (StatusCode::UNAUTHORIZED, "capability_invalid").into_response(),
    };
    let canonical = CanonicalRequest {
        method: "GET".to_string(),
        path: format!("/ws/v1/pty/{}", session_id),
        query: String::new(),
        ts: ts.to_string(),
        nonce: nonce.to_string(),
        body_sha256,
        device_id: cap_token.device_id.clone(),
        protocol_version: 1,
    };

    let verifier = Verifier::new(state.host_pubkey, state.nonce_store.clone());
    let now = Utc::now();
    let device_id = match verifier.verify(&canonical, &sig, cap_b64, Capability::PtyWrite, now) {
        Ok(id) => id,
        Err(e) => {
            let code = match e {
                AuthError::CapabilityExpired
                | AuthError::CapabilityInvalid
                | AuthError::SignatureInvalid
                | AuthError::CapabilityScope
                | AuthError::DeviceRevoked
                | AuthError::TsSkew
                | AuthError::NonceReplayed
                | AuthError::InvalidBody => StatusCode::UNAUTHORIZED,
            };
            return (code, format!("{e}")).into_response();
        }
    };

    // Per-frame revocation: check at handshake too, even though we'll re-check on every frame.
    if state.revocations.is_revoked(&device_id) {
        return (StatusCode::UNAUTHORIZED, "device_revoked").into_response();
    }

    // 4. All checks passed — accept upgrade and hand off to the bridge loop.
    let cap_for_session = cap_token.clone();
    let device_id_for_session = device_id.clone();
    ws.on_upgrade(move |socket| {
        handle_authed_socket(socket, state, cap_for_session, device_id_for_session)
    })
}

fn sha256_empty() -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let h = Sha256::digest(b"");
    let mut out = [0u8; 32];
    out.copy_from_slice(&h);
    out
}
