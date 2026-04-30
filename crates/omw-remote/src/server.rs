//! HTTP/WS server skeleton for `omw-remote`. Phase E exposes only the
//! `/ws/v1/pty/:session_id` route; pair-redeem and others land in later
//! phases.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use crate::host_key::HostKey;
use crate::pairing::Pairings;
use crate::replay::NonceStore;
use crate::revocations::RevocationList;
use crate::ws::pty::ShellSpec;

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

/// Run the server forever. Equivalent to `axum::serve(listener, make_router(config))`.
pub async fn serve(_config: ServerConfig) -> std::io::Result<()> {
    unimplemented!("Phase E executor: bind tcp listener, axum::serve(make_router(config))")
}

/// Build the axum router. Exposed separately from [`serve`] so tests can
/// drive the router via `tower::ServiceExt::oneshot` or by binding to
/// `127.0.0.1:0` themselves.
pub fn make_router(_config: ServerConfig) -> axum::Router {
    unimplemented!("Phase E executor: build axum::Router with /ws/v1/pty/:session_id")
}
