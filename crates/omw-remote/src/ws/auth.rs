//! Per-WS-session frame verification ladder. See spec §7.3.

use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use chrono::{DateTime, Utc};

use crate::capability::CapabilityToken;
use crate::revocations::RevocationList;
use crate::ws::frame::{Frame, FrameAuthError};

pub use crate::ws::frame::FrameAuthError as WsFrameAuthError;

/// Per-WS-session inbound auth state.
///
/// Each accepted handshake constructs one of these, holding the device's
/// capability token (already verified at handshake time) and an atomic
/// `last_inbound_seq` counter for §7.3 step 2.
pub struct WsSessionAuth {
    /// `last_seen_seq` for inbound frames. Incoming `frame.seq` MUST be
    /// strictly greater. Initialized to `u64::MAX` so the first accepted
    /// frame can be `seq = 0` (we treat MAX as "no frame yet"; any `seq`
    /// other than MAX clears the sentinel via the verify path).
    pub last_inbound_seq: AtomicU64,
    pub device_id: String,
    pub capability: CapabilityToken,
    pub revocations: Arc<RevocationList>,
    /// Allowed clock skew for frame ts vs. server `now`, in seconds. Spec
    /// default 30; tests may shrink this for determinism.
    pub ts_skew_seconds: i64,
}

impl WsSessionAuth {
    /// Run the §7.3 ladder against `frame`:
    /// 1. signature against `capability.device_pubkey`
    /// 2. `frame.seq > last_inbound_seq`
    /// 3. `|now − frame.ts| <= ts_skew_seconds`
    /// 4. capability not expired AND device not revoked
    /// 5. on success: bump `last_inbound_seq` to `frame.seq`
    pub fn verify_frame(
        &self,
        _frame: &Frame,
        _now: DateTime<Utc>,
    ) -> Result<(), FrameAuthError> {
        unimplemented!(
            "Phase E executor: per-frame ladder — sig, seq>last, ts skew, cap valid, not revoked"
        )
    }
}
