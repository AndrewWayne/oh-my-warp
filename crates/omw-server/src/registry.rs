//! In-memory registry of live PTY sessions.
//!
//! Owns the `omw_pty::Pty` for each session and a `tokio::sync::broadcast`
//! channel that fans output bytes out to one or more WebSocket subscribers.
//!
//! Thread-safe: registry methods take `&self` (interior mutability via
//! `Mutex` / `RwLock`).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use bytes::Bytes;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::Result;

/// Stable identifier for a session. UUID v4.
pub type SessionId = uuid::Uuid;

/// Description of a session to register. Mirrors the JSON body of
/// `POST /internal/v1/sessions` after deserialization.
#[derive(Debug, Clone, Deserialize)]
pub struct SessionSpec {
    /// Human-readable name. Surfaced in the session list.
    pub name: String,
    /// Program to spawn (looked up via the platform's PATH resolution).
    pub command: String,
    /// Arguments passed to `command`, in order.
    #[serde(default)]
    pub args: Vec<String>,
    /// Optional working directory for the child. `None` inherits the parent's.
    #[serde(default)]
    pub cwd: Option<PathBuf>,
    /// Environment overrides MERGED on top of the parent env at spawn time.
    #[serde(default)]
    pub env: Option<HashMap<String, String>>,
    /// Initial PTY column count. Defaults to 80 when `None`.
    #[serde(default)]
    pub cols: Option<u16>,
    /// Initial PTY row count. Defaults to 24 when `None`.
    #[serde(default)]
    pub rows: Option<u16>,
}

/// Public metadata for a session — what `GET /sessions` and `GET /sessions/:id`
/// surface. Does NOT include the PTY handles or output broadcast channel.
#[derive(Debug, Clone, Serialize)]
pub struct SessionMeta {
    pub id: SessionId,
    pub name: String,
    pub created_at: DateTime<Utc>,
    /// `true` if the child process has not yet exited.
    pub alive: bool,
}

/// One live session: PTY + output broadcast channel + metadata.
///
/// The struct itself is opaque to consumers; they interact via
/// [`SessionRegistry`]. It exists as a public type only so tests / callers can
/// construct registries that hold them.
pub struct Session {
    /// Stable id assigned at registration time.
    pub id: SessionId,
    /// Human-readable name from the [`SessionSpec`].
    pub name: String,
    /// Wall-clock time at registration.
    pub created_at: DateTime<Utc>,
    // The PTY handle itself, the output broadcast channel, and any
    // bookkeeping fields are intentionally left as Executor work — the
    // exact representation is implementation-defined. Tests interact with
    // sessions only through the registry surface.
    _impl: (),
}

/// Thread-safe registry of live sessions. Cheap to clone via `Arc`.
pub struct SessionRegistry {
    // Implementation-defined. Likely:
    //   sessions: tokio::sync::Mutex<HashMap<SessionId, SessionEntry>>,
    // where `SessionEntry` carries the Pty, broadcast::Sender<Bytes>, and a
    // task handle pumping bytes from the PtyReader into the broadcast.
    _impl: (),
}

impl SessionRegistry {
    /// Construct an empty registry, wrapped in `Arc` so it can be shared
    /// across the axum router state and any number of background tasks.
    pub fn new() -> Arc<Self> {
        unimplemented!("SessionRegistry::new (Executor)")
    }

    /// Register a new session: spawn the PTY, start the output pump, store
    /// the entry, return its assigned id.
    pub async fn register(&self, spec: SessionSpec) -> Result<SessionId> {
        let _ = spec;
        unimplemented!("SessionRegistry::register (Executor)")
    }

    /// Snapshot of every registered session's metadata. Order is not
    /// guaranteed; callers should sort if they need stability.
    pub fn list(&self) -> Vec<SessionMeta> {
        unimplemented!("SessionRegistry::list (Executor)")
    }

    /// Metadata for a single session, or `None` if no such id is registered.
    pub fn get(&self, id: SessionId) -> Option<SessionMeta> {
        let _ = id;
        unimplemented!("SessionRegistry::get (Executor)")
    }

    /// Write `bytes` to the session's PTY input. Returns
    /// [`crate::Error::NotFound`] if the id is unknown.
    pub async fn write_input(&self, id: SessionId, bytes: &[u8]) -> Result<()> {
        let _ = (id, bytes);
        unimplemented!("SessionRegistry::write_input (Executor)")
    }

    /// Subscribe to the session's output stream. Returns `None` if the id is
    /// unknown. Each subscriber receives a copy of every output chunk;
    /// subscribers that lag past the broadcast capacity will see `Lagged`
    /// errors on `recv`.
    pub fn subscribe(&self, id: SessionId) -> Option<broadcast::Receiver<Bytes>> {
        let _ = id;
        unimplemented!("SessionRegistry::subscribe (Executor)")
    }

    /// Kill the session's child and remove it from the registry.
    /// Idempotent: killing an already-removed id returns
    /// [`crate::Error::NotFound`].
    pub async fn kill(&self, id: SessionId) -> Result<()> {
        let _ = id;
        unimplemented!("SessionRegistry::kill (Executor)")
    }
}
