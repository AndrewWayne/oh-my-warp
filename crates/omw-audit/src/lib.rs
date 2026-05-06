//! `omw-audit` — append-only hash-chained audit log writer.
//!
//! Per [PRD §8.3](../../../PRD.md#83-component-ownership-map), audit log
//! writes are owned by `omw-server` and serialized through a single
//! [`AuditWriter`] behind a `tokio::sync::Mutex`. Other crates (omw-agent
//! over JSON-RPC, omw-remote over HTTP) post events to omw-server's
//! `/api/v1/audit/append` endpoint; nobody else writes the file directly.
//!
//! ## File layout
//!
//! `~/.local/share/omw/audit/YYYY-MM-DD.jsonl`. One JSON object per
//! line, terminated with `\n`. The writer rolls into a fresh file at the
//! local-time day boundary; the chain is contiguous *across* days
//! because the first entry of day N+1 carries the SHA-256 of day N's
//! last entry as its `prev_hash`.
//!
//! ## Hash chain
//!
//! Each entry's `hash` field is the SHA-256 (lowercase hex) of:
//!
//! ```text
//! prev_hash_hex || canonical_json(entry_without_hash)
//! ```
//!
//! `canonical_json` reconstructs the JSON value through a `BTreeMap` so
//! key order is deterministic (no new dep). This yields a stable byte
//! string regardless of how the caller built the `fields` payload.
//!
//! `verify_chain(path)` re-runs the chain from a given starting hash
//! and asserts every line's recorded `hash` matches the recomputed one.
//! Tampering surfaces as a `VerifyError::HashMismatch { line, .. }`.
//!
//! ## What we don't do
//!
//! - No redaction. The caller (omw-server's `/api/v1/audit/append`
//!   handler) owns the redaction pass — `redaction_rules` is a v0.2
//!   PRD deliverable not yet built. v1 audit files may contain command
//!   output verbatim.
//! - No retention / compaction. Files accumulate indefinitely.
//! - No encryption at rest. PRD §11.4 puts that on disk-level encryption.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Local, NaiveDate, Utc};
use serde::Serialize;
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

/// All-zero SHA-256 hex placeholder for the very first entry of the
/// audit log on a fresh install.
pub const GENESIS_PREV_HASH: &str =
    "0000000000000000000000000000000000000000000000000000000000000000";

#[derive(Debug, Error)]
pub enum AuditError {
    #[error("audit io: {0}")]
    Io(#[from] std::io::Error),
    #[error("audit serialize: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("audit corrupted: {0}")]
    Corrupted(String),
}

#[derive(Debug, Error)]
pub enum VerifyError {
    #[error("audit verify io: {0}")]
    Io(#[from] std::io::Error),
    #[error("audit verify parse: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("audit malformed line {line}: {msg}")]
    Malformed { line: usize, msg: String },
    #[error("audit hash mismatch on line {line}: expected {expected}, got {actual}")]
    HashMismatch {
        line: usize,
        expected: String,
        actual: String,
    },
    #[error("audit prev-hash mismatch on line {line}: expected {expected}, got {actual}")]
    PrevHashMismatch {
        line: usize,
        expected: String,
        actual: String,
    },
}

/// Append-only writer for a single audit log file (one calendar day).
///
/// Construction reads the existing file (if any) to discover the
/// `prev_hash` that the next append must extend. Across day boundaries,
/// callers should construct a new `AuditWriter` whose `prev_hash` is
/// seeded from yesterday's last entry — see [`AuditWriter::reopen`].
pub struct AuditWriter {
    audit_dir: PathBuf,
    day: NaiveDate,
    file: File,
    prev_hash: String,
}

impl AuditWriter {
    /// Open today's audit file under `audit_dir`. Creates the directory
    /// + file if missing. Reads the existing tail to discover
    /// `prev_hash`. Cross-day rollover: if `audit_dir` contains a file
    /// for the previous day, its last `hash` is used as our seed
    /// `prev_hash` (so the chain spans days).
    pub fn open(audit_dir: impl Into<PathBuf>) -> Result<Self, AuditError> {
        let audit_dir = audit_dir.into();
        std::fs::create_dir_all(&audit_dir)?;

        let day = Local::now().date_naive();
        Self::open_for_day(audit_dir, day)
    }

