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
use std::time::Instant;

use axum::extract::{Path, RawQuery, State, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{delete, get, post};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use chrono::{DateTime, Utc};
use ed25519_dalek::{Signature, Verifier as _, VerifyingKey};
use serde::Deserialize;
use uuid::Uuid;

use crate::auth::{AuthError, CanonicalRequest, Verifier};
use crate::capability::{Capability, CapabilityError, CapabilityToken};
use crate::host_key::HostKey;
use crate::http;
use crate::pairing::Pairings;
use crate::replay::{NonceError, NonceStore};
use crate::request_log::{RequestLog, RequestLogEntry};
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
    /// Pinned origins per spec §8.1, e.g. `https://host.tailnet.ts.net` and/or
    /// `http://127.0.0.1:8787`. The request's `Origin` must match one entry
    /// exactly; otherwise -> `403 origin_mismatch`. An empty list rejects every
    /// upgrade (no origin can match) — callers should always pass at least one
    /// allowed origin.
    pub pinned_origins: Vec<String>,
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
    /// Append-only auth audit trail (spec §4.2 step 10, §6.3). Every pair
    /// redeem and every authenticated request appends one accept/reject row.
    /// `None` disables logging — the request path is unaffected either way.
    pub request_log: Option<Arc<RequestLog>>,
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
    pub pinned_origins: Vec<String>,
    pub inactivity_timeout: Duration,
    pub revocations: Arc<RevocationList>,
    pub nonce_store: Arc<NonceStore>,
    pub shell: ShellSpec,
    pub pty_registry: Arc<omw_server::SessionRegistry>,
    pub pairings: Option<Arc<Pairings>>,
    pub request_log: Option<Arc<RequestLog>>,
    pub host_id: String,
}

impl AppState {
    /// Best-effort append to the auth audit trail. A logging failure is
    /// warned and swallowed — an audit-log write must never deny service
    /// (spec §6.3 calls this a durable trail, not a request precondition).
    /// No-op when no `request_log` is configured.
    pub(crate) fn log_request(&self, entry: RequestLogEntry) {
        if let Some(log) = &self.request_log {
            if let Err(e) = log.append(entry) {
                eprintln!("[omw-warn] request_log append failed: {e}");
            }
        }
    }
}

/// Read a header as an owned `String`, or `None` if absent / non-UTF-8.
/// Used to populate best-effort audit fields from an incoming request.
pub(crate) fn header_str(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|h| h.to_str().ok())
        .map(str::to_string)
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
        pinned_origins: config.pinned_origins,
        inactivity_timeout: config.inactivity_timeout,
        revocations: config.revocations,
        nonce_store: config.nonce_store,
        shell: config.shell,
        pty_registry: config.pty_registry,
        pairings: config.pairings,
        request_log: config.request_log,
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
        .fallback(crate::web_assets::serve_static)
        .with_state(state)
}

