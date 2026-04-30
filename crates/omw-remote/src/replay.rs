//! Nonce store for replay defense. See `specs/byorc-protocol.md` §6.

use std::sync::Arc;
use std::time::{Duration, Instant};

use thiserror::Error;

/// In-memory nonce dedup store with a sliding window.
pub struct NonceStore {
    // Mutex<HashMap<String, Instant>> + window
}

impl NonceStore {
    /// Construct a store whose retention window is `window` (spec default: 60 s,
    /// twice the 30 s timestamp skew window).
    pub fn new(_window: Duration) -> Arc<Self> {
        unimplemented!("NonceStore::new")
    }

    /// Insert `nonce` if unseen; otherwise reject as a replay.
    pub fn check_and_insert(&self, _nonce: &str, _now: Instant) -> Result<(), NonceError> {
        unimplemented!("NonceStore::check_and_insert")
    }

    /// Drop entries older than `now - window`.
    pub fn purge_expired(&self, _now: Instant) {
        unimplemented!("NonceStore::purge_expired")
    }
}

#[derive(Debug, PartialEq, Eq, Error)]
pub enum NonceError {
    #[error("nonce_replayed")]
    Replayed,
}
