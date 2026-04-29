//! SQLite-backed cost telemetry store for `omw`.
//!
//! See `crates/omw-cli/tests/cli_costs.rs` for the public-API contract.
//! Schema columns match the test-header gate verbatim.
//!
//! Invariant I-1: only token counts, provider/model identifiers, and
//! computed cost cents are persisted. No secret values touch this store.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};

/// One usage event recorded after a successful agent call.
pub struct UsageRecord {
    pub provider_id: String,
    pub provider_kind: String,
    pub model: String,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    pub duration_ms: u64,
}

/// Grouping axis for `cost_rollup`.
#[derive(Copy, Clone, Debug)]
pub enum GroupBy {
    Provider,
    Model,
    Day,
}

/// One row of a `cost_rollup` result. `total_cost_cents` is `None` when
/// every contributing row's `cost_cents` was NULL (unknown pricing).
pub struct RollupRow {
    pub key: String,
    pub total_prompt_tokens: i64,
    pub total_completion_tokens: i64,
    pub total_cost_cents: Option<i64>,
    pub call_count: i64,
}

/// Resolve the data directory. Resolution order:
///   1. `OMW_DATA_DIR` (used directly)
///   2. `XDG_DATA_HOME/omw`
///   3. `$HOME/.local/share/omw` (or `%USERPROFILE%\.local\share\omw`)
pub fn data_dir() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("OMW_DATA_DIR") {
        if !p.is_empty() {
            return Ok(PathBuf::from(p));
        }
    }
    if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
        if !xdg.is_empty() {
            return Ok(PathBuf::from(xdg).join("omw"));
        }
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .context("neither HOME nor USERPROFILE is set")?;
    Ok(home.join(".local").join("share").join("omw"))
}

/// Path to the SQLite db file. Filename is part of the test contract.
pub fn db_path() -> Result<PathBuf> {
    Ok(data_dir()?.join("omw.sqlite3"))
}

/// Open the default db, creating parent directories and applying migrations.
pub fn open() -> Result<Connection> {
    let path = db_path()?;
    open_at(&path)
}

/// Open a connection at `path`, creating parent directories and applying
/// migrations idempotently.
pub fn open_at(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating data dir {}", parent.display()))?;
    }
    let conn = Connection::open(path)
        .with_context(|| format!("opening sqlite db at {}", path.display()))?;
    apply_migrations(&conn)?;
    Ok(conn)
}

fn apply_migrations(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        PRAGMA journal_mode = WAL;
        PRAGMA foreign_keys = ON;

        CREATE TABLE IF NOT EXISTS provider_pricing (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            provider_kind TEXT NOT NULL,
            model TEXT NOT NULL,
            prompt_cost_per_million_cents INTEGER NOT NULL,
            completion_cost_per_million_cents INTEGER NOT NULL,
            effective_at TEXT NOT NULL,
            UNIQUE (provider_kind, model, effective_at)
        );

        CREATE TABLE IF NOT EXISTS usage_records (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            provider_id TEXT NOT NULL,
            provider_kind TEXT NOT NULL,
            model TEXT NOT NULL,
            prompt_tokens INTEGER NOT NULL,
            completion_tokens INTEGER NOT NULL,
            total_tokens INTEGER NOT NULL,
            duration_ms INTEGER NOT NULL,
            cost_cents INTEGER,
            created_at TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_usage_created_at ON usage_records(created_at);
        CREATE INDEX IF NOT EXISTS idx_usage_provider_model ON usage_records(provider_id, model);
        "#,
    )
    .context("applying migrations")?;
    Ok(())
}

/// Seed the canonical pricing schedule. Idempotent: re-running does not
/// duplicate rows or error.
pub fn seed_pricing(conn: &Connection) -> Result<()> {
    let effective_at = "2026-01-01T00:00:00Z";
    let rows: &[(&str, &str, i64, i64)] = &[
        ("openai", "gpt-4o", 250, 1000),
        ("openai", "gpt-4o-mini", 15, 60),
        ("anthropic", "claude-sonnet-4-6", 300, 1500),
        ("anthropic", "claude-haiku-4-5", 80, 400),
        ("ollama", "*", 0, 0),
    ];
    for (kind, model, prompt, completion) in rows {
        conn.execute(
            "INSERT OR IGNORE INTO provider_pricing \
                (provider_kind, model, prompt_cost_per_million_cents, \
                 completion_cost_per_million_cents, effective_at) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![kind, model, prompt, completion, effective_at],
        )
        .context("seeding provider_pricing")?;
    }
    Ok(())
}

