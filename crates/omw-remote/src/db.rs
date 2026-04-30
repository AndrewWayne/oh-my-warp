//! SQLite connection helper + migration application.
//!
//! Schema lives in `crates/omw-remote/migrations/`. The Phase D migration
//! creates the `devices`, `pairings`, and `request_log` tables per PRD §10.

use std::path::Path;

use rusqlite::Connection;

const INIT_SQL: &str = include_str!("../migrations/2026-05-01-init.sql");
const SCHEMA_VERSION: u32 = 1;

/// Open or create the SQLite db at `path` and apply all pending migrations.
/// Returns a connection ready for use with `Pairings::new` / `RequestLog::new`.
pub fn open_db(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch(INIT_SQL)?;
    conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    Ok(conn)
}

/// Highest migration applied to `db`. Used by tests to assert migration ordering.
pub fn schema_version(db: &Connection) -> rusqlite::Result<u32> {
    db.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
        .map(|v| v as u32)
}
