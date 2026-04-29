//! `list_omw` behaviour tests.
//!
//! Service-scoping (`SERVICE = "omw"` filter) is not directly observable
//! through the public v0.1 API on the in-memory backend: a flat global map
//! that ignored the service field would still pass every test we can write
//! without poking at internals. The macOS path enforces scoping via the
//! `service(SERVICE)` filter passed to Security.framework; the in-memory
//! backend's adherence is covered by the in-crate
//! `backend::tests::memory_list_is_service_scoped` unit test, which has
//! direct access to the underlying store. v0.2 adds a manual macOS
//! integration test (gated behind a developer-only env var) for external
//! coverage.

mod common;

use common::{init_memory_backend, key_ref, unique_name};
use omw_keychain as kc;

#[test]
fn list_omw_includes_entries_we_set() {
    init_memory_backend();
    let name = unique_name("list-incl");
    let kr = key_ref(&name);
    kc::set(&kr, "v").unwrap();
    let listed = kc::list_omw().unwrap();
    assert!(
        listed.iter().any(|n| n == &name),
        "expected {name} to appear in {listed:?}"
    );
    let _ = kc::delete(&kr);
}

#[test]
fn list_omw_omits_deleted_entries() {
    init_memory_backend();
    let name = unique_name("list-omit");
    let kr = key_ref(&name);
    kc::set(&kr, "v").unwrap();
    kc::delete(&kr).unwrap();
    let listed = kc::list_omw().unwrap();
    assert!(
        !listed.iter().any(|n| n == &name),
        "expected {name} to be absent after delete; saw {listed:?}"
    );
}

#[test]
fn list_omw_returns_account_names_not_full_uris() {
    init_memory_backend();
    let name = unique_name("list-bare");
    let kr = key_ref(&name);
    kc::set(&kr, "v").unwrap();
    let listed = kc::list_omw().unwrap();
    for entry in &listed {
        assert!(
            !entry.starts_with("keychain:"),
            "list_omw should return bare account names, got {entry:?}"
        );
    }
    let _ = kc::delete(&kr);
}
