//! Shared helpers for the integration test binaries.
//!
//! Each integration-test file in Cargo is compiled into its own binary and
//! run as its own process, so per-process state (env vars, the in-memory
//! backend's `OnceLock`s) is isolated between files but shared between
//! `#[test]`s in the same file. Helpers that mutate env vars therefore use
//! a `std::sync::Once` guard so that concurrent `#[test]`s within a binary
//! initialise once and can never race with each other on `set_var`.
//!
//! Each test binary recompiles this module and only references a subset of
//! the helpers; the crate-wide `#[allow(dead_code)]` below silences the
//! resulting per-binary `dead_code` warnings (which would otherwise break
//! `cargo clippy -- -D warnings`).

#![allow(dead_code)]

use std::sync::Once;

static INIT: Once = Once::new();

/// Set `OMW_KEYCHAIN_BACKEND=memory` for this test process. Idempotent.
pub fn init_memory_backend() {
    INIT.call_once(|| {
        // TODO(rust-2024): wrap in unsafe once edition migrates.
        std::env::set_var("OMW_KEYCHAIN_BACKEND", "memory");
    });
}

/// Mint a name unique within this process (and overwhelmingly likely to be
/// unique across processes too — used to keep macOS keychain tests from
/// stepping on each other if they're ever run for real).
pub fn unique_name(prefix: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::SeqCst);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("omw/{prefix}-{nanos:032x}-{n:08x}")
}

/// Construct a `KeyRef::Keychain { name }`.
pub fn key_ref(name: &str) -> omw_config::KeyRef {
    omw_config::KeyRef::Keychain {
        name: name.to_string(),
    }
}

/// Mint a sentinel string. Made highly distinctive so any 4-char window
/// inside it is unlikely to collide with format strings used by `Debug` /
/// `Display` impls under test. Trailing `-secret` is intentional: it is the
/// literal substring most likely to appear in error messages and we want to
/// make sure the redaction logic still hides it.
pub fn unique_sentinel() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::SeqCst);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!(
        "omwsentinel-{:016x}-{:08x}-4f{:06x}-secret",
        nanos,
        n,
        n.wrapping_mul(0x9e3779b97f4a7c15)
    )
}

const MIN_WINDOW: usize = 4;

/// Assert that `rendered` does not contain any contiguous substring of
/// `secret` of length `MIN_WINDOW` or longer. This catches `Debug`
/// implementations that truncate but otherwise leak.
pub fn assert_no_secret_leak(rendered: &str, secret: &str) {
    let bytes = secret.as_bytes();
    let len = bytes.len();
    if len < MIN_WINDOW {
        return;
    }
    for window in MIN_WINDOW..=len {
        for start in 0..=(len - window) {
            let end = start + window;
            // Skip non-UTF-8 boundaries.
            if !secret.is_char_boundary(start) || !secret.is_char_boundary(end) {
                continue;
            }
            let slice = &secret[start..end];
            assert!(
                !rendered.contains(slice),
                "rendered output contained secret slice (length {window})"
            );
        }
    }
}
