//! `omw-keychain` — OS keychain wrapper.
//!
//! Public surface:
//! - [`get`], [`set`], [`delete`], [`list_omw`] — CRUD against the resolved backend.
//! - [`current_backend_kind`] — runtime introspection.
//! - [`Secret`] — opaque, zero-on-drop, never `Display`.
//! - [`KeychainError`] — public-record error variants.
//!
//! Backend selection is driven by the `OMW_KEYCHAIN_BACKEND` environment
//! variable (`auto` | `memory` | `os`). See [PRD §10](../../../PRD.md#10) and
//! [`specs/threat-model.md`](../../../specs/threat-model.md) §3.4.

mod backend;
mod error;
mod secret;

pub use backend::{current_backend_kind, BackendKind};
pub use error::KeychainError;
pub use secret::Secret;

use omw_config::KeyRef;

/// Fetch a secret by reference. Returns [`KeychainError::NotFound`] if the
/// entry does not exist.
pub fn get(key: &KeyRef) -> Result<Secret, KeychainError> {
    let name = account_for(key);
    backend::get(name).map(Secret::new)
}

/// Store (creating or overwriting) a secret. The empty string is allowed.
///
/// On the OS backend, performs a write-then-read post-condition check:
/// some macOS configurations (ad-hoc-signed app bundles whose code identity
/// the system doesn't honor for keychain writes) return success from the
/// underlying Security API without persisting the entry. We catch that
/// here and surface [`KeychainError::WriteNotPersisted`] so callers don't
/// silently assume the write took effect.
pub fn set(key: &KeyRef, value: &str) -> Result<(), KeychainError> {
    let name = account_for(key);
    backend::set(name, value)?;
    if backend::current_backend_kind() == backend::BackendKind::Os {
        match backend::get(name) {
            Ok(round_trip) if round_trip == value => Ok(()),
            Ok(_) => Err(KeychainError::WriteNotPersisted),
            Err(KeychainError::NotFound) => Err(KeychainError::WriteNotPersisted),
            Err(e) => Err(e),
        }
    } else {
        Ok(())
    }
}

/// Remove a secret. Returns [`KeychainError::NotFound`] if the entry did not
/// exist (matches the `keyring` crate convention).
pub fn delete(key: &KeyRef) -> Result<(), KeychainError> {
    let name = account_for(key);
    backend::delete(name)
}

/// List the account names of every entry under the `omw` service. Returns
/// raw account names (e.g. `"omw/openai"`), not full `keychain:` URIs.
pub fn list_omw() -> Result<Vec<String>, KeychainError> {
    backend::list_omw()
}

fn account_for(key: &KeyRef) -> &str {
    match key {
        KeyRef::Keychain { name } => name,
    }
}
