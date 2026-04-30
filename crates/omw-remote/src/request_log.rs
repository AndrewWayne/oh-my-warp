//! Append-only request log. Schema in PRD §10 (`request_log` table).

use chrono::{DateTime, Utc};
use rusqlite::Connection;

/// One row in `request_log`. Mirrors the SQLite schema.
#[derive(Clone, Debug)]
pub struct RequestLogEntry {
    pub route: String,
    pub actor_device_id: Option<String>,
    pub nonce: Option<String>,
    pub ts: DateTime<Utc>,
    pub signature: Option<String>,
    pub body_hash: Option<String>,
    pub accepted: bool,
    pub reason: Option<String>,
}

pub struct RequestLog {
    // db connection
}

impl RequestLog {
    pub fn new(_db: Connection) -> Self {
        unimplemented!("RequestLog::new")
    }

    /// Append a log row. Always succeeds for well-formed entries.
    pub fn append(&self, _entry: RequestLogEntry) -> rusqlite::Result<()> {
        unimplemented!("RequestLog::append")
    }

    /// Read the last `n` rows in insertion order (oldest → newest).
    pub fn tail(&self, _n: u32) -> rusqlite::Result<Vec<RequestLogEntry>> {
        unimplemented!("RequestLog::tail")
    }
}
