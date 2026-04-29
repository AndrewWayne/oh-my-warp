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
pub fn set(key: &KeyRef, value: &str) -> Result<(), KeychainError> {
    let name = account_for(key);
    backend::set(name, value)
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
