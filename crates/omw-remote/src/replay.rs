//! Nonce store for replay defense. See `specs/byorc-protocol.md` §6.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use thiserror::Error;

/// In-memory nonce dedup store with a sliding window.
pub struct NonceStore {
    seen: Mutex<HashMap<String, Instant>>,
    window: Duration,
}

impl NonceStore {
    /// Construct a store whose retention window is `window` (spec default: 60 s,
    /// twice the 30 s timestamp skew window).
    pub fn new(window: Duration) -> Arc<Self> {
        Arc::new(Self {
            seen: Mutex::new(HashMap::new()),
            window,
        })
    }

    /// Insert `nonce` if unseen; otherwise reject as a replay.
    pub fn check_and_insert(&self, nonce: &str, now: Instant) -> Result<(), NonceError> {
        let mut seen = self.seen.lock().expect("nonce store poisoned");
        if let Some(prev) = seen.get(nonce) {
            if now.duration_since(*prev) < self.window * 2 {
                return Err(NonceError::Replayed);
            }
        }
        seen.insert(nonce.to_string(), now);
        Ok(())
    }

    /// Drop entries older than `now - window`.
    pub fn purge_expired(&self, now: Instant) {
        let mut seen = self.seen.lock().expect("nonce store poisoned");
        let window = self.window * 2;
        seen.retain(|_, t| now.duration_since(*t) < window);
    }
}

#[derive(Debug, PartialEq, Eq, Error)]
pub enum NonceError {
    #[error("nonce_replayed")]
    Replayed,
}
