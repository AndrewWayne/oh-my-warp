//! Compile-contract guarding `omw_remote::ServerConfig`'s field set.
//!
//! WHY THIS EXISTS: the vendored GUI app (`vendor/warp-stripped/`) is a
//! *separate cargo workspace* that root CI never compiles, yet it constructs
//! `omw_remote::ServerConfig { .. }` in
//! `vendor/warp-stripped/app/src/omw/remote_state.rs::bring_up_daemon`.
//! Adding or renaming a `ServerConfig` field therefore breaks the embedded
//! daemon silently — it only surfaces at release time (`build-mac-dmg.sh`).
//! This happened with #4 (`request_log`) and #5 (`default_pair_write`); see
//! issue #108.
//!
//! This test mirrors that construction as an exhaustive struct literal (no
//! `..rest`, no builder), so any field change makes root CI fail here in-PR.
//!
//! IF THIS STOPS COMPILING because you changed `ServerConfig`: update BOTH
//! this literal AND every out-of-CI constructor — the vendor daemon above,
//! and `crates/omw-cli/src/commands/remote.rs`.

use std::sync::Arc;
use std::time::Duration;

use omw_remote::{HostKey, NonceStore, RevocationList, ServerConfig, ShellSpec};
use omw_server::SessionRegistry;

/// Build a `ServerConfig` naming every field, exactly as the vendor embedded
/// daemon does. The values are throwaway; only the field set is under test.
fn construct_like_vendor() -> ServerConfig {
    ServerConfig {
        bind: "127.0.0.1:0".parse().expect("bind parses"),
        host_key: Arc::new(HostKey::generate()),
        pinned_origins: vec!["http://127.0.0.1:8787".to_string()],
        inactivity_timeout: Duration::from_secs(60),
        revocations: RevocationList::new(),
        nonce_store: NonceStore::new(Duration::from_secs(60)),
        pairings: None,
        request_log: None,
        default_pair_write: false,
        shell: ShellSpec::default_for_host(),
        pty_registry: SessionRegistry::new(),
        host_id: "contract-host".to_string(),
    }
}

#[test]
fn server_config_field_set_matches_vendor_construction() {
    // The construction above is the real assertion: if it compiles, the field
    // set the vendor relies on is intact. A couple of trivial checks keep the
    // value bound live.
    let cfg = construct_like_vendor();
    assert!(!cfg.default_pair_write, "read-only default (I-6)");
    assert!(cfg.bind.ip().is_loopback(), "loopback bind (I-15)");
}