/// Insert a usage record, computing `cost_cents` from the latest matching
/// `provider_pricing` snapshot. Returns the inserted rowid.
pub fn record_usage(conn: &Connection, rec: &UsageRecord) -> Result<i64> {
    let cost_cents = compute_cost_cents(
        conn,
        &rec.provider_kind,
        &rec.model,
        rec.prompt_tokens,
        rec.completion_tokens,
    )?;
    let now = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    conn.execute(
        "INSERT INTO usage_records \
            (provider_id, provider_kind, model, prompt_tokens, \
             completion_tokens, total_tokens, duration_ms, cost_cents, \
             created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            rec.provider_id,
            rec.provider_kind,
            rec.model,
            rec.prompt_tokens as i64,
            rec.completion_tokens as i64,
            rec.total_tokens as i64,
            rec.duration_ms as i64,
            cost_cents,
            now,
        ],
    )
    .context("inserting usage_records row")?;
    Ok(conn.last_insert_rowid())
}

/// Look up the latest pricing snapshot (by `effective_at`) for the given
/// `provider_kind`+`model` and compute the cost in integer cents. Returns
/// `None` if no pricing row matches.
fn compute_cost_cents(
    conn: &Connection,
    provider_kind: &str,
    model: &str,
    prompt_tokens: u64,
    completion_tokens: u64,
) -> Result<Option<i64>> {
    let row: Option<(i64, i64)> = conn
        .query_row(
            "SELECT prompt_cost_per_million_cents, completion_cost_per_million_cents \
             FROM provider_pricing \
             WHERE provider_kind = ?1 AND model = ?2 \
             ORDER BY effective_at DESC LIMIT 1",
            params![provider_kind, model],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .context("looking up provider_pricing")?;
    let Some((prompt_cents_per_m, completion_cents_per_m)) = row else {
        return Ok(None);
    };
    // (tokens / 1_000_000) * cents_per_million, rounded to nearest cent.
    let prompt = (prompt_tokens as f64) * (prompt_cents_per_m as f64) / 1_000_000.0;
    let completion = (completion_tokens as f64) * (completion_cents_per_m as f64) / 1_000_000.0;
    let total = prompt + completion;
    Ok(Some(total.round() as i64))
}

/// Aggregate `usage_records` into rollup rows.
pub fn cost_rollup(
    conn: &Connection,
    since: Option<DateTime<Utc>>,
    group_by: GroupBy,
) -> Result<Vec<RollupRow>> {
    // Day grouping uses the YYYY-MM-DD prefix of created_at (ISO-8601 UTC).
    let key_expr = match group_by {
        GroupBy::Provider => "provider_id",
        GroupBy::Model => "model",
        GroupBy::Day => "substr(created_at, 1, 10)",
    };
    let since_str = since.map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
    let sql = format!(
        "SELECT {key_expr} AS key, \
                SUM(prompt_tokens) AS pt, \
                SUM(completion_tokens) AS ct, \
                SUM(cost_cents) AS cost, \
                COUNT(*) AS calls \
         FROM usage_records \
         WHERE (?1 IS NULL OR created_at >= ?1) \
         GROUP BY key \
         ORDER BY key"
    );
    let mut stmt = conn.prepare(&sql).context("preparing cost_rollup query")?;
    let rows = stmt
        .query_map(params![since_str], |r| {
            Ok(RollupRow {
                key: r.get::<_, String>(0)?,
                total_prompt_tokens: r.get::<_, i64>(1)?,
                total_completion_tokens: r.get::<_, i64>(2)?,
                total_cost_cents: r.get::<_, Option<i64>>(3)?,
                call_count: r.get::<_, i64>(4)?,
            })
        })
        .context("running cost_rollup query")?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("reading cost_rollup row")?);
    }
    Ok(out)
}
