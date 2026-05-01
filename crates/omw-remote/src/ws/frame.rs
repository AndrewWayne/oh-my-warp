//! WS frame envelope + per-frame signing/verification. See spec §7.2 + §7.3.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use bytes::Bytes;
use chrono::{DateTime, SecondsFormat, Utc};
use ed25519_dalek::{Signature, Signer as _, SigningKey, Verifier as _, VerifyingKey};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::host_key::HostKey;
use crate::Signer;

/// Format a timestamp the same way both the wire and the canonical-bytes
/// signer/verifier consume it. We use millisecond precision + `Z` suffix
/// (matches JS `Date.prototype.toISOString` byte-for-byte) so a JS client
/// that signs canonical bytes built from the wire `ts` string verifies
/// against [`Frame::canonical_bytes`] without a one-byte format drift.
///
/// Default `chrono::DateTime<Utc>::to_rfc3339()` uses `+00:00` instead of
/// `Z`, which would not match JS-side canonical bytes — the cause of an
/// observed `4401 auth_failed` close on every input frame from the phone.
fn format_ts(ts: &DateTime<Utc>) -> String {
    ts.to_rfc3339_opts(SecondsFormat::Millis, true)
}

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

impl FrameKind {
    fn as_wire(&self) -> &'static str {
        match self {
            FrameKind::Input => "input",
            FrameKind::Output => "output",
            FrameKind::Control => "control",
            FrameKind::Ping => "ping",
            FrameKind::Pong => "pong",
        }
    }

    fn from_wire(s: &str) -> Option<Self> {
        match s {
            "input" => Some(FrameKind::Input),
            "output" => Some(FrameKind::Output),
            "control" => Some(FrameKind::Control),
            "ping" => Some(FrameKind::Ping),
            "pong" => Some(FrameKind::Pong),
            _ => None,
        }
    }
}

/// Wire JSON envelope including `sig`. Used by `to_json` / `from_json`.
#[derive(Serialize, Deserialize)]
struct WireFrame {
    kind: String,
    payload: String,
    seq: u64,
    sig: String,
    ts: String,
    v: u8,
}

impl Frame {
    /// Render the deterministic canonical form (sig omitted, sorted keys, no
    /// whitespace) the signer signs and the verifier re-derives.
    ///
    /// Sorted lex order: kind, payload, seq, ts, v.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let kind_json = serde_json::to_string(self.kind.as_wire()).expect("string");
        let payload_b64 = URL_SAFE_NO_PAD.encode(&self.payload);
        let payload_json = serde_json::to_string(&payload_b64).expect("string");
        let ts_json = serde_json::to_string(&format_ts(&self.ts)).expect("string");
        let s = format!(
            "{{\"kind\":{kind_json},\"payload\":{payload_json},\"seq\":{seq},\"ts\":{ts_json},\"v\":{v}}}",
            seq = self.seq,
            v = self.v,
        );
        s.into_bytes()
    }

    /// Sign this frame with the device key (client→server) by computing
    /// `sig = Ed25519(signer, self.canonical_bytes())`.
    pub fn sign(&mut self, signer: &Signer) {
        let signing = SigningKey::from_bytes(signer.device_priv);
        let bytes = self.canonical_bytes();
        let sig = signing.sign(&bytes);
        self.sig = sig.to_bytes();
    }

    /// Sign this frame with the host pairing key (server→client output frames).
    pub fn sign_with_host(&mut self, host: &HostKey) {
        let bytes = self.canonical_bytes();
        self.sig = host.sign(&bytes);
    }

    /// Verify `self.sig` against `pubkey` (32-byte Ed25519 verifying key).
    /// Used both for inbound device-signed frames (device pubkey) and
    /// outbound server-signed frames (host pubkey).
    pub fn verify(&self, pubkey: &[u8; 32]) -> Result<(), FrameAuthError> {
        let vk = VerifyingKey::from_bytes(pubkey).map_err(|_| FrameAuthError::SignatureInvalid)?;
        let sig = Signature::from_bytes(&self.sig);
        let bytes = self.canonical_bytes();
        vk.verify(&bytes, &sig)
            .map_err(|_| FrameAuthError::SignatureInvalid)
    }

    /// Encode as JSON for the wire (sig included, base64url).
    pub fn to_json(&self) -> String {
        // Wire `ts` MUST use the same format `canonical_bytes` does, otherwise
        // a server-signed output frame's signature won't verify on the JS
        // client (which canonicalizes using the wire `ts` string verbatim).
        let wire = WireFrame {
            kind: self.kind.as_wire().to_string(),
            payload: URL_SAFE_NO_PAD.encode(&self.payload),
            seq: self.seq,
            sig: URL_SAFE_NO_PAD.encode(self.sig),
            ts: format_ts(&self.ts),
            v: self.v,
        };
        serde_json::to_string(&wire).expect("wire frame serializes")
    }

    /// Parse a JSON frame off the wire. Decodes base64url payload + sig.
    pub fn from_json(s: &str) -> Result<Self, FrameError> {
        let wire: WireFrame = serde_json::from_str(s).map_err(|_| FrameError::InvalidJson)?;
        if wire.v != 1 {
            return Err(FrameError::UnsupportedVersion);
        }
        let kind = FrameKind::from_wire(&wire.kind).ok_or(FrameError::Malformed)?;
        let payload_bytes = URL_SAFE_NO_PAD
            .decode(&wire.payload)
            .map_err(|_| FrameError::Malformed)?;
        let sig_bytes = URL_SAFE_NO_PAD
            .decode(&wire.sig)
            .map_err(|_| FrameError::Malformed)?;
        if sig_bytes.len() != 64 {
            return Err(FrameError::Malformed);
        }
        let mut sig = [0u8; 64];
        sig.copy_from_slice(&sig_bytes);
        let ts = DateTime::parse_from_rfc3339(&wire.ts)
            .map_err(|_| FrameError::Malformed)?
            .with_timezone(&Utc);
        Ok(Self {
            v: wire.v,
            seq: wire.seq,
            ts,
            kind,
            payload: Bytes::from(payload_bytes),
            sig,
        })
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
