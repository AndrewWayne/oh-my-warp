//! Integration tests for cost telemetry: the `omw-cli::db` module and the
//! `omw costs` subcommand.
//!
//! File-boundary note: this file is owned by the Test Overseer under the
//! TRD protocol. The Executor MUST NOT modify it. The Executor authors
//! `crates/omw-cli/src/db.rs`, `crates/omw-cli/src/commands/costs.rs`, the
//! `omw costs` clap wiring in `lib.rs`, and the usage-capture code path
//! inside `commands/ask.rs`. Test infrastructure (the helpers in
//! `tests/common/mod.rs` and `tests/fixtures/fake-agent.cjs`) is also
//! Overseer-owned.
//!
//! ## Executor checklist (gates beyond `cli_provider.rs` / `cli_ask.rs`)
//!
//! 1. New module `crates/omw-cli/src/db.rs` exposing:
//!
//!    ```rust,ignore
//!    pub fn data_dir() -> anyhow::Result<std::path::PathBuf>;
//!    pub fn db_path() -> anyhow::Result<std::path::PathBuf>;
//!    pub fn open() -> anyhow::Result<rusqlite::Connection>;
//!    pub fn open_at(path: &std::path::Path) -> anyhow::Result<rusqlite::Connection>;
//!    pub fn seed_pricing(conn: &rusqlite::Connection) -> anyhow::Result<()>;
//!
//!    pub struct UsageRecord {
//!        pub provider_id: String,
//!        pub provider_kind: String,  // "openai" | "anthropic" | "ollama" | "openai-compatible"
//!        pub model: String,
//!        pub prompt_tokens: u64,
//!        pub completion_tokens: u64,
//!        pub total_tokens: u64,
//!        pub duration_ms: u64,
//!    }
//!    pub fn record_usage(conn: &rusqlite::Connection, rec: &UsageRecord) -> anyhow::Result<i64>;
//!
//!    pub enum GroupBy { Provider, Model, Day }
//!    pub struct RollupRow {
//!        pub key: String,
//!        pub total_prompt_tokens: i64,
//!        pub total_completion_tokens: i64,
//!        pub total_cost_cents: Option<i64>,
//!        pub call_count: i64,
//!    }
//!    pub fn cost_rollup(
//!        conn: &rusqlite::Connection,
//!        since: Option<chrono::DateTime<chrono::Utc>>,
//!        group_by: GroupBy,
//!    ) -> anyhow::Result<Vec<RollupRow>>;
//!    ```
//!
//! 2. `data_dir()` resolution order (highest priority first):
//!    - `OMW_DATA_DIR` env var (treated as the data dir directly).
//!    - `XDG_DATA_HOME/omw` (Linux/Unix XDG layout).
//!    - `$HOME/.local/share/omw` (final fallback).
//!
//! 3. `open()` and `open_at()` apply migrations idempotently. Schema
//!    (full SQL is the Executor's responsibility, but the test gate is):
//!
//!    - `usage_records` table with at minimum the columns referenced by
//!      these tests: `provider_id TEXT`, `provider_kind TEXT`,
//!      `model TEXT`, `prompt_tokens INTEGER`, `completion_tokens
//!      INTEGER`, `total_tokens INTEGER`, `duration_ms INTEGER`,
//!      `cost_cents INTEGER NULL`, `created_at TEXT` (ISO-8601 UTC).
//!    - `provider_pricing` table with `provider_kind TEXT`, `model
//!      TEXT`, `prompt_cost_per_million_cents INTEGER`,
//!      `completion_cost_per_million_cents INTEGER`, `effective_at TEXT`.
//!
//! 4. `record_usage()` MUST set `created_at` to "now" in UTC and MUST
//!    compute `cost_cents` from the most-recent (effective_at) matching
//!    `provider_pricing` row at insert time. If no row matches, store
//!    `cost_cents = NULL`.
//!
//! 5. `seed_pricing()` is idempotent: calling twice does not duplicate
//!    rows or error. It seeds the schedule listed in the spec
//!    (openai gpt-4o 250/1000, openai gpt-4o-mini 15/60, anthropic
//!    claude-sonnet-4-6 300/1500, anthropic claude-haiku-4-5 80/400,
//!    ollama any 0/0; effective_at = "2026-01-01T00:00:00Z").
//!
//! 6. New CLI subcommand: `omw costs [--since YYYY-MM-DD]
//!    [--by provider|model|day]`. Default since = 30 days ago. Default
//!    by = provider. Output: aligned table + a final `Total` row.
//!
//! 7. `omw ask` change: capture the LAST line of child stderr, parse it
//!    as JSON `{prompt_tokens, completion_tokens, total_tokens,
//!    provider, model, duration_ms}`, look up the provider's `kind`
//!    from omw-config, and call `db::record_usage`. Failures from the
//!    db write MUST NOT change the exit code or surface as user-visible
//!    errors (defensive instrumentation).