/// `GET /ws/v1/pty/:session_id` handler. Performs the §7.1 handshake checks,
/// then on success looks up the session in the registry and bridges it.
///
/// Two auth paths are supported:
/// - HTTP-header auth (`Authorization: Bearer <cap>` + `X-Omw-{Signature,Nonce,Ts}`),
///   used by native clients that can set headers on the upgrade.
/// - URL connect-token (`?ct=<base64url>`), used by browser `WebSocket` since
///   the JS constructor cannot set custom HTTP headers. The `ct` envelope
///   carries the same signed-request bits the headers would carry.
///
/// If `?ct=` is present, the URL path is taken; otherwise we fall back to
/// HTTP-header auth. Origin pinning (§8.2) applies to both.
async fn ws_handler(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    RawQuery(raw_query): RawQuery,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> axum::response::Response {
    let origin = headers
        .get("origin")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    let host_hdr = headers
        .get("host")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    eprintln!(
        "[omw-debug] ws_handler entry: session_id={session_id} origin={origin:?} host={host_hdr:?} has_query={} pinned={:?}",
        raw_query.is_some(),
        state.pinned_origins,
    );

    let request_path = format!("/ws/v1/pty/{}", session_id);

    // 1. Origin pinning (§8.2). Mismatch -> 403. Applies to both auth paths.
    //    The Origin header must match ANY entry in `pinned_origins`. An empty
    //    list rejects every upgrade — never accidentally accept all.
    if !state.pinned_origins.iter().any(|o| o == origin) {
        eprintln!("[omw-debug] ws_handler -> 403 origin_mismatch (origin={origin:?})");
        state.log_request(ws_log_entry(
            &request_path,
            None,
            &headers,
            false,
            Some("origin_mismatch"),
        ));
        return (StatusCode::FORBIDDEN, "origin_mismatch").into_response();
    }

    let now = Utc::now();

    // 2. Look for `?ct=...` in the query string. If present, use the URL
    //    connect-token path; otherwise fall through to HTTP-header auth. Both
    //    paths reduce to `(device_id, cap_token)` on success or
    //    `(status, code)` on reject, so a single append covers the decision.
    let raw_ct = raw_query
        .as_deref()
        .and_then(extract_ct_query_param)
        .map(|s| s.to_string());

    let auth = if let Some(ct) = raw_ct {
        // 300 s skew window: the spec default of 30 s assumes both endpoints
        // run NTP-tight clocks, but mobile browsers can sit on a stale
        // connect-token bundle for tens of seconds when the tab is backgrounded
        // or the WS upgrade gets queued behind page-load work, and consumer
        // phones routinely drift 1-2 minutes off true UTC. Anti-replay is
        // still bounded — `nonce_store` dedups within its 60 s window, and
        // the capability-token TTL caps the long horizon.
        verify_connect_token(
            &ct,
            &request_path,
            &state.host_pubkey,
            &state.nonce_store,
            &state.revocations,
            Capability::PtyWrite,
            300,
            now,
        )
        .map_err(|e| (e.status(), e.code()))
    } else {
        eprintln!("[omw-debug] ws_handler -> falling back to header auth (no ?ct=)");
        authenticate_with_headers(&state, &headers, &session_id, now)
    };

    let (device_id, cap_token) = match auth {
        Ok(pair) => pair,
        Err((status, code)) => {
            eprintln!("[omw-debug] ws_handler -> auth FAILED ({code})");
            state.log_request(ws_log_entry(
                &request_path,
                None,
                &headers,
                false,
                Some(code),
            ));
            return (status, code).into_response();
        }
    };
    eprintln!("[omw-debug] ws_handler -> auth ok device_id={device_id}");

    if state.revocations.is_revoked(&device_id) {
        eprintln!("[omw-debug] ws_handler -> 401 device_revoked");
        state.log_request(ws_log_entry(
            &request_path,
            Some(device_id.clone()),
            &headers,
            false,
            Some("device_revoked"),
        ));
        return (StatusCode::UNAUTHORIZED, "device_revoked").into_response();
    }

    // Auth accepted (§4.2 step 10). The session lookup below is request
    // processing, not an auth decision, so it is not logged as a second row.
    state.log_request(ws_log_entry(
        &request_path,
        Some(device_id.clone()),
        &headers,
        true,
        None,
    ));

    // 3. Look up the session in the registry. If session_id is not a UUID or
    //    not registered, reject the upgrade with 404.
    let session_uuid = match Uuid::parse_str(&session_id) {
        Ok(u) => u,
        Err(_) => {
            eprintln!("[omw-debug] ws_handler -> 404 session_id not a uuid");
            return (StatusCode::NOT_FOUND, "session_not_found").into_response();
        }
    };
    let (snapshot, parser_size, output_rx) = match state
        .pty_registry
        .subscribe_with_state_and_size(session_uuid)
    {
        Some(triple) => triple,
        None => {
            eprintln!("[omw-debug] ws_handler -> 404 session not in registry: {session_uuid}");
            return (StatusCode::NOT_FOUND, "session_not_found").into_response();
        }
    };
    let (parser_rows, parser_cols) = parser_size;
    eprintln!(
        "[omw-debug] ws_handler -> registry hit, upgrading (snapshot {} bytes, parser size {}x{})",
        snapshot.len(),
        parser_rows,
        parser_cols,
    );

    // 4. Accept upgrade.
    let cap_for_session = cap_token.clone();
    let device_id_for_session = device_id.clone();
    ws.on_upgrade(move |socket| {
        handle_authed_socket(
            socket,
            state,
            cap_for_session,
            device_id_for_session,
            session_uuid,
            snapshot,
            (parser_rows, parser_cols),
            output_rx,
        )
    })
}

/// Pull the `ct` query parameter out of the raw query string. Returns the
/// (un-percent-decoded) value if present. Browser-side `pty-ws.ts` sends
/// base64url which has no characters needing decoding, so we don't bother.
fn extract_ct_query_param(query: &str) -> Option<&str> {
    for pair in query.split('&') {
        if let Some(rest) = pair.strip_prefix("ct=") {
            return Some(rest);
        }
    }
    None
}

/// Run the HTTP-header signed-request ladder (the native-client path). Same
/// behavior as before connect-token support was added.
///
/// On reject, returns `(status, code)` — the `code` is the closed-vocabulary
/// wire error (also the response body), which the caller both returns to the
/// client and records as the `request_log` reject reason.
fn authenticate_with_headers(
    state: &AppState,
    headers: &HeaderMap,
    session_id: &str,
    now: DateTime<Utc>,
) -> Result<(String, CapabilityToken), (StatusCode, &'static str)> {
    let auth = match headers.get("authorization").and_then(|h| h.to_str().ok()) {
        Some(s) => s,
        None => {
            return Err((StatusCode::UNAUTHORIZED, "missing_authorization"));
        }
    };
    let cap_b64 = match auth.strip_prefix("Bearer ") {
        Some(s) => s.trim(),
        None => return Err((StatusCode::UNAUTHORIZED, "bad_authorization")),
    };
    let sig_b64 = match headers.get("x-omw-signature").and_then(|h| h.to_str().ok()) {
        Some(s) => s,
        None => return Err((StatusCode::UNAUTHORIZED, "missing_signature")),
    };
    let nonce = match headers.get("x-omw-nonce").and_then(|h| h.to_str().ok()) {
        Some(s) => s,
        None => return Err((StatusCode::UNAUTHORIZED, "missing_nonce")),
    };
    let ts = match headers.get("x-omw-ts").and_then(|h| h.to_str().ok()) {
        Some(s) => s,
        None => return Err((StatusCode::UNAUTHORIZED, "missing_ts")),
    };

    let sig_bytes = match URL_SAFE_NO_PAD.decode(sig_b64) {
        Ok(b) if b.len() == 64 => b,
        _ => return Err((StatusCode::UNAUTHORIZED, "bad_signature_encoding")),
    };
    let mut sig = [0u8; 64];
    sig.copy_from_slice(&sig_bytes);

    let body_sha256 = sha256_empty();
    let cap_token = match crate::capability::CapabilityToken::from_base64url(cap_b64) {
        Ok(t) => t,
        Err(_) => return Err((StatusCode::UNAUTHORIZED, "capability_invalid")),
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
    let device_id = match verifier.verify(&canonical, &sig, cap_b64, Capability::PtyWrite, now) {
        Ok(id) => id,
        Err(e) => {
            // Every variant historically maps to 401 on this path; the code is
            // the AuthError Display string (unchanged response body).
            let code = match e {
                AuthError::CapabilityInvalid => "capability_invalid",
                AuthError::CapabilityExpired => "capability_expired",
                AuthError::DeviceRevoked => "device_revoked",
                AuthError::TsSkew => "ts_skew",
                AuthError::NonceReplayed => "nonce_replayed",
                AuthError::SignatureInvalid => "signature_invalid",
                AuthError::CapabilityScope => "capability_scope",
                AuthError::InvalidBody => "invalid_body",
            };
            return Err((StatusCode::UNAUTHORIZED, code));
        }
    };

    Ok((device_id, cap_token))
}

/// Build a `request_log` row for a WS upgrade decision. Nonce/signature come
/// from headers when present; on the `?ct=` path they live inside the opaque
/// connect-token blob, so they stay `None` — the route + decision + device id
/// are the fields §6.3 requires. WS upgrades carry no body, so `body_hash` is
/// always `None`.
fn ws_log_entry(
    route: &str,
    device_id: Option<String>,
    headers: &HeaderMap,
    accepted: bool,
    reason: Option<&str>,
) -> RequestLogEntry {
    RequestLogEntry {
        route: route.to_string(),
        actor_device_id: device_id,
        nonce: header_str(headers, "x-omw-nonce"),
        ts: Utc::now(),
        signature: header_str(headers, "x-omw-signature"),
        body_hash: None,
        accepted,
        reason: reason.map(str::to_string),
    }
}

fn sha256_empty() -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let h = Sha256::digest(b"");
    let mut out = [0u8; 32];
    out.copy_from_slice(&h);
    out
}

/// Wire shape of the `?ct=` connect-token bundle. Mirror of the JSON object
/// emitted by `apps/web-controller/src/lib/pty-ws.ts::buildConnectToken`.
#[derive(Deserialize)]
struct ConnectTokenBundle {
    v: u8,
    device_id: String,
    ts: String,
    nonce: String,
    sig: String,
    capability_token: String,
}

/// Failure modes for the URL connect-token path. The shape mirrors
/// [`AuthError`] but adds [`ConnectTokenError::Malformed`] for bundle-parsing
/// failures (those map to HTTP 400; everything else is 401/403 like §11.1).
#[derive(Debug, PartialEq, Eq)]
pub enum ConnectTokenError {
    /// `?ct=` value isn't valid base64url, isn't valid JSON, or has the wrong
    /// shape / unsupported `v`. -> 400.
    Malformed,
    /// Capability token failed to parse. -> 401.
    CapabilityInvalid,
    /// Capability token signature/expiry checks failed. -> 401.
    CapabilityExpired,
    /// Capability token doesn't grant the required scope. -> 401.
    CapabilityScope,
    /// `device_id` appears in the revocation list. -> 401.
    DeviceRevoked,
    /// `ts` is outside the skew window. -> 401.
    TsSkew,
    /// `nonce` was already seen. -> 403.
    NonceReplayed,
    /// Ed25519 signature didn't verify. -> 401.
    SignatureInvalid,
}

impl ConnectTokenError {
    fn status(&self) -> StatusCode {
        match self {
            ConnectTokenError::Malformed => StatusCode::BAD_REQUEST,
            ConnectTokenError::NonceReplayed => StatusCode::FORBIDDEN,
            _ => StatusCode::UNAUTHORIZED,
        }
    }

    fn code(&self) -> &'static str {
        match self {
            ConnectTokenError::Malformed => "connect_token_invalid",
            ConnectTokenError::CapabilityInvalid => "capability_invalid",
            ConnectTokenError::CapabilityExpired => "capability_expired",
            ConnectTokenError::CapabilityScope => "capability_scope",
            ConnectTokenError::DeviceRevoked => "device_revoked",
            ConnectTokenError::TsSkew => "ts_skew",
            ConnectTokenError::NonceReplayed => "nonce_replayed",
            ConnectTokenError::SignatureInvalid => "signature_invalid",
        }
    }
}

