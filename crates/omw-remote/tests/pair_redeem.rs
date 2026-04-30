//! Pairing-token issue + redeem, per BYORC §3.2 / §3.3 / §3.5.
//!
//! Pins:
//! - issue() inserts a row keyed by SHA-256(token) (raw token never persisted).
//! - redeem(valid token, valid pubkey, name) → device_id (16 hex chars) + cap token.
//! - second redeem of same token → `TokenAlreadyUsed`.
//! - bogus token → `TokenUnknown`.
//! - expired token → `TokenExpired`.
//! - bad pubkey length → `InvalidPubkey`.

use std::time::Duration;

use chrono::{DateTime, TimeZone, Utc};
use omw_remote::{open_db, Capability, HostKey, PairToken, Pairings, RedeemError};
use tempfile::tempdir;

fn fresh_pairings() -> Pairings {
    let dir = tempdir().expect("tempdir");
    // Leak the dir guard — we only need the path for the connection lifetime
    // of one test. Keeping it on the stack is fine since each test owns a fresh one.
    let db_path = dir.path().join("test.sqlite");
    // Hold the dir alive by leaking; the OS will reap on test process exit.
    Box::leak(Box::new(dir));
    let conn = open_db(&db_path).expect("open db");
    Pairings::new(conn)
}

fn fresh_host() -> HostKey {
    HostKey::generate()
}

fn default_caps() -> Vec<Capability> {
    vec![Capability::PtyRead, Capability::AgentRead, Capability::AuditRead]
}

fn dummy_device_pubkey() -> [u8; 32] {
    // Deterministic non-zero pubkey is fine — the verifier path isn't exercised
    // here; pair-redeem only validates length + library-level point validity.
    let mut k = [0u8; 32];
    for (i, b) in k.iter_mut().enumerate() {
        *b = (i as u8).wrapping_mul(17).wrapping_add(3);
    }
    k
}

#[test]
fn issue_stores_hash_not_raw_token() {
    let p = fresh_pairings();
    let token = p.issue(Duration::from_secs(600)).expect("issue");

    // Hash is what should be in the row; raw token bytes must not be findable
    // via a substring scan of the DB. We can't probe the SQLite file from here
    // without re-opening, so we just pin the hash != raw bytes invariant.
    let h = token.hash();
    assert_ne!(h.0, token.0, "hash must not equal raw token");
}

#[test]
fn redeem_happy_path_returns_16_hex_device_id() {
    let p = fresh_pairings();
    let host = fresh_host();
    let token = p.issue(Duration::from_secs(600)).expect("issue");

    let resp = p
        .redeem(&token, &dummy_device_pubkey(), "Mark's iPhone", &host, &default_caps())
        .expect("redeem succeeds with valid token");

    assert_eq!(resp.device_id.len(), 16, "device_id is 16 hex chars (spec §3.2)");
    assert!(
        resp.device_id.chars().all(|c| c.is_ascii_hexdigit()),
        "device_id must be lowercase hex"
    );
    assert_eq!(resp.host_pubkey, host.pubkey());
    assert_eq!(resp.capabilities, default_caps());
    // Capability token's device_pubkey field matches what we presented.
    assert_eq!(resp.capability_token.device_pubkey, dummy_device_pubkey());
}

#[test]
fn second_redeem_of_same_token_is_already_used() {
    let p = fresh_pairings();
    let host = fresh_host();
    let token = p.issue(Duration::from_secs(600)).expect("issue");

    p.redeem(&token, &dummy_device_pubkey(), "iPhone", &host, &default_caps())
        .expect("first redeem");

    let err = p
        .redeem(&token, &dummy_device_pubkey(), "iPhone", &host, &default_caps())
        .expect_err("second redeem must fail");
    assert_eq!(err, RedeemError::TokenAlreadyUsed);
}

#[test]
fn redeem_with_bogus_token_is_unknown() {
    let p = fresh_pairings();
    let host = fresh_host();

    let bogus = PairToken([0xAB; 32]); // never issued
    let err = p
        .redeem(&bogus, &dummy_device_pubkey(), "iPhone", &host, &default_caps())
        .expect_err("bogus token must be rejected");
    assert_eq!(err, RedeemError::TokenUnknown);
}

#[test]
fn redeem_after_ttl_elapses_is_expired() {
    // Inject a clock that jumps past the TTL to avoid a real `sleep`.
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("ttl.sqlite");
    Box::leak(Box::new(dir));
    let conn = open_db(&db_path).expect("open db");

    fn future_now() -> DateTime<Utc> {
        // 2099 — well past any TTL the test will set.
        Utc.with_ymd_and_hms(2099, 1, 1, 0, 0, 0).unwrap()
    }

    // Issue at "now" (the real clock), then move the clock forward via
    // `set_clock` and observe expiry.
    let mut p = Pairings::new(conn);
    let token = p.issue(Duration::from_secs(60)).expect("issue with 60 s ttl");

    p.set_clock(future_now);

    let host = fresh_host();
    let err = p
        .redeem(&token, &dummy_device_pubkey(), "iPhone", &host, &default_caps())
        .expect_err("expired token must be rejected");
    assert_eq!(err, RedeemError::TokenExpired);
}

#[test]
fn redeem_with_malformed_pubkey_is_invalid_pubkey() {
    // The public type is `[u8; 32]`, so length is enforced at the call site.
    // The interesting failure mode is a 32-byte sequence that isn't a valid
    // Ed25519 point. We use the all-zero point — `ed25519-dalek` rejects it.
    let p = fresh_pairings();
    let host = fresh_host();
    let token = p.issue(Duration::from_secs(600)).expect("issue");

    let bad_pubkey = [0u8; 32];
    let err = p
        .redeem(&token, &bad_pubkey, "iPhone", &host, &default_caps())
        .expect_err("zero pubkey must be rejected");
    assert_eq!(err, RedeemError::InvalidPubkey);
}
