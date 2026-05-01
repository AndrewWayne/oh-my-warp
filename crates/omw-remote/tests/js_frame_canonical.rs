//! Cross-language proof for WS frame signing.
//!
//! Sister test to js_client_canonical.rs but for `ws::frame::Frame`.
//! Exercises the same failure mode (canonical-bytes mismatch) that hit
//! HTTP signed requests, applied to per-frame WS signing.

use bytes::Bytes;
use chrono::{DateTime, Utc};
use ed25519_dalek::{Signer as _, SigningKey};

use omw_remote::ws::Frame;

/// JS-style canonical bytes per `apps/web-controller/src/lib/pty-ws.ts::frameCanonicalBytes`:
/// the wire `ts` string is used VERBATIM in the canonical JSON, no
/// re-formatting. JS produces ts via `new Date().toISOString()` which always
/// uses the `Z` suffix.
fn js_canonical_bytes(
    v: u8,
    seq: u64,
    ts: &str,
    kind: &str,
    payload: &[u8],
) -> Vec<u8> {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine as _;
    let payload_b64 = URL_SAFE_NO_PAD.encode(payload);
    let s = format!(
        "{{\"kind\":{kind_json},\"payload\":{payload_json},\"seq\":{seq},\"ts\":{ts_json},\"v\":{v}}}",
        kind_json = serde_json::to_string(kind).unwrap(),
        payload_json = serde_json::to_string(&payload_b64).unwrap(),
        ts_json = serde_json::to_string(ts).unwrap(),
    );
    s.into_bytes()
}

#[test]
fn js_z_ts_canonical_matches_rust_byte_for_byte() {
    // JS sends ts="2026-05-02T01:00:00.123Z" (Z suffix from toISOString).
    // After the format_ts(SecondsFormat::Millis, true) fix, Rust's
    // canonical_bytes also produces the Z form. Bytes match exactly.
    let wire_ts = "2026-05-02T01:00:00.123Z";
    let dt: DateTime<Utc> = DateTime::parse_from_rfc3339(wire_ts)
        .unwrap()
        .with_timezone(&Utc);

    let js_bytes = js_canonical_bytes(1, 1, wire_ts, "input", b"hello");

    let frame = Frame {
        v: 1,
        seq: 1,
        ts: dt,
        kind: omw_remote::ws::FrameKind::Input,
        payload: Bytes::from_static(b"hello"),
        sig: [0u8; 64],
    };
    let rust_bytes = frame.canonical_bytes();

    assert_eq!(
        js_bytes,
        rust_bytes,
        "JS-shaped canonical bytes must match Rust Frame::canonical_bytes — drift here breaks WS frame auth"
    );
}

#[test]
fn js_signed_frame_verifies_via_rust_frame_verify() {
    // Sign the JS-shaped canonical bytes with a fresh device key.
    // Reconstruct the same canonical via Frame::canonical_bytes on the
    // server side. Verify with the device pubkey. Pass = both sides
    // agree on the canonical encoding.
    let mut device_seed = [0u8; 32];
    for (i, b) in device_seed.iter_mut().enumerate() {
        *b = (i as u8) ^ 0x33;
    }
    let device_signing = SigningKey::from_bytes(&device_seed);
    let device_pub: [u8; 32] = device_signing.verifying_key().to_bytes();

    let wire_ts = "2026-05-02T01:00:00.123Z";
    let dt: DateTime<Utc> = DateTime::parse_from_rfc3339(wire_ts)
        .unwrap()
        .with_timezone(&Utc);

    let js_bytes = js_canonical_bytes(1, 7, wire_ts, "input", b"abc");
    let sig: [u8; 64] = device_signing.sign(&js_bytes).to_bytes();

    let mut frame = Frame {
        v: 1,
        seq: 7,
        ts: dt,
        kind: omw_remote::ws::FrameKind::Input,
        payload: Bytes::from_static(b"abc"),
        sig: [0u8; 64],
    };
    frame.sig = sig;

    frame
        .verify(&device_pub)
        .expect("frame verify must accept a signature produced over JS-shaped canonical bytes");
}
