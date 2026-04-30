//! In-memory device revocation list. See `specs/byorc-protocol.md` §9.
//!
//! Phase E scaffold: the persistent source of truth lives in
//! `devices.revoked_at` (SQLite). This in-memory mirror exists so frame
//! verification (§7.3 step 4) can check revocation in O(1) without hitting
//! the DB on every WS frame.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

/// Set of currently revoked `device_id`s.
pub struct RevocationList {
    revoked: Mutex<HashSet<String>>,
}

impl RevocationList {
    /// Create an empty revocation list.
    pub fn new() -> Arc<Self> {
        unimplemented!("Phase E executor: construct an empty RevocationList")
    }

    /// Mark `device_id` as revoked. Idempotent.
    pub fn revoke(&self, _device_id: &str) {
        unimplemented!("Phase E executor: insert device_id into revoked set")
    }

    /// True iff `device_id` has been revoked.
    pub fn is_revoked(&self, _device_id: &str) -> bool {
        unimplemented!("Phase E executor: lookup device_id in revoked set")
    }
}

// Held to keep the mutex field referenced even before Phase E impl lands —
// avoids a "field never read" warning during the scaffolding window.
#[allow(dead_code)]
fn _force_field_use(list: &RevocationList) -> usize {
    list.revoked.lock().expect("revocations poisoned").len()
}
