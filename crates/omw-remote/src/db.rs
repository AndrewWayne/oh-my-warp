//! SQLite connection helper + migration application.
//!
//! Schema lives in `crates/omw-remote/migrations/`. The Phase D migration
//! creates the `devices`, `pairings`, and `request_log` tables per PRD §10.

use std::path::Path;

use rusqlite::Connection;

/// Open or create the SQLite db at `path` and apply all pending migrations.
/// Returns a connection ready for use with `Pairings::new` / `RequestLog::new`.
pub fn open_db(_path: &Path) -> rusqlite::Result<Connection> {
    unimplemented!("open_db")
}

/// Highest migration applied to `db`. Used by tests to assert migration ordering.
pub fn schema_version(_db: &Connection) -> rusqlite::Result<u32> {
    unimplemented!("schema_version")
}
