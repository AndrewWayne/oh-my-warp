//! WS frame envelope + per-frame signing/verification. See spec §7.2 + §7.3.

use bytes::Bytes;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::host_key::HostKey;
use crate::Signer;

/// JSON-encoded WS envelope.
///
/// Wire shape (spec §7.2):
/// ```json
/// { "v":1, "seq":42, "ts":"...", "kind":"input"|"output"|"control"|"ping"|"pong",
///   "payload": { ... }, "sig":"<base64url(64)>" }
/// ```
///
/// `canonical_bytes` returns the serialized form WITH `sig` omitted, sorted
/// keys, no whitespace — what both signer and verifier feed into Ed25519.
#[derive(Clone, Debug)]
pub struct Frame {
    pub v: u8,
    pub seq: u64,
    pub ts: DateTime<Utc>,
    pub kind: FrameKind,
    /// Opaque payload bytes. For `input`/`output` this is raw PTY bytes;
    /// for `ping`/`pong` an arbitrary nonce; for `control` a JSON blob.
    pub payload: Bytes,
    /// Ed25519 signature over `canonical_bytes`. All-zero on a freshly
    /// constructed frame; populated by `sign` or `from_json`.
    pub sig: [u8; 64],
}

/// Frame kind. Wire-encoded as the lowercase string literal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FrameKind {
    #[serde(rename = "input")]
    Input,
    #[serde(rename = "output")]
    Output,
    #[serde(rename = "control")]
    Control,
    #[serde(rename = "ping")]
    Ping,
    #[serde(rename = "pong")]
    Pong,
}

impl Frame {
    /// Render the deterministic canonical form (sig omitted, sorted keys, no
    /// whitespace) the signer signs and the verifier re-derives.
    ///
    /// Sorted lex order: kind, payload, seq, ts, v.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        unimplemented!("Phase E executor: emit canonical JSON with sig omitted, sorted keys, no whitespace")
    }

    /// Sign this frame with the device key (client→server) by computing
    /// `sig = Ed25519(signer, self.canonical_bytes())`.
    pub fn sign(&mut self, _signer: &Signer) {
        unimplemented!("Phase E executor: sign canonical_bytes() and store into self.sig")
    }

    /// Sign this frame with the host pairing key (server→client output frames).
    pub fn sign_with_host(&mut self, _host: &HostKey) {
        unimplemented!("Phase E executor: sign canonical_bytes() with host key")
    }

    /// Verify `self.sig` against `pubkey` (32-byte Ed25519 verifying key).
    /// Used both for inbound device-signed frames (device pubkey) and
    /// outbound server-signed frames (host pubkey).
    pub fn verify(&self, _pubkey: &[u8; 32]) -> Result<(), FrameAuthError> {
        unimplemented!("Phase E executor: Ed25519 verify of canonical_bytes against pubkey")
    }

    /// Encode as JSON for the wire (sig included, base64url).
    pub fn to_json(&self) -> String {
        unimplemented!("Phase E executor: serialize full envelope including base64url(sig)")
    }

    /// Parse a JSON frame off the wire. Decodes base64url payload + sig.
    pub fn from_json(_s: &str) -> Result<Self, FrameError> {
        unimplemented!("Phase E executor: parse JSON envelope, decode payload + sig")
    }
}

/// Frame authentication error. Re-exported via `ws::FrameAuthError`.
///
/// Defined here (not in `auth.rs`) so `Frame::verify` doesn't pull a circular
/// dependency on the session-tracking module.
#[derive(Debug, PartialEq, Eq, Error)]
pub enum FrameAuthError {
    #[error("signature_invalid")]
    SignatureInvalid,
    #[error("seq_regression")]
    SeqRegression,
    #[error("ts_skew")]
    TsSkew,
    #[error("capability_expired")]
    CapabilityExpired,
    #[error("device_revoked")]
    DeviceRevoked,
}

/// Frame decode error.
#[derive(Debug, PartialEq, Eq, Error)]
pub enum FrameError {
    #[error("invalid_json")]
    InvalidJson,
    #[error("malformed_frame")]
    Malformed,
    #[error("unsupported_version")]
    UnsupportedVersion,
}
