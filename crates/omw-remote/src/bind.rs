//! Daemon bind-address policy.
//!
//! Public-internet / tailnet exposure is opt-in (threat-model invariant I-15,
//! PRD §4.2): an `omw-remote` daemon listens on loopback by default, and
//! exposing it on other interfaces is an explicit operator action. Centralised
//! here so every embedder (the `omw remote start` CLI and the embedded GUI
//! daemon) shares one default and one override rule.

use std::net::SocketAddr;

/// Default listen address for an `omw-remote` daemon: loopback only.
/// Exposing on the tailnet / LAN is an explicit opt-in (I-15).
pub const DEFAULT_LISTEN_ADDR: &str = "127.0.0.1:8787";

/// Resolve the address the daemon should bind.
///
/// `override_addr` is the operator's explicit opt-in (e.g. the CLI `--listen`
/// value or the GUI daemon's `OMW_REMOTE_BIND` env var). When present and
/// non-empty it is used verbatim — this is how a user opts into tailnet/LAN
/// exposure (e.g. `0.0.0.0:8787` or a specific tailnet IP). Otherwise the
/// loopback default applies.
pub fn resolve_bind_addr(override_addr: Option<&str>) -> Result<SocketAddr, String> {
    let raw = override_addr
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_LISTEN_ADDR);
    raw.parse()
        .map_err(|e| format!("invalid bind address {raw:?}: {e}"))
}
