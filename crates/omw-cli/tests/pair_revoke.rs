//! Integration tests for `omw pair revoke <device_id>` — Phase F.
//!
//! File-boundary note: this file is owned by the Test Overseer under the
//! TRD protocol. The Executor MUST NOT modify it.
//!
//! ## Executor checklist
//!
//! 1. New CLI subcommand: `omw pair revoke <device_id>`.
//! 2. Updates `devices.revoked_at` to "now" in RFC 3339 UTC for the row
//!    matching `id = <device_id>`.
//! 3. Prints a success message on stdout (we accept any phrasing that
//!    contains the literal id) and exits 0.
//! 4. Unknown device id: exits non-zero, stderr contains the literal
//!    substring `not found`. The DB must NOT be mutated in this case.

mod common;

use std::path::{Path, PathBuf};

use omw_remote::open_db;
use rusqlite::params;

use crate::common::omw_cmd;

fn remote_db_in(data_dir: &Path) -> PathBuf {
    data_dir.join("omw-remote.sqlite3")
}

fn seed_one_device(db_path: &Path, id: &str) {
    let conn = open_db(db_path).expect("open_db Phase D schema");
    conn.execute(
        "INSERT INTO devices \
            (id, name, public_key, paired_at, last_seen, permissions_json, revoked_at) \
         VALUES (?1, ?2, ?3, ?4, NULL, ?5, NULL)",
        params![
            id,
            "test-device",
            vec![0x42u8; 32],
            "2026-04-20T10:00:00Z",
            "[\"PtyRead\",\"PtyWrite\"]",
        ],
    )
    .expect("seed device");
}

/// 1. Revoking an existing device sets `revoked_at` and exits 0.
#[test]
fn pair_revoke_marks_device() {
    let dir = tempfile::tempdir().expect("tempdir");
    let data_dir = dir.path().join("data");
    std::fs::create_dir_all(&data_dir).expect("mkdir data");

    let db_path = remote_db_in(&data_dir);
    let device_id = "0123456789abcdef";
    seed_one_device(&db_path, device_id);

    let mut cmd = omw_cmd(dir.path());
    cmd.env("OMW_DATA_DIR", &data_dir);
    let assert = cmd.args(["pair", "revoke", device_id]).assert();
    let output = assert.get_output();
    assert_eq!(
        output.status.code(),
        Some(0),
        "pair revoke on existing id must exit 0; stderr={:?}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify the row's revoked_at is now non-NULL.
    let conn = rusqlite::Connection::open(&db_path).expect("re-open db");
    let revoked_at: Option<String> = conn
        .query_row(
            "SELECT revoked_at FROM devices WHERE id = ?1",
            [device_id],
            |r| r.get(0),
        )
        .expect("query revoked_at");
    assert!(
        revoked_at.is_some(),
        "revoked_at must be non-NULL after `pair revoke`"
    );

    // Sanity: the value parses as RFC 3339 (i.e. it's not a sentinel
    // string like "true" or "1"). Using parse_from_rfc3339 is enough; we
    // don't need to compare it to wall-clock now.
    let raw = revoked_at.unwrap();
    chrono::DateTime::parse_from_rfc3339(&raw)
        .unwrap_or_else(|e| panic!("revoked_at {:?} must be RFC 3339; parse error: {}", raw, e));
}

/// 2. Revoking an unknown id fails with a non-zero exit and a clear
///    `not found` message on stderr. The DB must NOT have any new
///    revocations after this call.
#[test]
fn pair_revoke_unknown_id_fails() {
    let dir = tempfile::tempdir().expect("tempdir");
    let data_dir = dir.path().join("data");
    std::fs::create_dir_all(&data_dir).expect("mkdir data");

    let db_path = remote_db_in(&data_dir);
    // Open the db so the Phase D schema is in place. We seed a single
    // unrelated device to make sure the revoke command targets the
    // queried id specifically and doesn't, e.g., update every row.
    let known_id = "1111111111111111";
    seed_one_device(&db_path, known_id);

    let mut cmd = omw_cmd(dir.path());
    cmd.env("OMW_DATA_DIR", &data_dir);
    let assert = cmd.args(["pair", "revoke", "ffffffffffffffff"]).assert();
    let output = assert.get_output();
    assert_ne!(
        output.status.code(),
        Some(0),
        "pair revoke on unknown id must exit non-zero; stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.to_lowercase().contains("not found"),
        "stderr for unknown id must contain 'not found'; got: {:?}",
        stderr
    );

    // The known device must not have been touched.
    let conn = rusqlite::Connection::open(&db_path).expect("re-open db");
    let revoked_at: Option<String> = conn
        .query_row(
            "SELECT revoked_at FROM devices WHERE id = ?1",
            [known_id],
            |r| r.get(0),
        )
        .expect("query revoked_at");
    assert!(
        revoked_at.is_none(),
        "an unrelated device's revoked_at must remain NULL after a failed revoke; got {:?}",
        revoked_at
    );
}
