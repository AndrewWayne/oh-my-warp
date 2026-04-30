//! In-memory registry of live PTY sessions.
//!
//! Owns the `omw_pty::Pty` for each session and a `tokio::sync::broadcast`
//! channel that fans output bytes out to one or more WebSocket subscribers.
//!
//! Thread-safe: registry methods take `&self` (interior mutability via
//! `Mutex`).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tokio::sync::oneshot;
use tokio::sync::Mutex as AsyncMutex;
use tokio::task::JoinHandle;

use omw_pty::{Pty, PtyCommand, PtyWriter};

use crate::{Error, Result};

/// Stable identifier for a session. UUID v4.
pub type SessionId = uuid::Uuid;

/// Capacity (in chunks) of the per-session output broadcast channel.
const OUTPUT_BROADCAST_CAPACITY: usize = 256;

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
#[allow(clippy::manual_non_exhaustive)]
pub struct Session {
    /// Stable id assigned at registration time.
    pub id: SessionId,
    /// Human-readable name from the [`SessionSpec`].
    pub name: String,
    /// Wall-clock time at registration.
    pub created_at: DateTime<Utc>,
    _impl: (),
}

/// Internal state for one live session.
struct SessionEntry {
    name: String,
    created_at: DateTime<Utc>,
    /// Broadcast channel carrying PTY output chunks to any number of
    /// subscribers (WS clients, registry tests).
    output_tx: broadcast::Sender<Bytes>,
    /// Set to `false` when the watcher reaps the child.
    alive: Arc<AtomicBool>,
    /// PTY writer, behind a mutex so concurrent `write_input` calls serialize.
    writer: Arc<AsyncMutex<Option<PtyWriter>>>,
    /// One-shot channel to ask the watcher task to kill the child. Taken on
    /// the first kill request (subsequent kills find `None`).
    kill_tx: Option<oneshot::Sender<()>>,
    /// Watcher task — owns the `Pty`, waits for exit, handles kill requests.
    /// Aborted on registry drop.
    watcher: JoinHandle<()>,
    /// Output pump task — read PTY -> broadcast.
    output_pump: JoinHandle<()>,
}

impl Drop for SessionEntry {
    fn drop(&mut self) {
        // Best-effort: ask the watcher to kill the child if we still have the
        // signal channel (kill() may have already fired it). Either way, abort
        // both background tasks so they don't outlive the entry.
        if let Some(tx) = self.kill_tx.take() {
            let _ = tx.send(());
        }
        self.watcher.abort();
        self.output_pump.abort();
    }
}

/// Thread-safe registry of live sessions. Cheap to clone via `Arc`.
pub struct SessionRegistry {
    sessions: Mutex<HashMap<SessionId, SessionEntry>>,
}

