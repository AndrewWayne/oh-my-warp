//! Request signing + verification. See `specs/byorc-protocol.md` §4.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use thiserror::Error;

use crate::capability::Capability;
use crate::replay::NonceStore;

/// 16-hex-char device id (first 16 hex chars of SHA-256(device_pubkey)).
pub type DeviceId = String;

/// Canonical request the device signs. Per-spec layout in `to_bytes`.
#[derive(Clone, Debug)]
pub struct CanonicalRequest {
    pub method: String,
    pub path: String,
    pub query: String,
    pub ts: String,
    pub nonce: String,
    pub body_sha256: [u8; 32],
    pub device_id: String,
    pub protocol_version: u8,
}

impl CanonicalRequest {
    /// Render the bytes the device signs, per spec §4.1:
    ///
    /// ```text
    /// METHOD\nPATH\nQUERY\nTS\nNONCE\nhex(SHA256(BODY))\nDEVICE_ID\nVERSION\n
    /// ```
    pub fn to_bytes(&self) -> Vec<u8> {
        unimplemented!("CanonicalRequest::to_bytes")
    }
}

/// Device-side signer.
pub struct Signer<'a> {
    pub device_priv: &'a [u8; 32],
}

impl<'a> Signer<'a> {
    /// Sign a canonical request, returning the 64-byte Ed25519 signature.
    pub fn sign(&self, _req: &CanonicalRequest) -> [u8; 64] {
        unimplemented!("Signer::sign")
    }
}

/// Server-side verifier. Runs the §4.2 ordered checks.
pub struct Verifier {
    pub host_pubkey: [u8; 32],
    pub nonce_store: Arc<NonceStore>,
    pub ts_skew_seconds: i64,
}

impl Verifier {
    /// New verifier with the spec-default 30-second skew window.
    pub fn new(_host_pubkey: [u8; 32], _nonce_store: Arc<NonceStore>) -> Self {
        unimplemented!("Verifier::new")
    }

    /// Run the full §4.2 check ladder. Returns the device id on success.
    pub fn verify(
        &self,
        _req: &CanonicalRequest,
        _signature: &[u8; 64],
        _cap_token_b64: &str,
        _required_cap: Capability,
        _now: DateTime<Utc>,
    ) -> Result<DeviceId, AuthError> {
        unimplemented!("Verifier::verify")
    }
}

/// Authentication outcome. Maps 1:1 onto spec §11.1 error codes.
#[derive(Debug, PartialEq, Eq, Error)]
pub enum AuthError {
    #[error("capability_invalid")]
    CapabilityInvalid,
    #[error("capability_expired")]
    CapabilityExpired,
    #[error("device_revoked")]
    DeviceRevoked,
    #[error("ts_skew")]
    TsSkew,
    #[error("nonce_replayed")]
    NonceReplayed,
    #[error("signature_invalid")]
    SignatureInvalid,
    #[error("capability_scope")]
    CapabilityScope,
    #[error("invalid_body")]
    InvalidBody,
}
