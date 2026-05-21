//! Backend selection and the in-memory + OS implementations.
//!
//! The backend is resolved once per process via `OnceLock` based on the
//! `OMW_KEYCHAIN_BACKEND` env var read at first call. Recognised values:
//! `auto` (the default and any unrecognised value), `memory`, `os`. On
//! macOS and Linux, `auto` resolves to the OS credential store; elsewhere it
//! resolves to memory. Explicit `os` fails closed when no OS implementation is
//! available.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use crate::error::KeychainError;

const SERVICE: &str = "omw";

/// The kind of backend the process resolved at startup. Exposed for
/// platform-aware integration tests; production code rarely needs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
    Memory,
    Os,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Resolved {
    Memory,
    Os,
    OsUnavailable,
}

fn resolve() -> Resolved {
    let raw = std::env::var("OMW_KEYCHAIN_BACKEND").ok();
    let normalized = raw.as_deref().map(str::trim).unwrap_or("auto");
    match normalized {
        "memory" => Resolved::Memory,
        "os" => {
            if os_backend_available() {
                Resolved::Os
            } else {
                Resolved::OsUnavailable
            }
        }
        // "auto" or unrecognized — fall through to platform default.
        _ => {
            if os_backend_available() {
                Resolved::Os
            } else {
                Resolved::Memory
            }
        }
    }
}

fn os_backend_available() -> bool {
    cfg!(any(target_os = "macos", target_os = "linux"))
}

fn resolved() -> Resolved {
    static CELL: OnceLock<Resolved> = OnceLock::new();
    *CELL.get_or_init(resolve)
}

/// Returns the backend kind selected at process start. The `OsUnavailable`
/// sentinel maps to [`BackendKind::Os`] so callers can distinguish "we asked
/// for OS" from "we got memory" purely from this enum.
pub fn current_backend_kind() -> BackendKind {
    match resolved() {
        Resolved::Memory => BackendKind::Memory,
        Resolved::Os | Resolved::OsUnavailable => BackendKind::Os,
    }
}

pub(crate) fn get(account: &str) -> Result<String, KeychainError> {
    match resolved() {
        Resolved::Memory => memory_get(account),
        Resolved::Os => os_impl::get(account),
        Resolved::OsUnavailable => Err(backend_unavailable()),
    }
}

pub(crate) fn set(account: &str, value: &str) -> Result<(), KeychainError> {
    match resolved() {
        Resolved::Memory => memory_set(account, value),
        Resolved::Os => os_impl::set(account, value),
        Resolved::OsUnavailable => Err(backend_unavailable()),
    }
}

pub(crate) fn delete(account: &str) -> Result<(), KeychainError> {
    match resolved() {
        Resolved::Memory => memory_delete(account),
        Resolved::Os => os_impl::delete(account),
        Resolved::OsUnavailable => Err(backend_unavailable()),
    }
}

pub(crate) fn list_omw() -> Result<Vec<String>, KeychainError> {
    match resolved() {
        Resolved::Memory => memory_list_omw(),
        Resolved::Os => os_impl::list_omw(),
        Resolved::OsUnavailable => Err(backend_unavailable()),
    }
}

fn backend_unavailable() -> KeychainError {
    KeychainError::BackendUnavailable {
        reason: "OS keychain backend is unavailable on this platform".into(),
    }
}

// ---- in-memory backend ----

fn memory_store() -> &'static Mutex<HashMap<(String, String), String>> {
    static STORE: OnceLock<Mutex<HashMap<(String, String), String>>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn memory_get(account: &str) -> Result<String, KeychainError> {
    let store = memory_store().lock().expect("memory store mutex poisoned");
    store
        .get(&(SERVICE.to_string(), account.to_string()))
        .cloned()
        .ok_or(KeychainError::NotFound)
}

fn memory_set(account: &str, value: &str) -> Result<(), KeychainError> {
    let mut store = memory_store().lock().expect("memory store mutex poisoned");
    store.insert(
        (SERVICE.to_string(), account.to_string()),
        value.to_string(),
    );
    Ok(())
}

fn memory_delete(account: &str) -> Result<(), KeychainError> {
    let mut store = memory_store().lock().expect("memory store mutex poisoned");
    store
        .remove(&(SERVICE.to_string(), account.to_string()))
        .map(|_| ())
        .ok_or(KeychainError::NotFound)
}