impl SessionRegistry {
    /// Construct an empty registry, wrapped in `Arc` so it can be shared
    /// across the axum router state and any number of background tasks.
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            sessions: Mutex::new(HashMap::new()),
        })
    }

    /// Register a new session: spawn the PTY, start the output pump, store
    /// the entry, return its assigned id.
    pub async fn register(&self, spec: SessionSpec) -> Result<SessionId> {
        let mut cmd = PtyCommand::new(&spec.command).args(spec.args.iter().map(|s| s.as_str()));
        if let Some(cwd) = &spec.cwd {
            cmd = cmd.cwd(cwd.clone());
        }
        if let Some(env) = &spec.env {
            for (k, v) in env {
                cmd = cmd.env(k, v);
            }
        }
        let cols = spec.cols.unwrap_or(80);
        let rows = spec.rows.unwrap_or(24);
        cmd = cmd.size(cols, rows);

        let mut pty = Pty::spawn(cmd).await.map_err(Error::from)?;
        let mut reader = pty
            .reader()
            .ok_or_else(|| Error::Io("pty reader already taken".into()))?;
        let writer = pty
            .writer()
            .ok_or_else(|| Error::Io("pty writer already taken".into()))?;

        let id: SessionId = uuid::Uuid::new_v4();
        let created_at = Utc::now();
        let alive = Arc::new(AtomicBool::new(true));
        let (output_tx, _output_rx0) = broadcast::channel::<Bytes>(OUTPUT_BROADCAST_CAPACITY);

        // Output pump: async read loop into the broadcast.
        let output_tx_pump = output_tx.clone();
        let output_pump = tokio::spawn(async move {
            let mut buf = vec![0u8; 4096];
            loop {
                match reader.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        let chunk = Bytes::copy_from_slice(&buf[..n]);
                        // `send` returns Err only when there are zero
                        // subscribers; that's fine — keep pumping so the
                        // broadcast is ready for late subscribers.
                        let _ = output_tx_pump.send(chunk);
                    }
                    Err(_) => break,
                }
            }
        });

        // Watcher task owns the Pty. It selects between:
        //   - `pty.wait()`     — natural-exit path
        //   - `kill_rx` future — explicit kill from `SessionRegistry::kill`
        // Either way, once the wait returns, flip `alive` to false. The Pty
        // is dropped at the end of the task; that triggers Pty::Drop which
        // joins the underlying threads.
        let (kill_tx, kill_rx) = oneshot::channel::<()>();
        let alive_for_watcher = alive.clone();
        let watcher = tokio::spawn(async move {
            tokio::select! {
                _ = pty.wait() => {}
                _ = kill_rx => {
                    let _ = pty.kill();
                    let _ = pty.wait().await;
                }
            }
            alive_for_watcher.store(false, Ordering::SeqCst);
            // pty drops here, joining its internal threads.
            drop(pty);
        });

        let entry = SessionEntry {
            name: spec.name,
            created_at,
            output_tx,
            alive,
            writer: Arc::new(AsyncMutex::new(Some(writer))),
            kill_tx: Some(kill_tx),
            watcher,
            output_pump,
        };

        self.sessions
            .lock()
            .expect("registry mutex poisoned")
            .insert(id, entry);

        Ok(id)
    }

    /// Snapshot of every registered session's metadata. Order is not
    /// guaranteed; callers should sort if they need stability.
    pub fn list(&self) -> Vec<SessionMeta> {
        let map = self.sessions.lock().expect("registry mutex poisoned");
        map.iter()
            .map(|(id, e)| SessionMeta {
                id: *id,
                name: e.name.clone(),
                created_at: e.created_at,
                alive: e.alive.load(Ordering::SeqCst),
            })
            .collect()
    }

    /// Metadata for a single session, or `None` if no such id is registered.
    pub fn get(&self, id: SessionId) -> Option<SessionMeta> {
        let map = self.sessions.lock().expect("registry mutex poisoned");
        map.get(&id).map(|e| SessionMeta {
            id,
            name: e.name.clone(),
            created_at: e.created_at,
            alive: e.alive.load(Ordering::SeqCst),
        })
    }

    /// Write `bytes` to the session's PTY input. Returns
    /// [`crate::Error::NotFound`] if the id is unknown.
    pub async fn write_input(&self, id: SessionId, bytes: &[u8]) -> Result<()> {
        let writer = {
            let map = self.sessions.lock().expect("registry mutex poisoned");
            let entry = map.get(&id).ok_or(Error::NotFound(id))?;
            entry.writer.clone()
        };
        let mut guard = writer.lock().await;
        let w = guard
            .as_mut()
            .ok_or_else(|| Error::Io("pty writer already closed".into()))?;
        w.write_all(bytes).await.map_err(Error::from)
    }

    /// Subscribe to the session's output stream. Returns `None` if the id is
    /// unknown.
    pub fn subscribe(&self, id: SessionId) -> Option<broadcast::Receiver<Bytes>> {
        let map = self.sessions.lock().expect("registry mutex poisoned");
        map.get(&id).map(|e| e.output_tx.subscribe())
    }

    /// Kill the session's child and remove it from the registry.
    /// Idempotent: killing an already-removed id returns
    /// [`crate::Error::NotFound`].
    pub async fn kill(&self, id: SessionId) -> Result<()> {
        let mut entry = {
            let mut map = self.sessions.lock().expect("registry mutex poisoned");
            map.remove(&id).ok_or(Error::NotFound(id))?
        };

        // Signal the watcher to kill+reap the child. If the watcher has
        // already finished (natural exit), the send is a no-op.
        if let Some(tx) = entry.kill_tx.take() {
            let _ = tx.send(());
        }

        // Mark dead immediately so racing GETs see alive=false.
        entry.alive.store(false, Ordering::SeqCst);

        // Drop the entry. SessionEntry::Drop aborts both tasks. The watcher
        // owns the Pty; its abort drops Pty, which kills the child if still
        // alive (Pty::Drop) and joins the underlying threads.
        drop(entry);
        Ok(())
    }
}
