//! HTTP/WS server for `omw-remote`.
//!
//! Routes:
//! - `GET  /api/v1/host-info`           — unauthenticated discovery (host pubkey).
//! - `POST /api/v1/pair/redeem`         — unauthenticated pair-redeem (rate-limited).
//! - `GET  /api/v1/sessions`            — signed (PtyRead): list sessions.
//! - `POST /api/v1/sessions`            — signed (PtyWrite): spawn a new shell PTY.
//! - `DELETE /api/v1/sessions/:id`      — signed (PtyWrite): kill a session.
//! - `GET  /ws/v1/pty/:session_id`      — signed WS upgrade (PtyWrite).

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, State, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{delete, get, post};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use chrono::Utc;
use uuid::Uuid;

use crate::auth::{AuthError, CanonicalRequest, Verifier};
use crate::capability::Capability;
use crate::host_key::HostKey;
use crate::http;
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
    /// Shared nonce store for handshake replay defense.
    pub nonce_store: Arc<NonceStore>,
    /// Pairings registry — required for the `/api/v1/pair/redeem` route.
    /// `None` disables that route.
    pub pairings: Option<Arc<Pairings>>,
    /// Default shell spec for newly-spawned PTY sessions (HTTP-created).
    pub shell: ShellSpec,
    /// Live PTY-session registry. WS attach + HTTP CRUD share this.
    pub pty_registry: Arc<omw_server::SessionRegistry>,
    /// Friendly host id surfaced in `/api/v1/host-info` and pair-redeem
    /// responses. Defaults to `"omw-host"` if the embedder doesn't override.
    pub host_id: String,
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
    pub pty_registry: Arc<omw_server::SessionRegistry>,
    pub pairings: Option<Arc<Pairings>>,
    pub host_id: String,
}

/// Run the server forever.
pub async fn serve(config: ServerConfig) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(config.bind).await?;
    axum::serve(listener, make_router(config).into_make_service()).await
}

/// Build the axum router.
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
        pty_registry: config.pty_registry,
        pairings: config.pairings,
        host_id: config.host_id,
    };
    axum::Router::new()
        .route("/api/v1/host-info", get(http::host_info::handler))
        .route("/api/v1/pair/redeem", post(http::pair_redeem::handler))
        .route(
            "/api/v1/sessions",
            post(http::sessions::create).get(http::sessions::list),
        )
        .route("/api/v1/sessions/:id", delete(http::sessions::delete))
        .route("/ws/v1/pty/:session_id", get(ws_handler))
        .with_state(state)
}

/// `GET /ws/v1/pty/:session_id` handler. Performs the §7.1 handshake checks,
/// then on success looks up the session in the registry and bridges it.
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
    let body_sha256 = sha256_empty();
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

    if state.revocations.is_revoked(&device_id) {
        return (StatusCode::UNAUTHORIZED, "device_revoked").into_response();
    }

    // 4. Look up the session in the registry. If session_id is not a UUID or
    //    not registered, reject the upgrade with 404.
    let session_uuid = match Uuid::parse_str(&session_id) {
        Ok(u) => u,
        Err(_) => return (StatusCode::NOT_FOUND, "session_not_found").into_response(),
    };
    let output_rx = match state.pty_registry.subscribe(session_uuid) {
        Some(rx) => rx,
        None => return (StatusCode::NOT_FOUND, "session_not_found").into_response(),
    };

    // 5. Accept upgrade.
    let cap_for_session = cap_token.clone();
    let device_id_for_session = device_id.clone();
    ws.on_upgrade(move |socket| {
        handle_authed_socket(
            socket,
            state,
            cap_for_session,
            device_id_for_session,
            session_uuid,
            output_rx,
        )
    })
}

fn sha256_empty() -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let h = Sha256::digest(b"");
    let mut out = [0u8; 32];
    out.copy_from_slice(&h);
    out
}
