//! Public error type for the keychain crate.
//!
//! Variants are public-record (NOT `#[non_exhaustive]`) so tests can
//! construct synthetic instances with record syntax to exercise the
//! redaction guarantees. The hand-rolled `Debug`, `Display`, and
//! `Error::source` impls all redact `reason` and `source` so user secrets
//! that may have been embedded by a misbehaving caller never reach logs.

use std::error::Error;

/// Errors returned by the keychain CRUD surface.
pub enum KeychainError {
    /// No entry exists for the requested key.
    NotFound,

    /// The configured backend cannot service requests on this platform or
    /// in this build (e.g. `OMW_KEYCHAIN_BACKEND=os` on Linux in v0.1).
    BackendUnavailable { reason: String },

    /// `set` returned success at the underlying API, but a follow-up `get`
    /// could not retrieve the value. Surfaces silent persistence failures
    /// (e.g. ad-hoc-signed bundle ACL quirks on macOS) instead of letting
    /// callers think a write succeeded when it did not.
    WriteNotPersisted,

    /// The OS keychain returned an error. The wrapped source is intentionally
    /// hidden from `Debug` and `Display` to avoid leaking secret material a
    /// platform error message may have inlined.
    Os {
        source: Box<dyn Error + Send + Sync>,
    },
}

// Hand-rolled. Match with `{ .. }` so we never bind `reason`/`source` and
// therefore cannot accidentally print them.
impl std::fmt::Debug for KeychainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KeychainError::NotFound => f.write_str("KeychainError::NotFound"),
            KeychainError::BackendUnavailable { .. } => {
                f.write_str("KeychainError::BackendUnavailable { reason: <redacted> }")
            }
            KeychainError::WriteNotPersisted => f.write_str("KeychainError::WriteNotPersisted"),
            KeychainError::Os { .. } => f.write_str("KeychainError::Os { source: <redacted> }"),
        }
    }
}

impl std::fmt::Display for KeychainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KeychainError::NotFound => f.write_str("keychain entry not found"),
            KeychainError::BackendUnavailable { .. } => {
                f.write_str("keychain backend unavailable: <redacted>")
            }
            KeychainError::WriteNotPersisted => f.write_str(
                "keychain write reported success but the value did not persist \
                 (likely ad-hoc-signed bundle ACL on macOS — try re-signing the .app \
                 with a Developer ID, or use `security add-generic-password -A` as \
                 a workaround)",
            ),
            KeychainError::Os { .. } => f.write_str("OS keychain operation failed: <redacted>"),
        }
    }
}

impl Error for KeychainError {
    // Deliberately return `None` to break `Error::source()` chain walkers
    // (invariant I-1 in the threat model): an upstream logger that walks the
    // chain must not be able to reach the underlying OS error's text.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        None
    }
}
