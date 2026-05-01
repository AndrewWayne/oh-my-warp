//! Full HTTP-request verification ladder, per BYORC §4.2.
//!
//! Covers positive path + the seven distinct failure paths the verifier
//! enumerates: bad signature, replayed nonce, ts skew, capability/device
//! pubkey mismatch, and out-of-scope capability.
//!
//! These tests use `ed25519_dalek` directly because the test fixture needs to
//! mint a device key whose pubkey it can embed in a capability token (the host
//! pairing flow does this server-side; here we simulate the device).

use std::time::Duration;

use chrono::{Duration as ChronoDuration, Utc};
use ed25519_dalek::SigningKey;
use omw_remote::{
    AuthError, Capability, CanonicalRequest, CapabilityToken, HostKey, NonceStore, Signer, Verifier,
};
use sha2::{Digest, Sha256};

fn make_device_key() -> SigningKey {
    let seed = [42u8; 32];
    SigningKey::from_bytes(&seed)
}

fn device_id_from_pubkey(pk: &[u8; 32]) -> String {
    let digest = Sha256::digest(pk);
    hex_lower(&digest[..8])
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

fn body_hash(body: &[u8]) -> [u8; 32] {
    let h = Sha256::digest(body);
    let mut out = [0u8; 32];
    out.copy_from_slice(&h);
    out
}

struct Fixture {
    #[allow(dead_code)] // Held to keep the host pairing key alive across the test.
    host: HostKey,
    device: SigningKey,
    #[allow(dead_code)] // Pubkey is embedded in cap_token_b64; field kept for clarity.
    device_pubkey: [u8; 32],
    device_id: String,
    cap_token_b64: String,
    verifier: Verifier,
}

fn fixture_with_caps(caps: Vec<Capability>) -> Fixture {
    let host = HostKey::generate();
    let device = make_device_key();
    let device_pubkey = device.verifying_key().to_bytes();
    let device_id = device_id_from_pubkey(&device_pubkey);

    let cap_token = CapabilityToken::issue(
        &host,
        device_pubkey,
        device_id.clone(),
        caps,
        Duration::from_secs(30 * 24 * 3600),
    );
    let cap_token_b64 = cap_token.to_base64url();

    let nonce_store = NonceStore::new(Duration::from_secs(60));
    let verifier = Verifier::new(host.pubkey(), nonce_store);
    Fixture {
        host,
        device,
        device_pubkey,
        device_id,
        cap_token_b64,
        verifier,
    }
}

fn build_canonical(body: &[u8], device_id: &str, ts: chrono::DateTime<Utc>, nonce: &str) -> CanonicalRequest {
    CanonicalRequest {
        method: "POST".into(),
        path: "/api/v1/sessions/abc/input".into(),
        query: String::new(),
        ts: ts.to_rfc3339(),
        nonce: nonce.into(),
        body_sha256: body_hash(body),
        device_id: device_id.into(),
        protocol_version: 1,
    }
}

fn sign_canonical(device: &SigningKey, req: &CanonicalRequest) -> [u8; 64] {
    let priv_seed = device.to_bytes();
    let signer = Signer { device_priv: &priv_seed };
    signer.sign(req)
}

#[test]
fn canonical_request_bytes_match_spec_layout() {
    // Spec §4.1: 8 newline-terminated lines.
    let body = b"hello";
    let req = build_canonical(body, "deadbeefcafef00d", Utc::now(), "nonce-abc");
    let bytes = req.to_bytes();
    let s = std::str::from_utf8(&bytes).expect("canonical request must be utf8");

    // Eight lines, each terminated by '\n'.
    assert_eq!(s.matches('\n').count(), 8, "must be 8 newline-terminated lines");

    let lines: Vec<&str> = s.split('\n').collect();
    // After 8 trailing newlines, split yields 9 segments with the last empty.
    assert_eq!(lines.len(), 9);
    assert_eq!(lines[0], "POST");
    assert_eq!(lines[1], "/api/v1/sessions/abc/input");
    assert_eq!(lines[2], ""); // empty query
    // lines[3] is RFC3339 ts, lines[4] is nonce
    assert_eq!(lines[4], "nonce-abc");
    // body hash is hex-lower of SHA-256("hello")
    let expected_hash = hex_lower(&Sha256::digest(body));
    assert_eq!(lines[5], expected_hash);
    assert_eq!(lines[6], "deadbeefcafef00d");
    assert_eq!(lines[7], "1");
    assert_eq!(lines[8], "");
}

#[test]
fn happy_path_verifies_and_returns_device_id() {
    let f = fixture_with_caps(vec![Capability::PtyWrite]);
    let body = b"{\"data\":\"ls\"}";
    let now = Utc::now();
    let req = build_canonical(body, &f.device_id, now, "nonce-1");
    let sig = sign_canonical(&f.device, &req);

    let device_id = f
        .verifier
        .verify(&req, &sig, &f.cap_token_b64, Capability::PtyWrite, now)
        .expect("happy path verifies");
    assert_eq!(device_id, f.device_id);
}

#[test]
fn tampered_method_rejects_with_signature_invalid() {
    let f = fixture_with_caps(vec![Capability::PtyWrite]);
    let body = b"x";
    let now = Utc::now();
    let req = build_canonical(body, &f.device_id, now, "nonce-method");
    let sig = sign_canonical(&f.device, &req);

    let mut tampered = req.clone();
    tampered.method = "GET".into();

    let err = f
        .verifier
        .verify(&tampered, &sig, &f.cap_token_b64, Capability::PtyWrite, now)
        .expect_err("changing method must invalidate the signature");
    assert_eq!(err, AuthError::SignatureInvalid);
}

#[test]
fn tampered_path_rejects_with_signature_invalid() {
    let f = fixture_with_caps(vec![Capability::PtyWrite]);
    let body = b"x";
    let now = Utc::now();
    let req = build_canonical(body, &f.device_id, now, "nonce-path");
    let sig = sign_canonical(&f.device, &req);

    let mut tampered = req.clone();
    tampered.path = "/api/v1/devices/xyz/revoke".into();

    let err = f
        .verifier
        .verify(&tampered, &sig, &f.cap_token_b64, Capability::PtyWrite, now)
        .expect_err("changing path must invalidate the signature");
    assert_eq!(err, AuthError::SignatureInvalid);
}

#[test]
fn tampered_body_rejects_with_signature_invalid() {
    let f = fixture_with_caps(vec![Capability::PtyWrite]);
    let body = b"original";
    let now = Utc::now();
    let req = build_canonical(body, &f.device_id, now, "nonce-body");
    let sig = sign_canonical(&f.device, &req);

    // Reconstruct with a different body — body_sha256 changes, so the
    // canonical bytes the server sees no longer match what was signed.
    let mut tampered = req.clone();
    tampered.body_sha256 = body_hash(b"swapped");

    let err = f
        .verifier
        .verify(&tampered, &sig, &f.cap_token_b64, Capability::PtyWrite, now)
        .expect_err("changing body hash must invalidate the signature");
    assert_eq!(err, AuthError::SignatureInvalid);
}

#[test]
fn replayed_nonce_rejects_second_attempt() {
    let f = fixture_with_caps(vec![Capability::PtyWrite]);
    let body = b"x";
    let now = Utc::now();
    let req = build_canonical(body, &f.device_id, now, "nonce-shared");
    let sig = sign_canonical(&f.device, &req);

    // First attempt — accepted.
    f.verifier
        .verify(&req, &sig, &f.cap_token_b64, Capability::PtyWrite, now)
        .expect("first request accepted");

    // Second attempt with same nonce — rejected.
    let err = f
        .verifier
        .verify(&req, &sig, &f.cap_token_b64, Capability::PtyWrite, now)
        .expect_err("replay must be rejected");
    assert_eq!(err, AuthError::NonceReplayed);
}

#[test]
fn ts_outside_skew_window_rejects_with_ts_skew() {
    let f = fixture_with_caps(vec![Capability::PtyWrite]);
    let body = b"x";
    // Verifier::new uses a 300 s skew window (see auth.rs::Verifier::new
    // for the rationale — mobile clients drift). Sign with a ts well past
    // that window.
    let stale = Utc::now() - ChronoDuration::seconds(600);
    let req = build_canonical(body, &f.device_id, stale, "nonce-stale");
    let sig = sign_canonical(&f.device, &req);

    let err = f
        .verifier
        .verify(&req, &sig, &f.cap_token_b64, Capability::PtyWrite, Utc::now())
        .expect_err("ts outside skew window must be rejected");
    assert_eq!(err, AuthError::TsSkew);
}

#[test]
fn capability_token_pubkey_must_match_signing_device() {
    // Mint a capability token bound to a *different* device pubkey, then have
    // our test device try to use it. §4.2 step 7 must reject.
    let host = HostKey::generate();
    let real_device = make_device_key();
    let other_seed = [99u8; 32];
    let other_device = SigningKey::from_bytes(&other_seed);
    let other_pubkey = other_device.verifying_key().to_bytes();

    // Capability token belongs to "other" device.
    let cap_token = CapabilityToken::issue(
        &host,
        other_pubkey,
        device_id_from_pubkey(&other_pubkey),
        vec![Capability::PtyWrite],
        Duration::from_secs(60),
    );
    let cap_token_b64 = cap_token.to_base64url();

    // But "real" device signs the request, and uses the real device's id.
    let real_pubkey = real_device.verifying_key().to_bytes();
    let real_device_id = device_id_from_pubkey(&real_pubkey);
    let nonce_store = NonceStore::new(Duration::from_secs(60));
    let verifier = Verifier::new(host.pubkey(), nonce_store);

    let body = b"x";
    let now = Utc::now();
    let req = build_canonical(body, &real_device_id, now, "nonce-mismatch");
    let sig = sign_canonical(&real_device, &req);

    let err = verifier
        .verify(&req, &sig, &cap_token_b64, Capability::PtyWrite, now)
        .expect_err("device pubkey mismatch must reject");
    // Either signature_invalid (sig doesn't verify under cap token's pubkey)
    // or capability_invalid (device_id mismatch detected earlier). Both are
    // acceptable per spec §4.2 — pin to signature_invalid as the canonical
    // verification-step failure.
    assert_eq!(err, AuthError::SignatureInvalid);
}

#[test]
fn out_of_scope_capability_rejects_with_capability_scope() {
    // Token grants only pty:read, but caller asks for pty:write.
    let f = fixture_with_caps(vec![Capability::PtyRead]);
    let body = b"x";
    let now = Utc::now();
    let req = build_canonical(body, &f.device_id, now, "nonce-scope");
    let sig = sign_canonical(&f.device, &req);

    let err = f
        .verifier
        .verify(&req, &sig, &f.cap_token_b64, Capability::PtyWrite, now)
        .expect_err("scope mismatch must reject");
    assert_eq!(err, AuthError::CapabilityScope);
}

#[test]
fn expired_capability_token_rejects_with_capability_expired() {
    // Mint a capability token whose expires_at is already in the past.
    let host = HostKey::generate();
    let device = make_device_key();
    let device_pubkey = device.verifying_key().to_bytes();
    let device_id = device_id_from_pubkey(&device_pubkey);

    // 1-second TTL so expiration arrives within the test by the time we verify.
    let cap_token = CapabilityToken::issue(
        &host,
        device_pubkey,
        device_id.clone(),
        vec![Capability::PtyWrite],
        Duration::from_secs(1),
    );
    let cap_token_b64 = cap_token.to_base64url();

    let verifier = Verifier::new(host.pubkey(), NonceStore::new(Duration::from_secs(60)));
    let body = b"x";
    let signed_at = Utc::now();
    let req = build_canonical(body, &device_id, signed_at, "nonce-cap-exp");
    let sig = sign_canonical(&device, &req);

    // Verify with "now" 5 minutes after signed_at — the request is fresh from
    // the verifier's perspective in terms of ts skew (we pass `signed_at` for
    // the request's ts) but the cap token has expired.
    //
    // To pin specifically `CapabilityExpired` and not `TsSkew`, advance both
    // the request ts and now together, and rely on the cap token's 1-second TTL.
    let later = signed_at + ChronoDuration::seconds(120);
    let req_late = build_canonical(body, &device_id, later, "nonce-cap-exp-2");
    let sig_late = {
        let priv_seed = device.to_bytes();
        Signer { device_priv: &priv_seed }.sign(&req_late)
    };

    let err = verifier
        .verify(&req_late, &sig_late, &cap_token_b64, Capability::PtyWrite, later)
        .expect_err("expired capability must be rejected");
    assert_eq!(err, AuthError::CapabilityExpired);

    // Touch the unused happy-path bindings so dead-code lint stays quiet
    // even on platforms where Utc resolution makes signed_at == later somehow.
    let _ = (signed_at, req, sig);
}
