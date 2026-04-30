//! Capability tokens. See `specs/byorc-protocol.md` §5.

use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use chrono::{DateTime, Utc};
use ed25519_dalek::{Signature, Verifier as _, VerifyingKey};
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

impl Capability {
    fn as_scope(&self) -> &'static str {
        match self {
            Capability::PtyRead => "pty:read",
            Capability::PtyWrite => "pty:write",
            Capability::AgentRead => "agent:read",
            Capability::AgentWrite => "agent:write",
            Capability::AuditRead => "audit:read",
            Capability::PairAdmin => "pair:admin",
        }
    }
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

/// Wire-format JSON object, fixed key order: v, device_id, device_pubkey,
/// host_id, capabilities, issued_at, expires_at, sig.
#[derive(Serialize, Deserialize)]
struct WireToken {
    v: u8,
    device_id: String,
    device_pubkey: String,
    host_id: String,
    capabilities: Vec<String>,
    issued_at: String,
    expires_at: String,
    sig: String,
}

fn b64url_encode(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

fn b64url_decode(s: &str) -> Result<Vec<u8>, ParseError> {
    URL_SAFE_NO_PAD.decode(s).map_err(|_| ParseError::Malformed)
}

/// Serialize the token's signed-payload form (sig omitted) with deterministic
/// field order matching the spec's "sorted keys" requirement.
///
/// Sorted lex order of {capabilities, device_id, device_pubkey, expires_at,
/// host_id, issued_at, v} is:
///   capabilities, device_id, device_pubkey, expires_at, host_id, issued_at, v
fn canonical_signed_bytes(token: &CapabilityToken) -> Vec<u8> {
    let caps: Vec<&str> = token.capabilities.iter().map(|c| c.as_scope()).collect();
    let caps_json = serde_json::to_string(&caps).expect("vec of strs serializes");
    let device_id_json = serde_json::to_string(&token.device_id).expect("string");
    let device_pubkey_b64 = b64url_encode(&token.device_pubkey);
    let device_pubkey_json = serde_json::to_string(&device_pubkey_b64).expect("string");
    let expires_at_json = serde_json::to_string(&token.expires_at.to_rfc3339()).expect("string");
    let host_id_json = serde_json::to_string(&token.host_id).expect("string");
    let issued_at_json = serde_json::to_string(&token.issued_at.to_rfc3339()).expect("string");
    let v_json = token.v.to_string();

    let s = format!(
        "{{\"capabilities\":{caps_json},\"device_id\":{device_id_json},\
         \"device_pubkey\":{device_pubkey_json},\"expires_at\":{expires_at_json},\
         \"host_id\":{host_id_json},\"issued_at\":{issued_at_json},\"v\":{v_json}}}"
    );
    s.into_bytes()
}

impl CapabilityToken {
    /// Mint a new capability token: `expires_at = now + ttl`, signed with `host`.
    pub fn issue(
        host: &HostKey,
        device_pubkey: [u8; 32],
        device_id: String,
        capabilities: Vec<Capability>,
        ttl: Duration,
    ) -> Self {
        let issued_at = Utc::now();
        let expires_at =
            issued_at + chrono::Duration::from_std(ttl).unwrap_or(chrono::Duration::seconds(0));
        let mut token = Self {
            v: 1,
            device_id,
            device_pubkey,
            host_id: String::new(),
            capabilities,
            issued_at,
            expires_at,
            host_signature: [0u8; 64],
        };
        let bytes = canonical_signed_bytes(&token);
        token.host_signature = host.sign(&bytes);
        token
    }

    /// Verify the host signature and that `now <= expires_at`.
    pub fn verify(
        &self,
        host_pubkey: &[u8; 32],
        now: DateTime<Utc>,
    ) -> Result<(), CapabilityError> {
        let vk = VerifyingKey::from_bytes(host_pubkey).map_err(|_| CapabilityError::Invalid)?;
        let sig = Signature::from_bytes(&self.host_signature);
        let bytes = canonical_signed_bytes(self);
        if vk.verify(&bytes, &sig).is_err() {
            return Err(CapabilityError::Invalid);
        }
        if now >= self.expires_at {
            return Err(CapabilityError::Expired);
        }
        Ok(())
    }

    /// Does this token authorize `cap`?
    pub fn allows(&self, cap: Capability) -> bool {
        self.capabilities.contains(&cap)
    }

    /// Encode to base64url(JSON) for wire transmission.
    pub fn to_base64url(&self) -> String {
        let wire = WireToken {
            v: self.v,
            device_id: self.device_id.clone(),
            device_pubkey: b64url_encode(&self.device_pubkey),
            host_id: self.host_id.clone(),
            capabilities: self
                .capabilities
                .iter()
                .map(|c| c.as_scope().to_string())
                .collect(),
            issued_at: self.issued_at.to_rfc3339(),
            expires_at: self.expires_at.to_rfc3339(),
            sig: b64url_encode(&self.host_signature),
        };
        let json = serde_json::to_vec(&wire).expect("wire token serializes");
        b64url_encode(&json)
    }

    /// Decode from base64url(JSON).
    pub fn from_base64url(s: &str) -> Result<Self, ParseError> {
        let json = b64url_decode(s)?;
        let wire: WireToken = serde_json::from_slice(&json).map_err(|_| ParseError::Malformed)?;
        let device_pubkey_bytes = b64url_decode(&wire.device_pubkey)?;
        if device_pubkey_bytes.len() != 32 {
            return Err(ParseError::Malformed);
        }
        let mut device_pubkey = [0u8; 32];
        device_pubkey.copy_from_slice(&device_pubkey_bytes);

        let sig_bytes = b64url_decode(&wire.sig)?;
        if sig_bytes.len() != 64 {
            return Err(ParseError::Malformed);
        }
        let mut host_signature = [0u8; 64];
        host_signature.copy_from_slice(&sig_bytes);

        let issued_at = DateTime::parse_from_rfc3339(&wire.issued_at)
            .map_err(|_| ParseError::Malformed)?
            .with_timezone(&Utc);
        let expires_at = DateTime::parse_from_rfc3339(&wire.expires_at)
            .map_err(|_| ParseError::Malformed)?
            .with_timezone(&Utc);

        let mut capabilities = Vec::with_capacity(wire.capabilities.len());
        for s in &wire.capabilities {
            let cap = match s.as_str() {
                "pty:read" => Capability::PtyRead,
                "pty:write" => Capability::PtyWrite,
                "agent:read" => Capability::AgentRead,
                "agent:write" => Capability::AgentWrite,
                "audit:read" => Capability::AuditRead,
                "pair:admin" => Capability::PairAdmin,
                _ => return Err(ParseError::Malformed),
            };
            capabilities.push(cap);
        }

        Ok(Self {
            v: wire.v,
            device_id: wire.device_id,
            device_pubkey,
            host_id: wire.host_id,
            capabilities,
            issued_at,
            expires_at,
            host_signature,
        })
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
