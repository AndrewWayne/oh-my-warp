//! Phase E — WS frame envelope (`specs/byorc-protocol.md` §7.2).
//!
//! Pins:
//! - canonical_bytes is sorted-key, no whitespace, sig omitted;
//! - sign + verify roundtrip with a device key;
//! - any one-byte tamper of seq / payload / kind invalidates the signature;
//! - malformed JSON yields a decode error, not a panic.

use bytes::Bytes;
use chrono::{TimeZone, Utc};
use ed25519_dalek::SigningKey;
use omw_remote::{Frame, FrameAuthError, FrameError, FrameKind, Signer};

/// Deterministic device keypair so the canonical-bytes test is reproducible.
fn make_device_key() -> SigningKey {
    SigningKey::from_bytes(&[7u8; 32])
}

/// A non-stupid time so the JSON layout is stable across runs.
fn fixed_ts() -> chrono::DateTime<chrono::Utc> {
    Utc.with_ymd_and_hms(2026, 4, 30, 12, 0, 0).unwrap()
}

fn input_frame() -> Frame {
    Frame {
        v: 1,
        seq: 17,
        ts: fixed_ts(),
        kind: FrameKind::Input,
        payload: Bytes::from_static(b"hello"),
        sig: [0u8; 64],
    }
}

#[test]
fn frame_canonical_bytes_sorted_keys() {
    // Sorted lex order of {kind, payload, seq, ts, v}. `sig` MUST be omitted.
    let frame = input_frame();
    let bytes = frame.canonical_bytes();
    let s = std::str::from_utf8(&bytes).expect("canonical bytes are UTF-8");

    // No whitespace anywhere.
    assert!(
        !s.contains(' ') && !s.contains('\n') && !s.contains('\t'),
        "canonical form must have no whitespace, got: {s:?}"
    );

    // sig must be omitted from the canonical form.
    assert!(
        !s.contains("\"sig\""),
        "canonical form must omit sig, got: {s:?}"
    );

    // The four mandatory keys appear in lex-sorted order: kind, payload, seq, ts, v.
    let kind_pos = s.find("\"kind\"").expect("kind present");
    let payload_pos = s.find("\"payload\"").expect("payload present");
    let seq_pos = s.find("\"seq\"").expect("seq present");
    let ts_pos = s.find("\"ts\"").expect("ts present");
    let v_pos = s.find("\"v\"").expect("v present");
    assert!(
        kind_pos < payload_pos && payload_pos < seq_pos && seq_pos < ts_pos && ts_pos < v_pos,
        "canonical form must be sorted-key (kind, payload, seq, ts, v); got: {s:?}"
    );
}

#[test]
fn frame_sign_verify_roundtrip() {
    let device = make_device_key();
    let device_pubkey: [u8; 32] = device.verifying_key().to_bytes();

    let mut frame = input_frame();
    let priv_seed = device.to_bytes();
    let signer = Signer { device_priv: &priv_seed };
    frame.sign(&signer);

    frame
        .verify(&device_pubkey)
        .expect("signed frame must verify under the matching device pubkey");
}

#[test]
fn frame_tamper_seq_fails() {
    let device = make_device_key();
    let device_pubkey: [u8; 32] = device.verifying_key().to_bytes();
    let priv_seed = device.to_bytes();

    let mut frame = input_frame();
    Signer { device_priv: &priv_seed }.sign_into(&mut frame);

    frame.seq = frame.seq.wrapping_add(1);

    let err = frame
        .verify(&device_pubkey)
        .expect_err("changing seq after sign must fail verify");
    assert_eq!(err, FrameAuthError::SignatureInvalid);
}

#[test]
fn frame_tamper_payload_fails() {
    let device = make_device_key();
    let device_pubkey: [u8; 32] = device.verifying_key().to_bytes();
    let priv_seed = device.to_bytes();

    let mut frame = input_frame();
    Signer { device_priv: &priv_seed }.sign_into(&mut frame);

    frame.payload = Bytes::from_static(b"goodbye");

    let err = frame
        .verify(&device_pubkey)
        .expect_err("changing payload after sign must fail verify");
    assert_eq!(err, FrameAuthError::SignatureInvalid);
}

#[test]
fn frame_tamper_kind_fails() {
    let device = make_device_key();
    let device_pubkey: [u8; 32] = device.verifying_key().to_bytes();
    let priv_seed = device.to_bytes();

    let mut frame = input_frame();
    Signer { device_priv: &priv_seed }.sign_into(&mut frame);

    frame.kind = FrameKind::Output;

    let err = frame
        .verify(&device_pubkey)
        .expect_err("changing kind after sign must fail verify");
    assert_eq!(err, FrameAuthError::SignatureInvalid);
}

#[test]
fn frame_decode_invalid_json_errors() {
    let err = Frame::from_json("{ this is not json").expect_err("malformed input must error");
    assert_eq!(err, FrameError::InvalidJson);
}

// Helper: a tiny convenience trait so the tamper tests can read straight.
trait SignerExt {
    fn sign_into(&self, frame: &mut Frame);
}
impl SignerExt for Signer<'_> {
    fn sign_into(&self, frame: &mut Frame) {
        frame.sign(self);
    }
}