/// Parse a `?ct=` bundle from a WS upgrade URL and run the same §4.2 ladder
/// used for HTTP-header auth. On success, returns `(device_id, cap_token)` so
/// the caller can build the `WsSessionAuth` exactly as in the header path.
///
/// The bundle is the base64url-encoded JSON object documented in
/// `apps/web-controller/src/lib/pty-ws.ts`. The `sig` it carries is over the
/// same `CanonicalRequest::to_bytes()` form the header path uses, with
/// `query=""` and the empty-body SHA-256.
#[allow(clippy::too_many_arguments)] // signature pinned by wiring-2 spec
pub fn verify_connect_token(
    raw_ct: &str,
    request_path: &str,
    host_pubkey: &[u8; 32],
    nonce_store: &NonceStore,
    revocations: &RevocationList,
    required_cap: Capability,
    ts_skew_seconds: i64,
    now: DateTime<Utc>,
) -> Result<(String, CapabilityToken), ConnectTokenError> {
    // 1. base64url-decode and 2. JSON-parse. Either failing -> Malformed (400).
    let json_bytes = URL_SAFE_NO_PAD
        .decode(raw_ct)
        .map_err(|_| ConnectTokenError::Malformed)?;
    let bundle: ConnectTokenBundle =
        serde_json::from_slice(&json_bytes).map_err(|_| ConnectTokenError::Malformed)?;
    if bundle.v != 1 {
        return Err(ConnectTokenError::Malformed);
    }

    // 3. Decode the 64-byte signature (base64url).
    let sig_bytes = URL_SAFE_NO_PAD
        .decode(&bundle.sig)
        .map_err(|_| ConnectTokenError::Malformed)?;
    if sig_bytes.len() != 64 {
        return Err(ConnectTokenError::Malformed);
    }
    let mut sig_arr = [0u8; 64];
    sig_arr.copy_from_slice(&sig_bytes);

    // 4. Parse + verify the embedded capability token.
    let cap_token = CapabilityToken::from_base64url(&bundle.capability_token)
        .map_err(|_| ConnectTokenError::CapabilityInvalid)?;
    cap_token.verify(host_pubkey, now).map_err(|e| match e {
        CapabilityError::Invalid => ConnectTokenError::CapabilityInvalid,
        CapabilityError::Expired => ConnectTokenError::CapabilityExpired,
    })?;

    // 5. Build the canonical request the bundle's `sig` is over. Must mirror
    //    pty-ws.ts::buildConnectToken byte-for-byte: empty query, empty-body
    //    SHA-256, protocol_version=1.
    let canonical = CanonicalRequest {
        method: "GET".to_string(),
        path: request_path.to_string(),
        query: String::new(),
        ts: bundle.ts.clone(),
        nonce: bundle.nonce.clone(),
        body_sha256: sha256_empty(),
        device_id: bundle.device_id.clone(),
        protocol_version: 1,
    };

    // 6. Revocation list (§4.2 step 3).
    if revocations.is_revoked(&bundle.device_id) {
        return Err(ConnectTokenError::DeviceRevoked);
    }

    // 7. Timestamp skew (§4.2 step 4).
    let req_ts = DateTime::parse_from_rfc3339(&bundle.ts)
        .map_err(|_| ConnectTokenError::TsSkew)?
        .with_timezone(&Utc);
    let skew = (now - req_ts).num_seconds().abs();
    if skew > ts_skew_seconds {
        return Err(ConnectTokenError::TsSkew);
    }

    // 8. Verify Ed25519 signature with cap_token.device_pubkey.
    let device_vk = VerifyingKey::from_bytes(&cap_token.device_pubkey)
        .map_err(|_| ConnectTokenError::SignatureInvalid)?;
    let sig = Signature::from_bytes(&sig_arr);
    if device_vk.verify(&canonical.to_bytes(), &sig).is_err() {
        return Err(ConnectTokenError::SignatureInvalid);
    }

    // 9. Capability scope.
    if !cap_token.allows(required_cap) {
        return Err(ConnectTokenError::CapabilityScope);
    }

    // 10. Nonce check + insert (only on the success path, matching Verifier).
    match nonce_store.check_and_insert(&bundle.nonce, Instant::now()) {
        Ok(()) => {}
        Err(NonceError::Replayed) => return Err(ConnectTokenError::NonceReplayed),
    }

    Ok((cap_token.device_id.clone(), cap_token))
}