    /// Like [`Self::open`] but pinned to a specific date — useful for
    /// tests that want a deterministic file path without depending on
    /// the system clock.
    pub fn open_for_day(audit_dir: PathBuf, day: NaiveDate) -> Result<Self, AuditError> {
        std::fs::create_dir_all(&audit_dir)?;
        let path = audit_dir.join(format!("{day}.jsonl"));
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(&path)?;

        let prev_hash = match read_last_hash(&mut file)? {
            Some(h) => h,
            None => seed_from_previous_day(&audit_dir, day)?,
        };
        // After read, the file cursor is at EOF (read_last_hash leaves
        // it there); appends go to the right place.
        file.seek(SeekFrom::End(0))?;

        Ok(Self {
            audit_dir,
            day,
            file,
            prev_hash,
        })
    }

    /// Convenience for callers that want to rotate the writer at the
    /// day boundary without recomputing the audit dir.
    pub fn reopen(self) -> Result<Self, AuditError> {
        Self::open(self.audit_dir.clone())
    }

    /// Return the path of the currently-open file. Useful for tests +
    /// verification.
    pub fn current_path(&self) -> PathBuf {
        self.audit_dir.join(format!("{}.jsonl", self.day))
    }

    /// Return the audit directory.
    pub fn audit_dir(&self) -> &Path {
        &self.audit_dir
    }

    /// Currently-pinned `prev_hash`. Public so tests and tooling can
    /// inspect the chain head without re-reading the file.
    pub fn prev_hash(&self) -> &str {
        &self.prev_hash
    }