mod common;

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use chrono::{TimeZone, Utc};
use rusqlite::Connection;
use serde::Deserialize;

use crate::common::{env_lock, lib_mode_run, omw_cmd, seed_config};

// =============================================================================
// Local helpers (cost-telemetry-specific; not reused outside this file)
// =============================================================================

/// Process-global lock for tests that mutate `OMW_DATA_DIR` and call
/// `lib_mode_run`. The shared `env_lock()` from `common` already covers
/// `OMW_CONFIG`/`OMW_KEYCHAIN_BACKEND`; we reuse it to keep the
/// process-env mutation surface serialized end-to-end. (We hold both in
/// the same lock by always taking `env_lock()` first.)
fn data_lock() -> std::sync::MutexGuard<'static, ()> {
    static L: OnceLock<Mutex<()>> = OnceLock::new();
    L.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// Path to the Node fixture, expressed via `CARGO_MANIFEST_DIR`. cargo
/// sets that env var for every integration test.
fn fake_agent_script() -> PathBuf {
    let manifest = std::env::var("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR must be set by cargo for integration tests");
    Path::new(&manifest)
        .join("tests")
        .join("fixtures")
        .join("fake-agent.cjs")
}

/// Build a single-executable wrapper inside `dir` that forwards all args
/// to `node <fake-agent.cjs>`. Mirrors the helper in `cli_ask.rs`; we
/// duplicate it here because each integration-test file is its own crate
/// and `common::mod.rs` is intentionally minimal.
fn write_agent_wrapper(dir: &Path) -> PathBuf {
    let script = fake_agent_script();
    if cfg!(windows) {
        let wrapper = dir.join("fake-agent.cmd");
        let body = format!("@echo off\r\n\"node\" \"{}\" %*\r\n", script.display());
        std::fs::write(&wrapper, body).expect("write windows wrapper");
        wrapper
    } else {
        let wrapper = dir.join("fake-agent.sh");
        let body = format!("#!/bin/sh\nexec node \"{}\" \"$@\"\n", script.display());
        std::fs::write(&wrapper, body).expect("write unix wrapper");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perm = std::fs::metadata(&wrapper).expect("stat").permissions();
            perm.set_mode(0o755);
            std::fs::set_permissions(&wrapper, perm).expect("chmod");
        }
        wrapper
    }
}

/// Resolved path of the SQLite db file given an `OMW_DATA_DIR` value.
/// The Executor's `db_path()` is the source of truth; tests only use
/// this when they need to open the same file rusqlite-side to verify
/// side-effects of an in-process `lib_mode_run`. Filename is part of
/// the test contract.
fn db_file_in(data_dir: &Path) -> PathBuf {
    data_dir.join("omw.sqlite3")
}

/// Open a fresh in-memory connection, run the Executor's migrations,
/// and return it. We call `omw_cli::db::open_at` against a path inside
/// a tempdir rather than `:memory:` — the public API takes a filesystem
/// path; pinning to `:memory:` would require a separate API surface.
///
/// Returns the connection AND the tempdir guard (caller must keep it
/// alive for the duration of the test).
fn fresh_db(prefix: &str) -> (tempfile::TempDir, Connection) {
    let dir = tempfile::Builder::new()
        .prefix(prefix)
        .tempdir()
        .expect("tempdir");
    let path = dir.path().join("omw.sqlite3");
    let conn = omw_cli::db::open_at(&path).expect("open_at on fresh path");
    (dir, conn)
}

/// Insert a usage row with a SPECIFIC `created_at` value. The public
/// `record_usage` API stamps "now"; tests that need historical rows
/// (date filtering, day grouping) write directly via SQL after seeding.
/// This bypass is intentional and limited to date-shape tests.
fn insert_dated_usage(
    conn: &Connection,
    provider_id: &str,
    provider_kind: &str,
    model: &str,
    prompt_tokens: i64,
    completion_tokens: i64,
    duration_ms: i64,
    created_at: &str, // ISO-8601, e.g. "2026-04-15T10:00:00Z"
    cost_cents: Option<i64>,
) {
    conn.execute(
        "INSERT INTO usage_records \
            (provider_id, provider_kind, model, prompt_tokens, \
             completion_tokens, total_tokens, duration_ms, cost_cents, \
             created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        rusqlite::params![
            provider_id,
            provider_kind,
            model,
            prompt_tokens,
            completion_tokens,
            prompt_tokens + completion_tokens,
            duration_ms,
            cost_cents,
            created_at,
        ],
    )
    .expect("manual usage_records INSERT");
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct PricingRow {
    provider_kind: String,
    model: String,
    prompt: i64,
    completion: i64,
}

