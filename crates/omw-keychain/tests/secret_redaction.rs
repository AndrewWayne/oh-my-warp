//! Redaction guarantees for `Secret` and `KeychainError`.
//!
//! ## Why `Display` is forbidden on `Secret`
//!
//! The static-assertion below fails to compile if `Secret` ever gains a
//! `Display` impl. Without `Display`, `format!("{}", secret)` is a compile
//! error, so it is impossible for an Executor to accidentally interpolate a
//! secret into a log line via the most common formatting path.
//!
//! ## Why `KeychainError` variants are public-record
//!
//! Tests need to construct synthetic `BackendUnavailable`/`Os` errors to
//! check that their `Debug`/`Display` impls redact. `#[non_exhaustive]`
//! would forbid this. The redaction guarantee is the public contract; the
//! variant set is therefore allowed to be observable.
//!
//! ## Why a 4+-char window sweep
//!
//! A naive `Debug` that truncates the leaked value to e.g. its first 16
//! characters would still pass an equality assertion against the full
//! sentinel. The window sweep catches partial leaks at any starting offset
//! and any length down to four characters — short enough to catch real
//! leakage, long enough to avoid coincidental matches against common
//! format-string fragments.

mod common;

static_assertions::assert_not_impl_any!(omw_keychain::Secret: std::fmt::Display);

use common::{assert_no_secret_leak, init_memory_backend, key_ref, unique_name, unique_sentinel};
use omw_keychain::{self as kc, KeychainError, Secret};

#[test]
fn secret_expose_returns_underlying_value() {
    let s = Secret::new("v".to_string());
    assert_eq!(s.expose(), "v");
}

#[test]
fn secret_debug_does_not_leak_value() {
    let sentinel = unique_sentinel();
    let s = Secret::new(sentinel.clone());
    let rendered = format!("{:?}", s);
    assert_no_secret_leak(&rendered, &sentinel);
}

#[test]
fn secret_debug_indicates_redaction() {
    let s = Secret::new("anything".to_string());
    let rendered = format!("{:?}", s);
    assert!(
        rendered.contains("<redacted>")
            || rendered.contains("***")
            || rendered.contains("Secret")
            || rendered.contains("<secret"),
        "expected a redaction marker, got {rendered:?}"
    );
}

#[test]
fn keychain_error_not_found_does_not_leak_secret_in_scope() {
    init_memory_backend();
    // Hold a sentinel-bearing Secret in scope, then trigger NotFound on a
    // distinct, never-set name. We sweep both renderings against the
    // sentinel to make sure a misbehaving Debug doesn't somehow snapshot
    // unrelated process state.
    let sentinel = unique_sentinel();
    let _live = Secret::new(sentinel.clone());
    let kr = key_ref(&unique_name("missing"));
    let err = kc::get(&kr).expect_err("expected NotFound");
    assert!(matches!(err, KeychainError::NotFound));
    assert_no_secret_leak(&format!("{err:?}"), &sentinel);
    assert_no_secret_leak(&format!("{err}"), &sentinel);
}

#[test]
fn keychain_error_backend_unavailable_does_not_leak_reason() {
    let sentinel = unique_sentinel();
    let err = KeychainError::BackendUnavailable {
        reason: format!("contains {sentinel} inside"),
    };
    assert_no_secret_leak(&format!("{err:?}"), &sentinel);
    assert_no_secret_leak(&format!("{err}"), &sentinel);
}

#[test]
fn keychain_error_os_variant_does_not_leak_source_text() {
    let sentinel = unique_sentinel();
    let err = KeychainError::Os {
        source: Box::new(std::io::Error::other(format!("saw {sentinel} in payload"))),
    };
    assert_no_secret_leak(&format!("{err:?}"), &sentinel);
    assert_no_secret_leak(&format!("{err}"), &sentinel);
    // Walk the `Error::source()` chain — every link's Debug+Display must
    // also redact. Our `source()` returns `None` to break the chain (I-1),
    // so this loop runs zero times in practice; the assertion is here to
    // catch a regression where someone exposes the chain.
    let mut current: Option<&(dyn std::error::Error + 'static)> = std::error::Error::source(&err);
    while let Some(e) = current {
        assert_no_secret_leak(&format!("{e:?}"), &sentinel);
        assert_no_secret_leak(&format!("{e}"), &sentinel);
        current = e.source();
    }
}
