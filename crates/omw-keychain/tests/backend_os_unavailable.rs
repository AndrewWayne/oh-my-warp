//! `OMW_KEYCHAIN_BACKEND=os` on a non-macOS platform must fail closed.
//! v0.1 only supports the macOS keychain; Linux/Windows are Beyond v1.

#![cfg(not(target_os = "macos"))]

mod common;

use std::sync::Once;

use common::{key_ref, unique_name};
use omw_keychain::{self as kc, KeychainError};

static INIT: Once = Once::new();

fn init() {
    INIT.call_once(|| {
        // TODO(rust-2024): wrap in unsafe once edition migrates.
        std::env::set_var("OMW_KEYCHAIN_BACKEND", "os");
    });
}

#[test]
fn os_backend_on_non_mac_returns_backend_unavailable() {
    init();
    let kr = key_ref(&unique_name("os-unavail"));

    match kc::get(&kr) {
        Err(KeychainError::BackendUnavailable { .. }) => {}
        other => panic!(
            "get: expected BackendUnavailable, got {:?}",
            other.map(|s| s.expose().to_string())
        ),
    }

    match kc::set(&kr, "x") {
        Err(KeychainError::BackendUnavailable { .. }) => {}
        other => panic!("set: expected BackendUnavailable, got {other:?}"),
    }

    match kc::delete(&kr) {
        Err(KeychainError::BackendUnavailable { .. }) => {}
        other => panic!("delete: expected BackendUnavailable, got {other:?}"),
    }

    match kc::list_omw() {
        Err(KeychainError::BackendUnavailable { .. }) => {}
        other => panic!("list_omw: expected BackendUnavailable, got {other:?}"),
    }
}
