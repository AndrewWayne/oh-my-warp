//! CRUD round-trip tests against the in-memory backend.
//!
//! `KeychainError` has no `InvalidScheme` variant in v0.1: the only `KeyRef`
//! variant currently defined is `KeyRef::Keychain { name }`, which always
//! resolves to a valid account. When `KeyRef` gains `env:` and `cmd:`
//! schemes in v0.2+, an `InvalidScheme` variant will be added and a
//! corresponding test added here. This module's coverage is therefore
//! `Keychain`-only by design.

mod common;

use common::{init_memory_backend, key_ref, unique_name};
use omw_keychain::{self as kc, KeychainError};

#[test]
fn set_then_get_returns_same_secret() {
    init_memory_backend();
    let kr = key_ref(&unique_name("crud"));
    kc::set(&kr, "alpha-bravo").unwrap();
    let got = kc::get(&kr).unwrap();
    assert_eq!(got.expose(), "alpha-bravo");
    let _ = kc::delete(&kr);
}

#[test]
fn set_overwrites_previous_value() {
    init_memory_backend();
    let kr = key_ref(&unique_name("crud"));
    kc::set(&kr, "first").unwrap();
    kc::set(&kr, "second").unwrap();
    assert_eq!(kc::get(&kr).unwrap().expose(), "second");
    let _ = kc::delete(&kr);
}

#[test]
fn delete_removes_entry_and_get_returns_not_found() {
    init_memory_backend();
    let kr = key_ref(&unique_name("crud"));
    kc::set(&kr, "v").unwrap();
    kc::delete(&kr).unwrap();
    match kc::get(&kr) {
        Err(KeychainError::NotFound) => {}
        other => panic!("expected NotFound, got {other:?}"),
    }
}

#[test]
fn delete_on_missing_entry_returns_not_found() {
    init_memory_backend();
    let kr = key_ref(&unique_name("crud"));
    match kc::delete(&kr) {
        Err(KeychainError::NotFound) => {}
        other => panic!("expected NotFound, got {other:?}"),
    }
}

#[test]
fn get_on_missing_entry_returns_not_found() {
    init_memory_backend();
    let kr = key_ref(&unique_name("crud"));
    match kc::get(&kr) {
        Err(KeychainError::NotFound) => {}
        other => panic!(
            "expected NotFound, got {:?}",
            other.map(|s| s.expose().to_string())
        ),
    }
}

#[test]
fn empty_secret_string_is_supported() {
    init_memory_backend();
    let kr = key_ref(&unique_name("crud"));
    kc::set(&kr, "").unwrap();
    assert_eq!(kc::get(&kr).unwrap().expose(), "");
    let _ = kc::delete(&kr);
}

#[test]
fn binary_safe_utf8_secret_round_trips() {
    init_memory_backend();
    let kr = key_ref(&unique_name("crud"));
    // NUL byte, multi-byte codepoints, control chars, BMP and astral plane.
    let value = "a\0b\u{1F600}\u{0007}\u{200B}\nz\u{10FFFD}";
    kc::set(&kr, value).unwrap();
    assert_eq!(kc::get(&kr).unwrap().expose(), value);
    let _ = kc::delete(&kr);
}
