//! Pins the `HostKey` API per BYORC §3.1.
//!
//! The host pairing key is a long-lived Ed25519 keypair. In production it lives
//! in the OS keychain via `omw-keychain`; for Phase D tests we use a temp-file
//! loader. The contract this file pins:
//!
//! - `generate()` produces a fresh keypair.
//! - `save` + `load_or_create` round-trip — same path → same `pubkey()`.
//! - `load_or_create` on a missing path generates and persists.
//! - `pubkey()` and `sign()` are 32 / 64 bytes respectively.

use omw_remote::HostKey;
use tempfile::tempdir;

#[test]
fn round_trips_through_temp_file() {
    let dir = tempdir().expect("temp dir");
    let path = dir.path().join("host.key");

    let original = HostKey::generate();
    let original_pub = original.pubkey();
    original.save(&path).expect("save host key");

    let reloaded = HostKey::load_or_create(&path).expect("reload host key");
    assert_eq!(
        original_pub,
        reloaded.pubkey(),
        "reloading the same path must yield the same pubkey"
    );
}

#[test]
fn load_or_create_generates_when_missing() {
    let dir = tempdir().expect("temp dir");
    let path = dir.path().join("missing.key");
    assert!(!path.exists(), "precondition: path does not yet exist");

    let key = HostKey::load_or_create(&path).expect("create host key");
    assert!(path.exists(), "load_or_create must persist the new key");

    // A second load returns the *same* key, not a freshly generated one.
    let key2 = HostKey::load_or_create(&path).expect("reload host key");
    assert_eq!(key.pubkey(), key2.pubkey());
}

#[test]
fn pubkey_is_32_bytes_and_signature_is_64_bytes() {
    let host = HostKey::generate();
    let pk = host.pubkey();
    assert_eq!(pk.len(), 32);

    let sig = host.sign(b"hello");
    assert_eq!(sig.len(), 64);
}

#[test]
fn distinct_generate_calls_produce_distinct_keys() {
    let a = HostKey::generate();
    let b = HostKey::generate();
    assert_ne!(
        a.pubkey(),
        b.pubkey(),
        "generate() must use OS RNG, not a deterministic seed"
    );
}
