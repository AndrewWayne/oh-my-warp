//! Explicit `OMW_KEYCHAIN_BACKEND=memory` selection.

mod common;

use common::{init_memory_backend, key_ref, unique_name};
use omw_keychain as kc;

#[test]
fn set_and_get_round_trip_with_memory_backend() {
    init_memory_backend();
    let kr = key_ref(&unique_name("sel"));
    kc::set(&kr, "round-trip").unwrap();
    assert_eq!(kc::get(&kr).unwrap().expose(), "round-trip");
    let _ = kc::delete(&kr);
}

// Note: this test only proves that a name we never wrote is absent. Full
// process-freshness ("the backend always starts with an empty store") would
// require spawning a subprocess so the OnceLock initialises from scratch;
// that's deferred to v0.2's manual integration suite.
#[test]
fn memory_backend_starts_empty_in_a_fresh_process() {
    init_memory_backend();
    let kr = key_ref(&unique_name("never-set"));
    match kc::get(&kr) {
        Err(kc::KeychainError::NotFound) => {}
        other => panic!(
            "expected NotFound for never-written name, got {:?}",
            other.map(|s| s.expose().to_string())
        ),
    }
}