fn memory_list_omw() -> Result<Vec<String>, KeychainError> {
    let store = memory_store().lock().expect("memory store mutex poisoned");
    Ok(store
        .keys()
        .filter(|(s, _)| s == SERVICE)
        .map(|(_, a)| a.clone())
        .collect())
}

// ---- OS backend ----

#[cfg(any(target_os = "macos", target_os = "linux"))]
mod os_impl {
    use super::{KeychainError, SERVICE};

    pub(super) fn get(account: &str) -> Result<String, KeychainError> {
        match keyring::Entry::new(SERVICE, account)
            .map_err(box_os)?
            .get_password()
        {
            Ok(pw) => Ok(pw),
            Err(keyring::Error::NoEntry) => Err(KeychainError::NotFound),
            Err(e) => Err(KeychainError::Os {
                source: Box::new(e),
            }),
        }
    }

    pub(super) fn set(account: &str, value: &str) -> Result<(), KeychainError> {
        keyring::Entry::new(SERVICE, account)
            .map_err(box_os)?
            .set_password(value)
            .map_err(box_os)
    }

    pub(super) fn delete(account: &str) -> Result<(), KeychainError> {
        match keyring::Entry::new(SERVICE, account)
            .map_err(box_os)?
            .delete_credential()
        {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Err(KeychainError::NotFound),
            Err(e) => Err(KeychainError::Os {
                source: Box::new(e),
            }),
        }
    }

    fn box_os(e: keyring::Error) -> KeychainError {
        match e {
            keyring::Error::NoEntry => KeychainError::NotFound,
            other => KeychainError::Os {
                source: Box::new(other),
            },
        }
    }

    #[cfg(target_os = "macos")]
    pub(super) fn list_omw() -> Result<Vec<String>, KeychainError> {
        use security_framework::item::{ItemClass, ItemSearchOptions, Limit, SearchResult};

        let mut search = ItemSearchOptions::new();
        search
            .class(ItemClass::generic_password())
            .service(SERVICE)
            .load_attributes(true)
            .limit(Limit::All);
        let results = match search.search() {
            Ok(r) => r,
            Err(e) if e.code() == -25300 /* errSecItemNotFound */ => return Ok(vec![]),
            Err(e) => return Err(KeychainError::Os { source: Box::new(e) }),
        };
        Ok(results
            .into_iter()
            .filter_map(|r| match r {
                SearchResult::Dict(_) => r.simplify_dict(),
                _ => None,
            })
            .filter_map(|d| d.get("acct").cloned())
            .collect())
    }

    #[cfg(target_os = "linux")]
    pub(super) fn list_omw() -> Result<Vec<String>, KeychainError> {
        Err(KeychainError::BackendUnavailable {
            reason: "listing Linux Secret Service entries is not supported by this backend".into(),
        })
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
mod os_impl {
    use super::KeychainError;

    pub(super) fn get(_: &str) -> Result<String, KeychainError> {
        Err(unavailable())
    }
    pub(super) fn set(_: &str, _: &str) -> Result<(), KeychainError> {
        Err(unavailable())
    }
    pub(super) fn delete(_: &str) -> Result<(), KeychainError> {
        Err(unavailable())
    }
    pub(super) fn list_omw() -> Result<Vec<String>, KeychainError> {
        Err(unavailable())
    }
    fn unavailable() -> KeychainError {
        KeychainError::BackendUnavailable {
            reason: "OS keychain not implemented on this platform".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_list_is_service_scoped() {
        let store = memory_store();
        // Plant entries under both "omw" and a foreign service.
        {
            let mut s = store.lock().unwrap();
            s.insert(
                ("omw".to_string(), "test/scoped-a".to_string()),
                "v1".to_string(),
            );
            s.insert(
                ("not-omw".to_string(), "test/scoped-b".to_string()),
                "v2".to_string(),
            );
        }
        let listed = memory_list_omw().unwrap();
        assert!(listed.iter().any(|n| n == "test/scoped-a"));
        assert!(!listed.iter().any(|n| n == "test/scoped-b"));
        // Cleanup so other tests in the same binary aren't affected.
        let mut s = store.lock().unwrap();
        s.remove(&("omw".to_string(), "test/scoped-a".to_string()));
        s.remove(&("not-omw".to_string(), "test/scoped-b".to_string()));
    }
}