    /// Append one entry. Returns the entry's `hash` (the new chain
    /// head).
    ///
    /// Day rollover is **not** automatic — the writer keeps appending
    /// to whatever file it was opened with. Higher layers (omw-server)
    /// detect the local-time day boundary and call [`Self::reopen`].
    /// Reasoning: writers may be opened with a pinned date for tests,
    /// auto-rollover that reads the system clock would silently switch
    /// to a different file mid-test.
    pub fn append(
        &mut self,
        kind: &str,
        session_id: Uuid,
        fields: serde_json::Value,
    ) -> Result<String, AuditError> {
        let ts = Utc::now();
        let body = AuditEntryBody {
            ts,
            kind: kind.to_string(),
            session_id,
            fields,
            prev_hash: self.prev_hash.clone(),
        };
        let body_canonical = canonical_json(&serde_json::to_value(&body)?)?;

        let mut hasher = Sha256::new();
        hasher.update(self.prev_hash.as_bytes());
        hasher.update(body_canonical.as_bytes());
        let hash_hex = hex_lower(&hasher.finalize());

        // Re-emit the body with the `hash` field appended. Use canonical
        // JSON for the on-disk form so `verify_chain` can recompute the
        // exact bytes we hashed. We achieve canonicality by going through
        // BTreeMap in `canonical_json`.
        let mut entry_value = serde_json::to_value(&body)?;
        if let serde_json::Value::Object(ref mut map) = entry_value {
            map.insert("hash".into(), serde_json::Value::String(hash_hex.clone()));
        }
        let line = canonical_json(&entry_value)?;

        self.file.write_all(line.as_bytes())?;
        self.file.write_all(b"\n")?;
        self.file.flush()?;

        self.prev_hash = hash_hex.clone();
        Ok(hash_hex)
    }
}

#[derive(Serialize)]
struct AuditEntryBody {
    ts: DateTime<Utc>,
    kind: String,
    session_id: Uuid,
    fields: serde_json::Value,
    prev_hash: String,
}

fn read_last_hash(file: &mut File) -> Result<Option<String>, AuditError> {
    file.seek(SeekFrom::Start(0))?;
    let reader = BufReader::new(&mut *file);
    let mut last: Option<String> = None;
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let v: serde_json::Value = serde_json::from_str(&line)?;
        let hash = v
            .get("hash")
            .and_then(|h| h.as_str())
            .ok_or_else(|| AuditError::Corrupted("entry missing `hash`".into()))?;
        last = Some(hash.to_string());
    }
    file.seek(SeekFrom::End(0))?;
    Ok(last)
}

fn seed_from_previous_day(audit_dir: &Path, day: NaiveDate) -> Result<String, AuditError> {
    // Walk back day-by-day until we find an existing audit file or
    // we've gone too far. We cap the walk at 31 days (one month) to
    // avoid an unbounded loop on a fresh install.
    let mut probe = day;
    for _ in 0..31 {
        probe = probe.pred_opt().ok_or_else(|| {
            AuditError::Corrupted("date arithmetic underflowed; this should not happen".into())
        })?;
        let path = audit_dir.join(format!("{probe}.jsonl"));
        if path.exists() {
            let mut f = OpenOptions::new().read(true).open(&path)?;
            if let Some(h) = read_last_hash(&mut f)? {
                return Ok(h);
            }
        }
    }
    Ok(GENESIS_PREV_HASH.to_string())
}

/// Re-walk a single audit file and verify every line's `hash` field
/// matches the recomputed SHA-256 of `prev_hash || canonical_json(body)`,
/// and that successive lines' `prev_hash` match the previous line's
/// `hash`. Returns Ok on a clean chain.
///
/// `seed_prev_hash` is the chain head expected before the first line.
/// Pass [`GENESIS_PREV_HASH`] for a fresh chain or yesterday's last
/// hash for a cross-day verify.
pub fn verify_chain(path: &Path, seed_prev_hash: &str) -> Result<String, VerifyError> {
    let f = OpenOptions::new().read(true).open(path)?;
    let reader = BufReader::new(f);
    let mut prev_hash = seed_prev_hash.to_string();
    let mut last_hash = prev_hash.clone();

    for (idx, line) in reader.lines().enumerate() {
        let line = line?;
        let line_no = idx + 1;
        if line.trim().is_empty() {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(&line)?;
        let recorded_hash = value
            .get("hash")
            .and_then(|v| v.as_str())
            .ok_or_else(|| VerifyError::Malformed {
                line: line_no,
                msg: "entry missing `hash`".into(),
            })?
            .to_string();
        let recorded_prev = value
            .get("prev_hash")
            .and_then(|v| v.as_str())
            .ok_or_else(|| VerifyError::Malformed {
                line: line_no,
                msg: "entry missing `prev_hash`".into(),
            })?
            .to_string();

        if recorded_prev != prev_hash {
            return Err(VerifyError::PrevHashMismatch {
                line: line_no,
                expected: prev_hash.clone(),
                actual: recorded_prev,
            });
        }

        // Strip `hash` to recompute. The remaining body must canonicalize
        // identically to what we wrote at append time.
        let mut body_value = value.clone();
        if let serde_json::Value::Object(ref mut map) = body_value {
            map.remove("hash");
        }
        let body_canonical = canonical_json(&body_value).map_err(|e| VerifyError::Malformed {
            line: line_no,
            msg: format!("canonicalize: {e}"),
        })?;
        let mut hasher = Sha256::new();
        hasher.update(prev_hash.as_bytes());
        hasher.update(body_canonical.as_bytes());
        let recomputed = hex_lower(&hasher.finalize());

        if recomputed != recorded_hash {
            return Err(VerifyError::HashMismatch {
                line: line_no,
                expected: recomputed,
                actual: recorded_hash,
            });
        }

        prev_hash = recorded_hash.clone();
        last_hash = recorded_hash;
    }

    Ok(last_hash)
}

/// Canonicalize a JSON value: deterministic key order via BTreeMap
/// reconstruction; no other transforms (numbers, strings, arrays
/// preserve their inputs exactly).
fn canonical_json(v: &serde_json::Value) -> Result<String, AuditError> {
    let canon = canonicalize(v);
    serde_json::to_string(&canon).map_err(AuditError::from)
}

fn canonicalize(v: &serde_json::Value) -> serde_json::Value {
    match v {
        serde_json::Value::Object(map) => {
            let mut sorted = std::collections::BTreeMap::new();
            for (k, vv) in map {
                sorted.insert(k.clone(), canonicalize(vv));
            }
            serde_json::Value::Object(sorted.into_iter().collect())
        }
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.iter().map(canonicalize).collect())
        }
        other => other.clone(),
    }
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let hi = (b >> 4) & 0xF;
        let lo = b & 0xF;
        s.push(hex_digit(hi));
        s.push(hex_digit(lo));
    }
    s
}

const fn hex_digit(n: u8) -> char {
    match n {
        0..=9 => (b'0' + n) as char,
        10..=15 => (b'a' + (n - 10)) as char,
        _ => '?',
    }
}
