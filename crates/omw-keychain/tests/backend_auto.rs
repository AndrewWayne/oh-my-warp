//! `OMW_KEYCHAIN_BACKEND=auto` resolves to the platform default.
//!
//! `current_backend_kind()` is the contract the keychain crate exposes so
//! this kind of platform-aware test can mechanically assert correctness
//! without poking at internals.

mod common;

use std::sync::Once;

#[cfg(not(target_os = "macos"))]
use common::{key_ref, unique_name};

static INIT: Once = Once::new();

fn init() {
    INIT.call_once(|| {
        // TODO(rust-2024): wrap in unsafe once edition migrates.
        std::env::set_var("OMW_KEYCHAIN_BACKEND", "auto");
    });
}

#[cfg(not(target_os = "macos"))]
#[test]
fn auto_backend_on_non_mac_uses_memory_backend() {
    init();
    assert_eq!(
        omw_keychain::current_backend_kind(),
        omw_keychain::BackendKind::Memory
    );
    // Round-trip to confirm.
    let kr = key_ref(&unique_name("auto"));
    omw_keychain::set(&kr, "x").unwrap();
    assert_eq!(omw_keychain::get(&kr).unwrap().expose(), "x");
    let _ = omw_keychain::delete(&kr);
}

#[cfg(target_os = "macos")]
#[test]
fn auto_backend_on_mac_uses_os_backend() {
    init();
    assert_eq!(
        omw_keychain::current_backend_kind(),
        omw_keychain::BackendKind::Os
    );
    // Pure introspection, no UI side-effect.
}
