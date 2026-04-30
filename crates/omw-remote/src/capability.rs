//! Capability tokens. See `specs/byorc-protocol.md` §5.

use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::host_key::HostKey;

/// Closed v1 scope vocabulary (spec §5.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Capability {
    #[serde(rename = "pty:read")]
    PtyRead,
    #[serde(rename = "pty:write")]
    PtyWrite,
    #[serde(rename = "agent:read")]
    AgentRead,
    #[serde(rename = "agent:write")]
    AgentWrite,
    #[serde(rename = "audit:read")]
    AuditRead,
    #[serde(rename = "pair:admin")]
    PairAdmin,
}

/// JSON capability token, signed by the host pairing key.
///
/// Wire shape (spec §5.1):
/// ```json
/// { "v":1, "device_id": "...", "device_pubkey":"...", "host_id":"...",
///   "capabilities":[...], "issued_at":"...", "expires_at":"...", "sig":"..." }
/// ```
#[derive(Clone, Debug)]
pub struct CapabilityToken {
    pub v: u8,
    pub device_id: String,
    pub device_pubkey: [u8; 32],
    pub host_id: String,
    pub capabilities: Vec<Capability>,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    /// Signature over the canonical JSON form with `sig` omitted.
    pub host_signature: [u8; 64],
}

impl CapabilityToken {
    /// Mint a new capability token: `expires_at = now + ttl`, signed with `host`.
    pub fn issue(
        _host: &HostKey,
        _device_pubkey: [u8; 32],
        _device_id: String,
        _capabilities: Vec<Capability>,
        _ttl: Duration,
    ) -> Self {
        unimplemented!("CapabilityToken::issue")
    }

    /// Verify the host signature and that `now <= expires_at`.
    pub fn verify(
        &self,
        _host_pubkey: &[u8; 32],
        _now: DateTime<Utc>,
    ) -> Result<(), CapabilityError> {
        unimplemented!("CapabilityToken::verify")
    }

    /// Does this token authorize `cap`?
    pub fn allows(&self, _cap: Capability) -> bool {
        unimplemented!("CapabilityToken::allows")
    }

    /// Encode to base64url(JSON) for wire transmission.
    pub fn to_base64url(&self) -> String {
        unimplemented!("CapabilityToken::to_base64url")
    }

    /// Decode from base64url(JSON).
    pub fn from_base64url(_s: &str) -> Result<Self, ParseError> {
        unimplemented!("CapabilityToken::from_base64url")
    }
}

#[derive(Debug, PartialEq, Eq, Error)]
pub enum CapabilityError {
    #[error("capability_invalid")]
    Invalid,
    #[error("capability_expired")]
    Expired,
}

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("malformed capability token")]
    Malformed,
}
