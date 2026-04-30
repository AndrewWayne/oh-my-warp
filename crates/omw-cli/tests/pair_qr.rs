//! Integration tests for `omw pair qr` — Phase F.
//!
//! File-boundary note: this file is owned by the Test Overseer under the
//! TRD protocol. The Executor MUST NOT modify it.
//!
//! ## Executor checklist (gates these tests)
//!
//! 1. New CLI subcommand: `omw pair qr [--ttl <minutes>] [--out
//!    <terminal|png|svg>] [--base-url <url>]`.
//!    - Default `--ttl` = 10 minutes.
//!    - Default `--out` = terminal (ASCII QR, not a binary file).
//!    - Default `--base-url` = `https://127.0.0.1:8787`.
//! 2. Issues a fresh pair token via `omw_remote::Pairings::issue` and
//!    inserts a row into the `pairings` table at
//!    `<OMW_DATA_DIR>/omw-remote.sqlite3` (the Phase D schema lives in
//!    `omw-remote/migrations/2026-05-01-init.sql`).
//! 3. Prints the pairing URL to stdout in the canonical form
//!    `https://<base>/pair?t=<crockford-base32-token>`. The token is the
//!    base32 encoding of the 32-byte raw `PairToken`.
//! 4. Stores SHA-256 of the token (NOT the raw token) in
//!    `pairings.token_hash`. The base32 token must NOT appear anywhere in
//!    the on-disk db.
//! 5. Sets `pairings.expires_at` to roughly `now + ttl_minutes`. Tests
//!    accept a ±2 minute slop window.
//! 6. The exact db filename used by `omw pair *` and `omw remote *` is
//!    `omw-remote.sqlite3` inside `OMW_DATA_DIR`. This is distinct from
//!    the cost-telemetry db (`omw.sqlite3`) so the two stores can evolve
//!    independently.

mod common;

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use rusqlite::Connection;

use crate::common::omw_cmd;

/// Resolved path to the omw-remote SQLite db inside `data_dir`.
fn remote_db_in(data_dir: &Path) -> PathBuf {
    data_dir.join("omw-remote.sqlite3")
}

/// 1. The `omw pair qr` happy path prints a `/pair?t=<token>` URL whose
///    token decodes as a valid base32 string of the expected length.
#[test]
fn pair_qr_prints_url_and_token() {
    let dir = tempfile::tempdir().expect("tempdir");
    let data_dir = dir.path().join("data");
    std::fs::create_dir_all(&data_dir).expect("mkdir data");

    let mut cmd = omw_cmd(dir.path());
    cmd.env("OMW_DATA_DIR", &data_dir);
    let assert = cmd.args(["pair", "qr"]).assert();
    let output = assert.get_output();

    assert_eq!(
        output.status.code(),
        Some(0),
        "pair qr must exit 0; stderr={:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);

    // The URL must appear somewhere in stdout. We don't pin the exact host
    // (default is 127.0.0.1:8787), but the path + query shape is fixed.
    assert!(
        stdout.contains("/pair?t="),
        "stdout must contain a `/pair?t=<token>` URL; got:\n{}",
        stdout
    );

    // Extract the token after `t=` and validate it's a non-empty
    // Crockford base32 string. The 32-byte `PairToken` encodes to ~52
    // characters of base32; we only assert "looks like base32 and is at
    // least 26 chars" so an impl change to the encoder padding doesn't
    // break the test.
    let after = stdout
        .split("/pair?t=")
        .nth(1)
        .expect("split off /pair?t=");
    // The token runs to whitespace or end-of-string.
    let token: String = after
        .chars()
        .take_while(|c| !c.is_whitespace())
        .take(120)
        .collect();
    assert!(
        token.len() >= 26,
        "extracted token is implausibly short ({} chars): {:?}",
        token.len(),
        token
    );
    let alphabet_ok = token.chars().all(|c| {
        // Crockford base32: 0-9 + A-Z minus I, L, O, U. Allow lowercase too.
        c.is_ascii_alphanumeric() && !matches!(c.to_ascii_uppercase(), 'I' | 'L' | 'O' | 'U')
    });
    assert!(
        alphabet_ok,
        "token contains non-Crockford-base32 characters: {:?}",
        token
    );
}

/// 2. Running `omw pair qr` writes exactly one row into the `pairings`
///    table at `<OMW_DATA_DIR>/omw-remote.sqlite3`.
#[test]
fn pair_qr_writes_token_to_db() {
    let dir = tempfile::tempdir().expect("tempdir");
    let data_dir = dir.path().join("data");
    std::fs::create_dir_all(&data_dir).expect("mkdir data");

    let mut cmd = omw_cmd(dir.path());
    cmd.env("OMW_DATA_DIR", &data_dir);
    let assert = cmd.args(["pair", "qr"]).assert();
    let output = assert.get_output();
    assert_eq!(
        output.status.code(),
        Some(0),
        "pair qr must exit 0; stderr={:?}",
        String::from_utf8_lossy(&output.stderr)
    );

    let db_path = remote_db_in(&data_dir);
    assert!(
        db_path.exists(),
        "pair qr must create the omw-remote SQLite db at {:?}",
        db_path
    );

    let conn = Connection::open(&db_path).expect("open omw-remote db");
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM pairings", [], |r| r.get(0))
        .expect("count pairings");
    assert_eq!(
        count, 1,
        "pair qr must insert exactly one pairings row; got {}",
        count
    );

    // Sanity: the stored row must have a non-empty token_hash and a
    // non-empty expires_at — a buggy impl that wrote NULLs would still
    // satisfy a naive COUNT(*) check.
    let (hash_len, expires_at): (i64, String) = conn
        .query_row(
            "SELECT length(token_hash), expires_at FROM pairings LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("query pairings row");
    assert_eq!(
        hash_len, 32,
        "pairings.token_hash must be 32 bytes (SHA-256); got {}",
        hash_len
    );
    assert!(
        !expires_at.is_empty(),
        "pairings.expires_at must be a non-empty RFC 3339 string"
    );
}

/// 3. `--ttl 5` sets `expires_at` to ~5 minutes in the future. Accept a
///    ±2-minute slop window to absorb test scheduler / I/O delays.
#[test]
fn pair_qr_with_ttl() {
    let dir = tempfile::tempdir().expect("tempdir");
    let data_dir = dir.path().join("data");
    std::fs::create_dir_all(&data_dir).expect("mkdir data");

    let before = Utc::now();
    let mut cmd = omw_cmd(dir.path());
    cmd.env("OMW_DATA_DIR", &data_dir);
    let assert = cmd.args(["pair", "qr", "--ttl", "5"]).assert();
    let output = assert.get_output();
    assert_eq!(
        output.status.code(),
        Some(0),
        "pair qr --ttl 5 must exit 0; stderr={:?}",
        String::from_utf8_lossy(&output.stderr)
    );

    let db_path = remote_db_in(&data_dir);
    let conn = Connection::open(&db_path).expect("open omw-remote db");
    let expires_at_str: String = conn
        .query_row(
            "SELECT expires_at FROM pairings ORDER BY id DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .expect("query latest pairings row");

    let expires_at = DateTime::parse_from_rfc3339(&expires_at_str)
        .unwrap_or_else(|e| panic!("parse expires_at {:?}: {}", expires_at_str, e))
        .with_timezone(&Utc);

    let target = before + chrono::Duration::minutes(5);
    let delta = (expires_at - target).num_seconds().abs();
    assert!(
        delta <= 120,
        "expires_at ({}) must be within 2 minutes of target ({}); delta = {}s",
        expires_at,
        target,
        delta
    );
}
