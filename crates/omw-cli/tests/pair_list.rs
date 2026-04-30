//! Integration tests for `omw pair list` — Phase F.
//!
//! File-boundary note: this file is owned by the Test Overseer under the
//! TRD protocol. The Executor MUST NOT modify it.
//!
//! ## Executor checklist (gates these tests)
//!
//! 1. New CLI subcommand: `omw pair list`. Reads from the
//!    `<OMW_DATA_DIR>/omw-remote.sqlite3` db, table `devices`. The schema
//!    is defined in `omw-remote/migrations/2026-05-01-init.sql`:
//!
//!    ```sql
//!    CREATE TABLE devices (
//!        id                TEXT PRIMARY KEY,
//!        name              TEXT NOT NULL,
//!        public_key        BLOB NOT NULL,
//!        paired_at         TEXT NOT NULL,
//!        last_seen         TEXT,
//!        permissions_json  TEXT NOT NULL,
//!        revoked_at        TEXT
//!    );
//!    ```
//! 2. Empty case: stdout matches a literal `no paired devices` message.
//!    Exit 0.
//! 3. Non-empty: stdout contains every device's `id` and `name`. Order
//!    is unspecified; the test only asserts presence.
//! 4. The `omw pair list` command must NOT require the daemon to be
//!    running — it's a direct DB read.

mod common;

use std::path::{Path, PathBuf};

use omw_remote::open_db;
use rusqlite::params;

use crate::common::omw_cmd;

fn remote_db_in(data_dir: &Path) -> PathBuf {
    data_dir.join("omw-remote.sqlite3")
}

/// 1. Empty-db case: `omw pair list` must succeed and emit a recognizable
///    "no paired devices" message.
#[test]
fn pair_list_empty_prints_message() {
    let dir = tempfile::tempdir().expect("tempdir");
    let data_dir = dir.path().join("data");
    std::fs::create_dir_all(&data_dir).expect("mkdir data");

    // Create the db file with the Phase D schema applied so the SUT's
    // SELECT against `devices` doesn't trip "no such table". The Executor
    // is free to also do this on first read in their `pair list`
    // implementation; we set it up here defensively.
    let db_path = remote_db_in(&data_dir);
    let conn = open_db(&db_path).expect("open_db Phase D schema");
    drop(conn);

    let mut cmd = omw_cmd(dir.path());
    cmd.env("OMW_DATA_DIR", &data_dir);
    let assert = cmd.args(["pair", "list"]).assert();
    let output = assert.get_output();

    assert_eq!(
        output.status.code(),
        Some(0),
        "pair list on empty db must exit 0; stderr={:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}\n{}", stdout, stderr).to_lowercase();
    assert!(
        combined.contains("no paired devices"),
        "empty list must mention 'no paired devices'; got stdout={:?} stderr={:?}",
        stdout,
        stderr
    );
}

/// 2. Two device rows seeded directly via rusqlite must both appear in
///    `omw pair list` output. The id and the name columns are the load-
///    bearing cells for an operator scanning the table.
#[test]
fn pair_list_shows_devices() {
    let dir = tempfile::tempdir().expect("tempdir");
    let data_dir = dir.path().join("data");
    std::fs::create_dir_all(&data_dir).expect("mkdir data");

    let db_path = remote_db_in(&data_dir);
    let conn = open_db(&db_path).expect("open_db Phase D schema");

    // Insert two device rows. `public_key` is a 32-byte blob; we use
    // synthetic but valid byte strings (NOT actual Ed25519 points — the
    // pair-list command doesn't crypto-verify them).
    let pk_a = vec![0xAAu8; 32];
    let pk_b = vec![0xBBu8; 32];
    conn.execute(
        "INSERT INTO devices \
            (id, name, public_key, paired_at, last_seen, permissions_json, revoked_at) \
         VALUES (?1, ?2, ?3, ?4, NULL, ?5, NULL)",
        params![
            "aaaaaaaaaaaaaaaa",
            "iphone-andrea",
            pk_a,
            "2026-04-20T10:00:00Z",
            "[\"PtyRead\",\"PtyWrite\"]",
        ],
    )
    .expect("insert device A");
    conn.execute(
        "INSERT INTO devices \
            (id, name, public_key, paired_at, last_seen, permissions_json, revoked_at) \
         VALUES (?1, ?2, ?3, ?4, NULL, ?5, NULL)",
        params![
            "bbbbbbbbbbbbbbbb",
            "ipad-andrea",
            pk_b,
            "2026-04-21T10:00:00Z",
            "[\"PtyRead\"]",
        ],
    )
    .expect("insert device B");
    drop(conn);

    let mut cmd = omw_cmd(dir.path());
    cmd.env("OMW_DATA_DIR", &data_dir);
    let assert = cmd.args(["pair", "list"]).assert();
    let output = assert.get_output();
    assert_eq!(
        output.status.code(),
        Some(0),
        "pair list with seeded devices must exit 0; stderr={:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);

    for needle in [
        "aaaaaaaaaaaaaaaa",
        "iphone-andrea",
        "bbbbbbbbbbbbbbbb",
        "ipad-andrea",
    ] {
        assert!(
            stdout.contains(needle),
            "stdout must mention {:?}; got\n{}",
            needle,
            stdout
        );
    }
}
