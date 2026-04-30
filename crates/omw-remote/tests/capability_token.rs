//! Capability token issuance + verification, per BYORC §5.

use std::time::Duration;

use chrono::{Duration as ChronoDuration, Utc};
use omw_remote::{Capability, CapabilityError, CapabilityToken, HostKey};

fn fresh_device_pubkey() -> [u8; 32] {
    let mut k = [0u8; 32];
    for (i, b) in k.iter_mut().enumerate() {
        *b = (i as u8).wrapping_mul(7).wrapping_add(11);
    }
    k
}

#[test]
fn issued_token_verifies_against_host_pubkey() {
    let host = HostKey::generate();
    let now = Utc::now();
    let token = CapabilityToken::issue(
        &host,
        fresh_device_pubkey(),
        "a1b2c3d4e5f6a7b8".into(),
        vec![Capability::PtyRead],
        Duration::from_secs(30 * 24 * 3600),
    );

    token
        .verify(&host.pubkey(), now)
        .expect("freshly issued token must verify");
}

#[test]
fn tampered_capabilities_fail_verification() {
    let host = HostKey::generate();
    let mut token = CapabilityToken::issue(
        &host,
        fresh_device_pubkey(),
        "a1b2c3d4e5f6a7b8".into(),
        vec![Capability::PtyRead],
        Duration::from_secs(60),
    );

    // Privilege escalation attempt: silently swap in pty:write after issuance.
    token.capabilities = vec![Capability::PtyRead, Capability::PtyWrite];

    let now = Utc::now();
    let err = token
        .verify(&host.pubkey(), now)
        .expect_err("tampered capabilities must invalidate the signature");
    assert_eq!(err, CapabilityError::Invalid);
}

#[test]
fn tampered_device_pubkey_fails_verification() {
    let host = HostKey::generate();
    let mut token = CapabilityToken::issue(
        &host,
        fresh_device_pubkey(),
        "a1b2c3d4e5f6a7b8".into(),
        vec![Capability::PtyRead],
        Duration::from_secs(60),
    );

    // An attacker tries to bind their own pubkey to a stolen capability token.
    let mut attacker_pubkey = [0u8; 32];
    attacker_pubkey[0] = 0xFF;
    token.device_pubkey = attacker_pubkey;

    let now = Utc::now();
    let err = token
        .verify(&host.pubkey(), now)
        .expect_err("device_pubkey swap must invalidate the signature");
    assert_eq!(err, CapabilityError::Invalid);
}

#[test]
fn expired_token_returns_capability_expired() {
    let host = HostKey::generate();
    let token = CapabilityToken::issue(
        &host,
        fresh_device_pubkey(),
        "a1b2c3d4e5f6a7b8".into(),
        vec![Capability::PtyRead],
        Duration::from_secs(60),
    );

    // Verify with a "now" that is past the token's expires_at.
    let way_after = token.expires_at + ChronoDuration::seconds(120);
    let err = token
        .verify(&host.pubkey(), way_after)
        .expect_err("expired token must be rejected");
    assert_eq!(err, CapabilityError::Expired);
}

#[test]
fn allows_returns_true_only_for_granted_scopes() {
    let host = HostKey::generate();
    let token = CapabilityToken::issue(
        &host,
        fresh_device_pubkey(),
        "a1b2c3d4e5f6a7b8".into(),
        vec![Capability::PtyRead, Capability::AgentRead],
        Duration::from_secs(60),
    );

    assert!(token.allows(Capability::PtyRead));
    assert!(token.allows(Capability::AgentRead));
    assert!(!token.allows(Capability::PtyWrite));
    assert!(!token.allows(Capability::PairAdmin));
}

#[test]
fn base64url_round_trip_preserves_signature() {
    let host = HostKey::generate();
    let token = CapabilityToken::issue(
        &host,
        fresh_device_pubkey(),
        "a1b2c3d4e5f6a7b8".into(),
        vec![Capability::PtyRead],
        Duration::from_secs(60),
    );

    let wire = token.to_base64url();
    let decoded = CapabilityToken::from_base64url(&wire).expect("decode");
    let now = Utc::now();
    decoded
        .verify(&host.pubkey(), now)
        .expect("round-tripped token must still verify");
    assert_eq!(decoded.device_id, token.device_id);
}