// =============================================================================
// Group A — `omw costs` subcommand: subprocess tests
//
// These tests boot the cargo-built `omw` binary against a tempdir-scoped
// `OMW_DATA_DIR` and inspect stdout. They DO NOT depend on the in-process
// memory keychain because `costs` doesn't touch the keychain.
// =============================================================================

/// 1. Empty-db case. With no `usage_records` rows, `omw costs` must
/// exit 0 and indicate that there is nothing to report. We accept any
/// of a few canonical phrasings to give the Executor wiggle room on
/// exact wording.
#[test]
fn costs_on_empty_db_outputs_no_records() {
    let dir = tempfile::tempdir().expect("tempdir");
    let data_dir = dir.path().join("data");
    std::fs::create_dir_all(&data_dir).expect("mkdir data");

    let mut cmd = omw_cmd(dir.path());
    cmd.env("OMW_DATA_DIR", &data_dir);
    let assert = cmd.args(["costs"]).assert();
    let output = assert.get_output();

    assert_eq!(
        output.status.code(),
        Some(0),
        "costs on empty db must exit 0; stderr={:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}\n{}", stdout, stderr).to_lowercase();
    assert!(
        combined.contains("no usage")
            || combined.contains("no records")
            || combined.contains("(no usage")
            || combined.contains("nothing"),
        "empty-db output must indicate no records; got stdout={:?} stderr={:?}",
        stdout,
        stderr
    );
}

