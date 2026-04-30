//! Append-only request log. Schema in PRD §10 (`request_log` table).

use std::sync::Mutex;

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};

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
    db: Mutex<Connection>,
}

impl RequestLog {
    pub fn new(db: Connection) -> Self {
        Self { db: Mutex::new(db) }
    }

    /// Append a log row. Always succeeds for well-formed entries.
    pub fn append(&self, entry: RequestLogEntry) -> rusqlite::Result<()> {
        let db = self.db.lock().expect("request log poisoned");
        db.execute(
            "INSERT INTO request_log \
             (route, actor_device_id, nonce, ts, signature, body_hash, accepted, reason) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                entry.route,
                entry.actor_device_id,
                entry.nonce,
                entry.ts.to_rfc3339(),
                entry.signature,
                entry.body_hash,
                if entry.accepted { 1_i64 } else { 0_i64 },
                entry.reason,
            ],
        )?;
        Ok(())
    }

    /// Read the last `n` rows in insertion order (oldest → newest).
    pub fn tail(&self, n: u32) -> rusqlite::Result<Vec<RequestLogEntry>> {
        let db = self.db.lock().expect("request log poisoned");
        let mut stmt = db.prepare(
            "SELECT route, actor_device_id, nonce, ts, signature, body_hash, accepted, reason \
             FROM (SELECT * FROM request_log ORDER BY id DESC LIMIT ?1) \
             ORDER BY id ASC",
        )?;
        let rows = stmt.query_map(params![n], |row| {
            let ts_str: String = row.get(3)?;
            let accepted: i64 = row.get(6)?;
            let ts = DateTime::parse_from_rfc3339(&ts_str)
                .map(|d| d.with_timezone(&Utc))
                .map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        3,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
            Ok(RequestLogEntry {
                route: row.get(0)?,
                actor_device_id: row.get(1)?,
                nonce: row.get(2)?,
                ts,
                signature: row.get(4)?,
                body_hash: row.get(5)?,
                accepted: accepted != 0,
                reason: row.get(7)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }
}
