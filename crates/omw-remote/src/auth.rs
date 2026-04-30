//! Request signing + verification. See `specs/byorc-protocol.md` §4.

use std::sync::Arc;
use std::time::Instant;

use chrono::{DateTime, Utc};
use ed25519_dalek::{Signature, Signer as _, SigningKey, Verifier as _, VerifyingKey};
use thiserror::Error;

use crate::capability::{Capability, CapabilityError, CapabilityToken};
use crate::replay::{NonceError, NonceStore};

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

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0x0F) as usize] as char);
    }
    s
}

impl CanonicalRequest {
    /// Render the bytes the device signs, per spec §4.1:
    ///
    /// ```text
    /// METHOD\nPATH\nQUERY\nTS\nNONCE\nhex(SHA256(BODY))\nDEVICE_ID\nVERSION\n
    /// ```
    pub fn to_bytes(&self) -> Vec<u8> {
        let body_hash_hex = hex_lower(&self.body_sha256);
        let s = format!(
            "{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n",
            self.method,
            self.path,
            self.query,
            self.ts,
            self.nonce,
            body_hash_hex,
            self.device_id,
            self.protocol_version,
        );
        s.into_bytes()
    }
}

/// Device-side signer.
pub struct Signer<'a> {
    pub device_priv: &'a [u8; 32],
}

impl<'a> Signer<'a> {
    /// Sign a canonical request, returning the 64-byte Ed25519 signature.
    pub fn sign(&self, req: &CanonicalRequest) -> [u8; 64] {
        let signing = SigningKey::from_bytes(self.device_priv);
        let sig = signing.sign(&req.to_bytes());
        sig.to_bytes()
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
    pub fn new(host_pubkey: [u8; 32], nonce_store: Arc<NonceStore>) -> Self {
        Self {
            host_pubkey,
            nonce_store,
            ts_skew_seconds: 30,
        }
    }

    /// Run the full §4.2 check ladder. Returns the device id on success.
    pub fn verify(
        &self,
        req: &CanonicalRequest,
        signature: &[u8; 64],
        cap_token_b64: &str,
        required_cap: Capability,
        now: DateTime<Utc>,
    ) -> Result<DeviceId, AuthError> {
        // 1. Parse + verify capability token.
        let cap_token = CapabilityToken::from_base64url(cap_token_b64)
            .map_err(|_| AuthError::CapabilityInvalid)?;
        cap_token
            .verify(&self.host_pubkey, now)
            .map_err(|e| match e {
                CapabilityError::Invalid => AuthError::CapabilityInvalid,
                CapabilityError::Expired => AuthError::CapabilityExpired,
            })?;

        // 3. Revocation list — Phase D has none, skip.

        // 4. Timestamp skew.
        let req_ts = DateTime::parse_from_rfc3339(&req.ts)
            .map_err(|_| AuthError::TsSkew)?
            .with_timezone(&Utc);
        let skew = (now - req_ts).num_seconds().abs();
        if skew > self.ts_skew_seconds {
            return Err(AuthError::TsSkew);
        }

        // 5+6+7. Verify Ed25519 signature with cap_token.device_pubkey.
        let device_vk = VerifyingKey::from_bytes(&cap_token.device_pubkey)
            .map_err(|_| AuthError::SignatureInvalid)?;
        let sig = Signature::from_bytes(signature);
        if device_vk.verify(&req.to_bytes(), &sig).is_err() {
            return Err(AuthError::SignatureInvalid);
        }

        // 8. Capability scope.
        if !cap_token.allows(required_cap) {
            return Err(AuthError::CapabilityScope);
        }

        // 5/9. Nonce check + insert (only on the success path, per spec).
        match self
            .nonce_store
            .check_and_insert(&req.nonce, Instant::now())
        {
            Ok(()) => {}
            Err(NonceError::Replayed) => return Err(AuthError::NonceReplayed),
        }

        Ok(cap_token.device_id)
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