/// 2. Group-by-provider with three providers. Seeded directly via
/// rusqlite into the same db file the subprocess will read.
#[test]
fn costs_with_seed_data_groups_by_provider() {
    let dir = tempfile::tempdir().expect("tempdir");
    let data_dir = dir.path().join("data");
    std::fs::create_dir_all(&data_dir).expect("mkdir data");
    let db = db_file_in(&data_dir);
    let conn = omw_cli::db::open_at(&db).expect("open_at");
    omw_cli::db::seed_pricing(&conn).expect("seed_pricing");

    // Three rows, three providers. Use a recent date so the default
    // 30-day window includes them.
    let recent = "2026-04-20T10:00:00Z";
    insert_dated_usage(
        &conn,
        "openai-prod",
        "openai",
        "gpt-4o",
        1_000_000,
        1_000_000,
        100,
        recent,
        Some(1250),
    );
    insert_dated_usage(
        &conn,
        "anthropic-prod",
        "anthropic",
        "claude-sonnet-4-6",
        1_000_000,
        1_000_000,
        100,
        recent,
        Some(1800),
    );
    insert_dated_usage(
        &conn,
        "local-llama",
        "ollama",
        "llama3",
        1_000_000,
        1_000_000,
        100,
        recent,
        Some(0),
    );
    drop(conn);

    let mut cmd = omw_cmd(dir.path());
    cmd.env("OMW_DATA_DIR", &data_dir);
    let assert = cmd.args(["costs", "--by", "provider"]).assert();
    let output = assert.get_output();
    assert_eq!(
        output.status.code(),
        Some(0),
        "costs --by provider must exit 0; stderr={:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);

    // All three provider ids must appear in the output (order is up to
    // the impl, but each must be present).
    for id in ["openai-prod", "anthropic-prod", "local-llama"] {
        assert!(
            stdout.contains(id),
            "stdout must mention provider id {:?}; got\n{}",
            id,
            stdout
        );
    }
    // The default `--by provider` should NOT collapse three rows into
    // one. Each provider should occupy its own line.
    let lines: Vec<&str> = stdout.lines().collect();
    let openai_line = lines
        .iter()
        .find(|l| l.contains("openai-prod"))
        .unwrap_or_else(|| panic!("no line for openai-prod in:\n{}", stdout));
    let anth_line = lines
        .iter()
        .find(|l| l.contains("anthropic-prod"))
        .unwrap_or_else(|| panic!("no line for anthropic-prod in:\n{}", stdout));
    assert!(
        !std::ptr::eq(*openai_line, *anth_line),
        "openai and anthropic must be on different lines; got combined line {:?}",
        openai_line
    );
}

/// 3. `--since` filter excludes rows older than the cutoff.
#[test]
fn costs_filters_by_since() {
    let dir = tempfile::tempdir().expect("tempdir");
    let data_dir = dir.path().join("data");
    std::fs::create_dir_all(&data_dir).expect("mkdir data");
    let db = db_file_in(&data_dir);
    let conn = omw_cli::db::open_at(&db).expect("open_at");
    omw_cli::db::seed_pricing(&conn).expect("seed_pricing");

    // One old row (March), one new row (April).
    insert_dated_usage(
        &conn,
        "old-provider",
        "openai",
        "gpt-4o",
        1_000_000,
        1_000_000,
        100,
        "2026-03-15T10:00:00Z",
        Some(1250),
    );
    insert_dated_usage(
        &conn,
        "new-provider",
        "anthropic",
        "claude-sonnet-4-6",
        1_000_000,
        1_000_000,
        100,
        "2026-04-15T10:00:00Z",
        Some(1800),
    );
    drop(conn);

    let mut cmd = omw_cmd(dir.path());
    cmd.env("OMW_DATA_DIR", &data_dir);
    let assert = cmd.args(["costs", "--since", "2026-04-01"]).assert();
    let output = assert.get_output();
    assert_eq!(
        output.status.code(),
        Some(0),
        "costs --since must exit 0; stderr={:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("new-provider"),
        "post-cutoff provider must appear in output:\n{}",
        stdout
    );
    assert!(
        !stdout.contains("old-provider"),
        "pre-cutoff provider must be filtered out, but was present:\n{}",
        stdout
    );
}

/// 4. `--by day` groups by date. Seed two rows on the same day and one
/// on a different day; expect two distinct day rows in output.
#[test]
fn costs_groups_by_day() {
    let dir = tempfile::tempdir().expect("tempdir");
    let data_dir = dir.path().join("data");
    std::fs::create_dir_all(&data_dir).expect("mkdir data");
    let db = db_file_in(&data_dir);
    let conn = omw_cli::db::open_at(&db).expect("open_at");
    omw_cli::db::seed_pricing(&conn).expect("seed_pricing");

    insert_dated_usage(
        &conn,
        "p1",
        "openai",
        "gpt-4o",
        1_000_000,
        1_000_000,
        100,
        "2026-04-10T08:00:00Z",
        Some(1250),
    );
    insert_dated_usage(
        &conn,
        "p1",
        "openai",
        "gpt-4o",
        1_000_000,
        1_000_000,
        200,
        "2026-04-10T20:00:00Z",
        Some(1250),
    );
    insert_dated_usage(
        &conn,
        "p1",
        "openai",
        "gpt-4o",
        1_000_000,
        1_000_000,
        300,
        "2026-04-15T12:00:00Z",
        Some(1250),
    );
    drop(conn);

    let mut cmd = omw_cmd(dir.path());
    cmd.env("OMW_DATA_DIR", &data_dir);
    let assert = cmd
        .args(["costs", "--since", "2026-04-01", "--by", "day"])
        .assert();
    let output = assert.get_output();
    assert_eq!(
        output.status.code(),
        Some(0),
        "costs --by day must exit 0; stderr={:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Both distinct dates must be present in their YYYY-MM-DD form.
    assert!(
        stdout.contains("2026-04-10"),
        "day-grouping must surface 2026-04-10:\n{}",
        stdout
    );
    assert!(
        stdout.contains("2026-04-15"),
        "day-grouping must surface 2026-04-15:\n{}",
        stdout
    );

    // The two rows for 2026-04-10 must be COLLAPSED into one row, not
    // emitted twice. Match by counting non-Total lines that mention
    // "2026-04-10".
    let day_line_count = stdout
        .lines()
        .filter(|l| l.contains("2026-04-10") && !l.to_lowercase().contains("total"))
        .count();
    assert_eq!(
        day_line_count, 1,
        "day-grouping must collapse multiple rows for the same date into ONE row; \
         saw {} lines mentioning 2026-04-10:\n{}",
        day_line_count, stdout
    );
}

/// 5. `--by model` groups by model name regardless of provider.
#[test]
fn costs_groups_by_model() {
    let dir = tempfile::tempdir().expect("tempdir");
    let data_dir = dir.path().join("data");
    std::fs::create_dir_all(&data_dir).expect("mkdir data");
    let db = db_file_in(&data_dir);
    let conn = omw_cli::db::open_at(&db).expect("open_at");
    omw_cli::db::seed_pricing(&conn).expect("seed_pricing");

    let recent = "2026-04-20T10:00:00Z";
    // Two rows on gpt-4o (different provider ids), one on
    // claude-sonnet-4-6. By-model should yield two rows total.
    insert_dated_usage(
        &conn,
        "openai-prod-1",
        "openai",
        "gpt-4o",
        1_000_000,
        1_000_000,
        100,
        recent,
        Some(1250),
    );
    insert_dated_usage(
        &conn,
        "openai-prod-2",
        "openai",
        "gpt-4o",
        1_000_000,
        1_000_000,
        100,
        recent,
        Some(1250),
    );
    insert_dated_usage(
        &conn,
        "anthropic-prod",
        "anthropic",
        "claude-sonnet-4-6",
        1_000_000,
        1_000_000,
        100,
        recent,
        Some(1800),
    );
    drop(conn);

    let mut cmd = omw_cmd(dir.path());
    cmd.env("OMW_DATA_DIR", &data_dir);
    let assert = cmd.args(["costs", "--by", "model"]).assert();
    let output = assert.get_output();
    assert_eq!(
        output.status.code(),
        Some(0),
        "costs --by model must exit 0; stderr={:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("gpt-4o"),
        "model output must include gpt-4o:\n{}",
        stdout
    );
    assert!(
        stdout.contains("claude-sonnet-4-6"),
        "model output must include claude-sonnet-4-6:\n{}",
        stdout
    );
    // The two openai rows must collapse into ONE gpt-4o line.
    let gpt_line_count = stdout
        .lines()
        .filter(|l| l.contains("gpt-4o") && !l.to_lowercase().contains("total"))
        .count();
    assert_eq!(
        gpt_line_count, 1,
        "model-grouping must collapse two gpt-4o rows into ONE; saw {} lines:\n{}",
        gpt_line_count, stdout
    );
}

/// 6. The trailing `Total` row sums the per-group columns. We seed with
/// known integer cents per row so we can pin the expected total.
#[test]
fn costs_total_row_sums_correctly() {
    let dir = tempfile::tempdir().expect("tempdir");
    let data_dir = dir.path().join("data");
    std::fs::create_dir_all(&data_dir).expect("mkdir data");
    let db = db_file_in(&data_dir);
    let conn = omw_cli::db::open_at(&db).expect("open_at");
    omw_cli::db::seed_pricing(&conn).expect("seed_pricing");

    let recent = "2026-04-20T10:00:00Z";
    // Three rows; cost_cents column sums to 100 + 250 + 50 = 400.
    insert_dated_usage(
        &conn,
        "p1",
        "openai",
        "gpt-4o",
        100_000,
        100_000,
        50,
        recent,
        Some(100),
    );
    insert_dated_usage(
        &conn,
        "p2",
        "anthropic",
        "claude-sonnet-4-6",
        100_000,
        100_000,
        50,
        recent,
        Some(250),
    );
    insert_dated_usage(
        &conn,
        "p3",
        "openai",
        "gpt-4o-mini",
        100_000,
        100_000,
        50,
        recent,
        Some(50),
    );
    drop(conn);

    let mut cmd = omw_cmd(dir.path());
    cmd.env("OMW_DATA_DIR", &data_dir);
    let assert = cmd.args(["costs"]).assert();
    let output = assert.get_output();
    assert_eq!(
        output.status.code(),
        Some(0),
        "costs must exit 0; stderr={:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);

    let total_line = stdout
        .lines()
        .find(|l| l.to_lowercase().contains("total"))
        .unwrap_or_else(|| {
            panic!(
                "output must include a row whose key contains 'Total':\n{}",
                stdout
            )
        });
    // 400 cents == $4.00. We accept either the cents-int form or the
    // dollar form. The test fails if neither is present.
    let cents_form = total_line.contains("400");
    let dollar_form = total_line.contains("4.00") || total_line.contains("$4.00");
    assert!(
        cents_form || dollar_form,
        "Total row must show summed cost (400 cents or $4.00); got {:?}",
        total_line
    );

    // The Total row must also sum prompt+completion tokens.
    // 3 rows × 100_000 prompt = 300_000 prompt; 3 × 100_000 completion
    // = 300_000 completion. The renderer may use either count or
    // human-formatted (e.g. "300000" or "300,000" or "300k"). Accept
    // any form that contains the digits "300".
    assert!(
        total_line.contains("300"),
        "Total row should reflect aggregate token counts (>=300k each); got {:?}",
        total_line
    );
}

// =============================================================================
// Group B — `db` module unit-style tests against a real on-disk SQLite
// =============================================================================

/// 7. `record_usage` inserts a row and computes `cost_cents` from the
/// matching pricing snapshot.
#[test]
fn record_usage_inserts_row_with_cost_cents() {
    let (_dir, conn) = fresh_db("rec_usage_cost_");
    omw_cli::db::seed_pricing(&conn).expect("seed_pricing");

    // openai gpt-4o is priced at 250 / 1000 cents per million tokens.
    // 1_000_000 prompt + 2_000_000 completion =>
    //   250 * 1 + 1000 * 2 = 250 + 2000 = 2250 cents.
    let rec = omw_cli::db::UsageRecord {
        provider_id: "openai-prod".to_string(),
        provider_kind: "openai".to_string(),
        model: "gpt-4o".to_string(),
        prompt_tokens: 1_000_000,
        completion_tokens: 2_000_000,
        total_tokens: 3_000_000,
        duration_ms: 1234,
    };
    let id = omw_cli::db::record_usage(&conn, &rec).expect("record_usage");
    assert!(id > 0, "record_usage must return a positive rowid, got {id}");

    let (cost_cents, model, kind): (Option<i64>, String, String) = conn
        .query_row(
            "SELECT cost_cents, model, provider_kind FROM usage_records WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .expect("query inserted row");
    assert_eq!(model, "gpt-4o", "model must round-trip");
    assert_eq!(kind, "openai", "provider_kind must round-trip");
    assert_eq!(
        cost_cents,
        Some(2250),
        "cost_cents must be 250*1 + 1000*2 = 2250 cents"
    );
}

/// 8. Newer pricing rows take precedence over older ones for the same
/// provider_kind+model.
#[test]
fn record_usage_uses_latest_pricing_snapshot() {
    let (_dir, conn) = fresh_db("rec_usage_latest_");

    // Two pricing rows for openai gpt-4o: an old one at 100/200, and a
    // new one at 250/1000. The newer row must win.
    conn.execute(
        "INSERT INTO provider_pricing \
            (provider_kind, model, prompt_cost_per_million_cents, \
             completion_cost_per_million_cents, effective_at) \
         VALUES ('openai', 'gpt-4o', 100, 200, '2025-01-01T00:00:00Z')",
        [],
    )
    .expect("insert old pricing");
    conn.execute(
        "INSERT INTO provider_pricing \
            (provider_kind, model, prompt_cost_per_million_cents, \
             completion_cost_per_million_cents, effective_at) \
         VALUES ('openai', 'gpt-4o', 250, 1000, '2026-01-01T00:00:00Z')",
        [],
    )
    .expect("insert new pricing");

    // 1M prompt + 1M completion → with NEW pricing: 250 + 1000 = 1250.
    //                              With OLD pricing: 100 + 200 = 300.
    let rec = omw_cli::db::UsageRecord {
        provider_id: "openai-prod".to_string(),
        provider_kind: "openai".to_string(),
        model: "gpt-4o".to_string(),
        prompt_tokens: 1_000_000,
        completion_tokens: 1_000_000,
        total_tokens: 2_000_000,
        duration_ms: 100,
    };
    let id = omw_cli::db::record_usage(&conn, &rec).expect("record_usage");

    let cost: Option<i64> = conn
        .query_row(
            "SELECT cost_cents FROM usage_records WHERE id = ?1",
            [id],
            |r| r.get(0),
        )
        .expect("query");
    assert_eq!(
        cost,
        Some(1250),
        "newer pricing snapshot (effective 2026-01-01) must win over older \
         (2025-01-01); 250+1000 = 1250"
    );
}

/// 9. Unknown provider/model → row inserted with `cost_cents = NULL`.
#[test]
fn record_usage_with_unknown_pricing_stores_null_cost() {
    let (_dir, conn) = fresh_db("rec_usage_null_");
    omw_cli::db::seed_pricing(&conn).expect("seed_pricing");

    let rec = omw_cli::db::UsageRecord {
        provider_id: "nobody".to_string(),
        provider_kind: "openai-compatible".to_string(),
        model: "no-such-model-9999".to_string(),
        prompt_tokens: 1_000_000,
        completion_tokens: 1_000_000,
        total_tokens: 2_000_000,
        duration_ms: 50,
    };
    let id = omw_cli::db::record_usage(&conn, &rec).expect("record_usage");

    let cost: Option<i64> = conn
        .query_row(
            "SELECT cost_cents FROM usage_records WHERE id = ?1",
            [id],
            |r| r.get(0),
        )
        .expect("query");
    assert_eq!(
        cost, None,
        "unknown provider_kind+model must yield NULL cost_cents; got {:?}",
        cost
    );

    // The other token columns must round-trip even when cost is NULL —
    // a buggy impl that bails out when pricing is missing would skip
    // the insert entirely.
    let (pt, ct): (i64, i64) = conn
        .query_row(
            "SELECT prompt_tokens, completion_tokens FROM usage_records WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("query token columns");
    assert_eq!(pt, 1_000_000);
    assert_eq!(ct, 1_000_000);
}

/// 10. `seed_pricing` is idempotent: calling twice does not duplicate
/// rows or error.
#[test]
fn seed_pricing_idempotent() {
    let (_dir, conn) = fresh_db("seed_idempotent_");

    omw_cli::db::seed_pricing(&conn).expect("first seed");
    let count_first: i64 = conn
        .query_row("SELECT COUNT(*) FROM provider_pricing", [], |r| r.get(0))
        .expect("count after first seed");
    assert!(
        count_first >= 5,
        "seed_pricing should populate at least 5 rows (openai gpt-4o, \
         openai gpt-4o-mini, anthropic claude-sonnet-4-6, anthropic \
         claude-haiku-4-5, ollama any); got {}",
        count_first
    );

    omw_cli::db::seed_pricing(&conn).expect("second seed must not error");
    let count_second: i64 = conn
        .query_row("SELECT COUNT(*) FROM provider_pricing", [], |r| r.get(0))
        .expect("count after second seed");
    assert_eq!(
        count_first, count_second,
        "second seed_pricing must not duplicate rows; first={} second={}",
        count_first, count_second
    );

    // Spot-check the openai gpt-4o pricing values per spec
    // (250 prompt, 1000 completion). Allow one canonical row.
    let (prompt, completion): (i64, i64) = conn
        .query_row(
            "SELECT prompt_cost_per_million_cents, completion_cost_per_million_cents \
             FROM provider_pricing \
             WHERE provider_kind = 'openai' AND model = 'gpt-4o' \
             ORDER BY effective_at DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("query openai/gpt-4o pricing");
    assert_eq!(prompt, 250, "openai/gpt-4o prompt price must be 250 cents/M");
    assert_eq!(
        completion, 1000,
        "openai/gpt-4o completion price must be 1000 cents/M"
    );
}

/// 11. `data_dir()` honors `OMW_DATA_DIR` when set.
#[test]
fn data_dir_honors_omw_data_dir() {
    let _g = env_lock();
    let _g2 = data_lock();

    let dir = tempfile::tempdir().expect("tempdir");
    let want = dir.path().join("explicit-data-dir");
    std::env::set_var("OMW_DATA_DIR", &want);
    // Clear the XDG/HOME fallbacks so a stale env doesn't shadow the
    // override path resolution.
    std::env::remove_var("XDG_DATA_HOME");

    let resolved = omw_cli::db::data_dir().expect("data_dir with OMW_DATA_DIR set");
    assert_eq!(
        resolved, want,
        "data_dir must equal the OMW_DATA_DIR env value verbatim"
    );

    // Tidy up so this doesn't bleed into sibling tests.
    std::env::remove_var("OMW_DATA_DIR");
}

/// 12. `data_dir()` without override falls back to a sensible XDG-style
/// path. We don't pin the OS-default — that depends on the platform —
/// but we DO assert the path ends with the `omw` segment so a buggy
/// impl that returned, say, the parent directory would be caught.
#[test]
fn data_dir_default_xdg_layout() {
    let _g = env_lock();
    let _g2 = data_lock();

    let dir = tempfile::tempdir().expect("tempdir");
    std::env::remove_var("OMW_DATA_DIR");
    let xdg = dir.path().join("xdg-data");
    std::env::set_var("XDG_DATA_HOME", &xdg);

    let resolved = omw_cli::db::data_dir().expect("data_dir with XDG_DATA_HOME set");

    // Must terminate with `omw` (XDG convention: $XDG_DATA_HOME/omw).
    assert_eq!(
        resolved.file_name().and_then(|s| s.to_str()),
        Some("omw"),
        "default data_dir must terminate with 'omw' segment, got {:?}",
        resolved
    );
    // Must be under the XDG_DATA_HOME we configured.
    assert!(
        resolved.starts_with(&xdg),
        "default data_dir under XDG_DATA_HOME must live inside it; \
         expected prefix {:?}, got {:?}",
        xdg,
        resolved
    );

    std::env::remove_var("XDG_DATA_HOME");
}

// =============================================================================
// Group C — End-to-end `omw ask` -> db side-effect tests
// =============================================================================

/// 13. After a successful `omw ask`, the usage row must be persisted.
/// We use `lib_mode_run` so we can read back the SQLite file the same
/// in-process invocation wrote to (subprocess-based tests would also
/// work for SQLite, but the spec calls out `lib_mode_run` because the
/// fake-agent path through clap runs identically and we want to keep
/// state inspection simple).
#[test]
fn ask_writes_usage_record_after_successful_call() {
    let _g = env_lock();
    let _g2 = data_lock();

    let dir = tempfile::tempdir().expect("tempdir");
    let data_dir = dir.path().join("data");
    std::fs::create_dir_all(&data_dir).expect("mkdir data");
    let cfg_path = dir.path().join("config.toml");

    // Seed config with a provider whose id matches what the fake-agent
    // emits in its usage line (FAKE_AGENT_PROVIDER defaults to "test").
    // We make it ollama so no key is required.
    seed_config(
        dir.path(),
        r#"version = 1

[providers.test]
kind = "ollama"
"#,
    );

    let wrapper = write_agent_wrapper(dir.path());

    // Configure process env for the lib_mode_run.
    std::env::set_var("OMW_CONFIG", &cfg_path);
    std::env::set_var("OMW_KEYCHAIN_BACKEND", "memory");
    std::env::set_var("OMW_DATA_DIR", &data_dir);
    std::env::set_var("OMW_AGENT_BIN", &wrapper);

    let (code, _stdout, stderr) = lib_mode_run(&["ask", "hello"]);
    assert_eq!(
        code,
        0,
        "ask should succeed via fake-agent; stderr={:?}",
        String::from_utf8_lossy(&stderr)
    );

    // Inspect the db directly. The Executor's record_usage must have
    // been called with the values the fake-agent emitted on its last
    // stderr line (10 prompt, 20 completion, 30 total, provider=test,
    // model=test-model, duration_ms=100).
    let db = db_file_in(&data_dir);
    assert!(
        db.exists(),
        "ask must have created the SQLite db at {:?}",
        db
    );
    let conn = rusqlite::Connection::open(&db).expect("open db readonly");
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM usage_records", [], |r| r.get(0))
        .expect("count usage_records");
    assert_eq!(
        count, 1,
        "ask must insert exactly one usage_records row; got {}",
        count
    );

    let (provider_id, model, pt, ct, tt, dur): (String, String, i64, i64, i64, i64) = conn
        .query_row(
            "SELECT provider_id, model, prompt_tokens, completion_tokens, \
                    total_tokens, duration_ms \
             FROM usage_records LIMIT 1",
            [],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            },
        )
        .expect("query usage row");
    assert_eq!(provider_id, "test", "provider_id must round-trip");
    assert_eq!(model, "test-model", "model must round-trip");
    assert_eq!(pt, 10, "prompt_tokens must round-trip");
    assert_eq!(ct, 20, "completion_tokens must round-trip");
    assert_eq!(tt, 30, "total_tokens must round-trip");
    assert_eq!(dur, 100, "duration_ms must round-trip");

    // Tidy.
    std::env::remove_var("OMW_DATA_DIR");
    std::env::remove_var("OMW_AGENT_BIN");
}

/// 14. If the db write fails, `omw ask` must STILL exit with the
/// agent's exit code (0 here). The instrumentation is best-effort.
///
/// We force the failure by pointing `OMW_DATA_DIR` at a path that is
/// itself a regular file: `create_dir_all` and `Connection::open` will
/// both fail in that scenario on every platform.
#[test]
fn ask_does_not_fail_if_db_write_fails() {
    let _g = env_lock();
    let _g2 = data_lock();

    let dir = tempfile::tempdir().expect("tempdir");
    let cfg_path = dir.path().join("config.toml");

    seed_config(
        dir.path(),
        r#"version = 1

[providers.test]
kind = "ollama"
"#,
    );

    // Create a regular file at the path we'll set OMW_DATA_DIR to.
    // Any attempt to create_dir or open <file>/omw.sqlite3 will fail.
    let bad_data_dir = dir.path().join("not-a-directory");
    std::fs::write(&bad_data_dir, b"this is a file, not a directory").expect("write file");

    let wrapper = write_agent_wrapper(dir.path());

    std::env::set_var("OMW_CONFIG", &cfg_path);
    std::env::set_var("OMW_KEYCHAIN_BACKEND", "memory");
    std::env::set_var("OMW_DATA_DIR", &bad_data_dir);
    std::env::set_var("OMW_AGENT_BIN", &wrapper);

    let (code, stdout, stderr) = lib_mode_run(&["ask", "ping"]);
    assert_eq!(
        code,
        0,
        "ask must exit 0 even when db write fails; stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&stdout),
        String::from_utf8_lossy(&stderr)
    );

    // Defense in depth: the agent's stdout must still be streamed
    // through. (A buggy impl that aborts on db error before forwarding
    // stdout would fail this — and the user-visible regression of
    // "agent succeeded but I see nothing" is exactly what this test
    // guards against.)
    let stdout_str = String::from_utf8_lossy(&stdout);
    assert!(
        !stdout_str.is_empty(),
        "agent stdout must still be streamed even when db write fails; got empty"
    );

    std::env::remove_var("OMW_DATA_DIR");
    std::env::remove_var("OMW_AGENT_BIN");
}

// =============================================================================
// Sanity: chrono is in scope so `since: Option<DateTime<Utc>>` use sites
// in the spec compile. We don't otherwise use this binding; ignoring a
// dead-code warning is preferable to a stray import vanishing if the
// Executor changes the API shape.
// =============================================================================
#[allow(dead_code)]
fn _chrono_in_scope() -> chrono::DateTime<chrono::Utc> {
    Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap()
}
