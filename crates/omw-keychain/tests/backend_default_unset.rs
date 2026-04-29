//! Behaviour when `OMW_KEYCHAIN_BACKEND` is unset entirely. Should match
//! `auto` exactly: macOS resolves to OS, everywhere else to memory.

mod common;

use std::sync::Once;

#[cfg(not(target_os = "macos"))]
use common::{key_ref, unique_name};

static INIT: Once = Once::new();

fn init() {
    INIT.call_once(|| {
        // TODO(rust-2024): wrap in unsafe once edition migrates.
        std::env::remove_var("OMW_KEYCHAIN_BACKEND");
    });
}

#[cfg(not(target_os = "macos"))]
#[test]
fn unset_backend_on_non_mac_defaults_to_memory() {
    init();
    assert_eq!(
        omw_keychain::current_backend_kind(),
        omw_keychain::BackendKind::Memory
    );
    let kr = key_ref(&unique_name("default"));
    omw_keychain::set(&kr, "x").unwrap();
    assert_eq!(omw_keychain::get(&kr).unwrap().expose(), "x");
    let _ = omw_keychain::delete(&kr);
}

#[cfg(target_os = "macos")]
#[test]
fn unset_backend_on_mac_defaults_to_os() {
    init();
    assert_eq!(
        omw_keychain::current_backend_kind(),
        omw_keychain::BackendKind::Os
    );
}
