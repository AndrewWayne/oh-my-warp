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
        Arc::new(Self {
            revoked: Mutex::new(HashSet::new()),
        })
    }

    /// Mark `device_id` as revoked. Idempotent.
    pub fn revoke(&self, device_id: &str) {
        let mut g = self.revoked.lock().expect("revocations poisoned");
        g.insert(device_id.to_string());
    }

    /// True iff `device_id` has been revoked.
    pub fn is_revoked(&self, device_id: &str) -> bool {
        let g = self.revoked.lock().expect("revocations poisoned");
        g.contains(device_id)
    }
}
