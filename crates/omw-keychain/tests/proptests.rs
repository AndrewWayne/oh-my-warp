//! Property-based round-trip tests against the in-memory backend.

mod common;

use common::{init_memory_backend, key_ref};
use omw_keychain::{self as kc, KeychainError};
use proptest::prelude::*;

fn arb_name() -> impl Strategy<Value = String> {
    "[A-Za-z0-9_/-]{1,64}".prop_map(|s| format!("omw/{s}"))
}

fn arb_value() -> impl Strategy<Value = String> {
    // \PC = any printable Unicode codepoint (excludes the Other category:
    // control/format/...). Binary-safe UTF-8 including NUL is separately
    // covered by crud.rs::binary_safe_utf8_secret_round_trips.
    "\\PC{1,4096}"
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, .. ProptestConfig::default() })]

    #[test]
    fn set_get_round_trip(name in arb_name(), value in arb_value()) {
        init_memory_backend();
        let kr = key_ref(&name);
        kc::set(&kr, &value)
            .map_err(|e| TestCaseError::fail(format!("set failed: {e:?}")))?;
        let got = kc::get(&kr)
            .map_err(|e| TestCaseError::fail(format!("get failed: {e:?}")))?;
        prop_assert_eq!(got.expose(), value.as_str());
        let _ = kc::delete(&kr);
    }

    #[test]
    fn set_delete_set_again(name in arb_name(), v1 in arb_value(), v2 in arb_value()) {
        init_memory_backend();
        let kr = key_ref(&name);
        kc::set(&kr, &v1)
            .map_err(|e| TestCaseError::fail(format!("set#1 failed: {e:?}")))?;
        kc::delete(&kr)
            .map_err(|e| TestCaseError::fail(format!("delete failed: {e:?}")))?;
        match kc::get(&kr) {
            Err(KeychainError::NotFound) => {}
            other => prop_assert!(false, "expected NotFound after delete, got {:?}", other),
        }
        kc::set(&kr, &v2)
            .map_err(|e| TestCaseError::fail(format!("set#2 failed: {e:?}")))?;
        let got = kc::get(&kr)
            .map_err(|e| TestCaseError::fail(format!("get#2 failed: {e:?}")))?;
        prop_assert_eq!(got.expose(), v2.as_str());
        let _ = kc::delete(&kr);
    }
}
