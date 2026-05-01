//! Cross-language proof: canonical bytes built the JS way (lines joined
//! with "\n" plus a trailing "\n") match the Rust server's
//! [`CanonicalRequest::to_bytes`] byte-for-byte, AND a signature produced
//! over those JS-shaped bytes verifies against [`Verifier::verify`].
//!
//! This test exists because the JS client and Rust server canonical
//! encodings diverged by one byte (trailing "\n") for an entire session,
//! producing mysterious `signature_invalid` errors only visible at
//! cross-language end-to-end. See commit message of the matching JS fix
//! for the full forensics.

use std::time::Duration;

use chrono::Utc;
use ed25519_dalek::{Signer as _, SigningKey};
use sha2::{Digest, Sha256};

use omw_remote::auth::{CanonicalRequest, Verifier};
use omw_remote::capability::{Capability, CapabilityToken};
use omw_remote::host_key::HostKey;
use omw_remote::replay::NonceStore;

/// Build canonical bytes the JavaScript way:
///   lines.join("\n") + "\n"
/// This is what `apps/web-controller/src/lib/crypto/canonical.ts::canonicalBytes`
/// produces after the trailing-newline fix.
fn js_style_canonical(req: &CanonicalRequest) -> Vec<u8> {
    let body_hash_hex: String = req
        .body_sha256
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect();
    let lines = [
        req.method.clone(),
        req.path.clone(),
        req.query.clone(),
        req.ts.clone(),
        req.nonce.clone(),
        body_hash_hex,
        req.device_id.clone(),
        req.protocol_version.to_string(),
    ];
    let mut s = lines.join("\n");
    s.push('\n');
    s.into_bytes()
}

#[test]
fn js_canonical_matches_rust_to_bytes_byte_for_byte() {
    let req = CanonicalRequest {
        method: "POST".into(),
        path: "/api/v1/sessions".into(),
        query: "".into(),
        ts: "2026-05-02T01:00:00.000Z".into(),
        nonce: "AAAAAAAAAAAAAAAAAAAAAA".into(),
        body_sha256: {
            let empty: [u8; 0] = [];
            let mut h = Sha256::new();
            h.update(empty);
            let out = h.finalize();
            let mut a = [0u8; 32];
            a.copy_from_slice(&out);
            a
        },
        device_id: "abcd1234abcd1234".into(),
        protocol_version: 1,
    };
    let rust_bytes = req.to_bytes();
    let js_bytes = js_style_canonical(&req);
    assert_eq!(
        rust_bytes,
        js_bytes,
        "canonical encodings must agree across languages — Rust:\n{:?}\nJS:\n{:?}",
        String::from_utf8_lossy(&rust_bytes),
        String::from_utf8_lossy(&js_bytes),
    );
}

#[test]
fn signature_over_js_canonical_verifies_via_server_verifier() {
    // 1. Generate a device keypair (the same shape the JS client does).
    let mut device_seed = [0u8; 32];
    for (i, b) in device_seed.iter_mut().enumerate() {
        *b = (i as u8) ^ 0xa5;
    }
    let device_signing = SigningKey::from_bytes(&device_seed);
    let device_pub: [u8; 32] = device_signing.verifying_key().to_bytes();

    // 2. Generate a host key so we can mint a real capability token.
    let host = HostKey::generate();
    let host_pub: [u8; 32] = host.pubkey();

    // 3. Mint a capability token the device will present.
    let now = Utc::now();
    let device_id: String = {
        let mut h = Sha256::new();
        h.update(device_pub);
        let out = h.finalize();
        out[..16].iter().map(|b| format!("{:02x}", b)).collect()
    };
    let cap_token = CapabilityToken::issue(
        &host,
        device_pub,
        device_id.clone(),
        vec![Capability::PtyWrite, Capability::PtyRead],
        Duration::from_secs(3600),
    );
    let cap_b64 = cap_token.to_base64url();

    // 4. Build a canonical request matching what the JS client would build
    //    for `POST /api/v1/sessions` with body `{"name":"main"}`.
    let body = br#"{"name":"main"}"#;
    let body_sha256: [u8; 32] = {
        let mut h = Sha256::new();
        h.update(body);
        let out = h.finalize();
        let mut a = [0u8; 32];
        a.copy_from_slice(&out);
        a
    };
    let req = CanonicalRequest {
        method: "POST".into(),
        path: "/api/v1/sessions".into(),
        query: "".into(),
        ts: now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        nonce: "abcdefghijklmnopqrstuv".into(),
        body_sha256,
        device_id: device_id.clone(),
        protocol_version: 1,
    };

    // 5. Sign the JS-shaped canonical bytes with the device key.
    let js_bytes = js_style_canonical(&req);
    let sig: [u8; 64] = device_signing.sign(&js_bytes).to_bytes();

    // 6. Verify with the server's Verifier — same code path
    //    `crates/omw-remote/src/http/sessions.rs::verify_signed` uses.
    let nonce_store = NonceStore::new(Duration::from_secs(60));
    let verifier = Verifier::new(host_pub, nonce_store);
    let verified_id = verifier
        .verify(&req, &sig, &cap_b64, Capability::PtyWrite, now)
        .expect("verifier must accept a JS-shaped signature when canonical encodings match");
    assert_eq!(verified_id, device_id);
}
