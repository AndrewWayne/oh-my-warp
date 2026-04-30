//! Per-WS-session frame verification ladder. See spec §7.3.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use chrono::{DateTime, Utc};

use crate::capability::{CapabilityError, CapabilityToken};
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
    /// Host pubkey, needed to re-verify the capability token on every frame
    /// (§7.3 step 4 — checked at frame granularity, not handshake granularity).
    pub host_pubkey: [u8; 32],
}

impl WsSessionAuth {
    /// Run the §7.3 ladder against `frame`:
    /// 1. signature against `capability.device_pubkey`
    /// 2. `frame.seq > last_inbound_seq`
    /// 3. `|now − frame.ts| <= ts_skew_seconds`
    /// 4. capability not expired AND device not revoked
    /// 5. on success: bump `last_inbound_seq` to `frame.seq`
    pub fn verify_frame(&self, frame: &Frame, now: DateTime<Utc>) -> Result<(), FrameAuthError> {
        // 1. Signature against device key.
        frame.verify(&self.capability.device_pubkey)?;

        // 2. Strict seq monotonicity. Sentinel u64::MAX means "no frame yet";
        //    any other value requires `frame.seq > last`.
        let last = self.last_inbound_seq.load(Ordering::Relaxed);
        if last != u64::MAX && frame.seq <= last {
            return Err(FrameAuthError::SeqRegression);
        }

        // 3. Timestamp skew.
        let skew = (now - frame.ts).num_seconds().abs();
        if skew > self.ts_skew_seconds {
            return Err(FrameAuthError::TsSkew);
        }

        // 4. Capability still valid + device not revoked.
        self.capability
            .verify(&self.host_pubkey, now)
            .map_err(|e| match e {
                CapabilityError::Expired => FrameAuthError::CapabilityExpired,
                CapabilityError::Invalid => FrameAuthError::CapabilityExpired,
            })?;
        if self.revocations.is_revoked(&self.device_id) {
            return Err(FrameAuthError::DeviceRevoked);
        }

        // 5. Bump high-water mark.
        self.last_inbound_seq.store(frame.seq, Ordering::Relaxed);
        Ok(())
    }
}
