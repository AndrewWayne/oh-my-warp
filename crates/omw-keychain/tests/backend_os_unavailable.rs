//! `OMW_KEYCHAIN_BACKEND=os` on platforms without an OS backend must fail closed.
//! macOS and Linux have OS credential store implementations.

#![cfg(not(any(target_os = "macos", target_os = "linux")))]

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
