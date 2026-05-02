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
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::sync::Mutex as AsyncMutex;
use tokio::task::JoinHandle;

use omw_pty::{Pty, PtyCommand, PtySize, PtyWriter};

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

/// Description of an externally-owned session — one whose I/O is provided as
/// channels rather than a `Pty` the registry spawns and owns.
///
/// Used by callers that already manage the underlying transport (e.g. a
/// remote-control bridge) but want the session surfaced through the same
/// `SessionRegistry` lookup/subscribe/write/kill API as owned-Pty sessions.
pub struct ExternalSessionSpec {
    /// Human-readable name. Surfaced in the session list.
    pub name: String,
    /// Sender side of the input mpsc the registry forwards `write_input`
    /// bytes to. The receiver is held by the caller.
    pub input_tx: mpsc::Sender<Vec<u8>>,
    /// Broadcast sender the registry hands out `subscribe()` receivers from.
    /// The caller pushes output chunks into this Sender.
    pub output_tx: broadcast::Sender<Bytes>,
    /// Closure invoked on `kill(id)`. Called at most once.
    pub kill: Box<dyn Fn() + Send + Sync>,
    /// Initial PTY size — recorded for future resize support; v0.4-thin does
    /// NOT plumb resize for external sessions.
    pub initial_size: PtySize,
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

/// Internal state for one live session, regardless of source.
struct SessionEntry {
    name: String,
    created_at: DateTime<Utc>,
    /// Broadcast channel carrying output chunks to any number of subscribers
    /// (WS clients, tests). For owned sessions this is the registry-created
    /// Sender. For external sessions this is the spec-provided Sender.
    output_tx: broadcast::Sender<Bytes>,
    /// Set to `false` when the child is reaped (owned) or `kill` is invoked
    /// (external).
    alive: Arc<AtomicBool>,
    /// Server-side terminal emulator. Every byte that flows through
    /// `record_output` is fed here, and `subscribe_with_state` serializes
    /// the current grid to ANSI for new attachers — tmux-style attach.
    term: Arc<Mutex<vt100::Parser>>,
    /// Per-source state.
    source: SessionSource,
}

/// Parser scrollback line count. The parser's internal scrollback is used
/// for multi-line cursor moves within the visible grid; we don't ship any
/// of it on attach (only `Screen::contents_formatted()` covers the visible
/// viewport). 1000 lines × ~32B/cell × 80 cols ≈ 2.4 MiB per session.
const PARSER_SCROLLBACK_LINES: usize = 1000;

/// Source-specific state for a session entry.
enum SessionSource {
    /// Registry spawned and owns the underlying PTY.
    Owned {
        /// PTY writer, behind a mutex so concurrent `write_input` calls
        /// serialize.
        writer: Arc<AsyncMutex<Option<PtyWriter>>>,
        /// One-shot channel to ask the watcher task to kill the child. Taken
        /// on the first kill request (subsequent kills find `None`).
        kill_tx: Option<oneshot::Sender<()>>,
        /// Watcher task — owns the `Pty`, waits for exit, handles kill
        /// requests. Aborted on registry drop.
        watcher: JoinHandle<()>,
        /// Output pump task — read PTY -> broadcast.
        output_pump: JoinHandle<()>,
    },
    /// Caller owns the underlying transport; registry only routes through
    /// the channels.
    External {
        /// Input bytes from `write_input` are forwarded here.
        input_tx: mpsc::Sender<Vec<u8>>,
        /// Closure invoked on first `kill(id)`. `take()`'d to fire-once.
        kill: Option<Box<dyn Fn() + Send + Sync>>,
        /// Recorded for future resize support; not used in v0.4-thin.
        #[allow(dead_code)]
        initial_size: PtySize,
    },
}

impl Drop for SessionEntry {
    fn drop(&mut self) {
        match &mut self.source {
            SessionSource::Owned {
                kill_tx,
                watcher,
                output_pump,
                ..
            } => {
                // Best-effort: ask the watcher to kill the child if we still
                // have the signal channel (kill() may have already fired it).
                // Either way, abort both background tasks so they don't
                // outlive the entry.
                if let Some(tx) = kill_tx.take() {
                    let _ = tx.send(());
                }
                watcher.abort();
                output_pump.abort();
            }
            SessionSource::External { kill, .. } => {
                // The closure may already have been invoked by kill(); only
                // fire it if it hasn't been taken.
                if let Some(k) = kill.take() {
                    k();
                }
            }
        }
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
    pub async fn register(self: &Arc<Self>, spec: SessionSpec) -> Result<SessionId> {
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
        let term = Arc::new(Mutex::new(vt100::Parser::new(
            rows,
            cols,
            PARSER_SCROLLBACK_LINES,
        )));

        // Output pump: async read loop -> registry.record_output. Routing
        // through record_output (rather than output_tx.send directly) feeds
        // the per-session vt100 parser so attachers can receive a snapshot
        // of the current screen state. Capture an Arc<Self> clone so the
        // pump can call back into the registry.
        let registry_for_pump: Arc<SessionRegistry> = self.clone();
        let id_for_pump = id;
        let output_pump = tokio::spawn(async move {
            let mut buf = vec![0u8; 4096];
            loop {
                match reader.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        let chunk = Bytes::copy_from_slice(&buf[..n]);
                        // record_output is fire-and-forget here: any error
                        // (NotFound after kill) means the session is gone
                        // and the pump should also stop, but the watcher
                        // task will tear us down via abort regardless.
                        let _ = registry_for_pump.record_output(id_for_pump, chunk);
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
            term,
            source: SessionSource::Owned {
                writer: Arc::new(AsyncMutex::new(Some(writer))),
                kill_tx: Some(kill_tx),
                watcher,
                output_pump,
            },
        };

        self.sessions
            .lock()
            .expect("registry mutex poisoned")
            .insert(id, entry);

        Ok(id)
    }

    /// Register an externally-owned session — one whose I/O is provided as
    /// channels and whose lifecycle is driven by a caller-supplied `kill`
    /// closure rather than a `Pty` the registry owns.
    pub async fn register_external(self: &Arc<Self>, spec: ExternalSessionSpec) -> Result<SessionId> {
        let id: SessionId = uuid::Uuid::new_v4();
        let created_at = Utc::now();
        let alive = Arc::new(AtomicBool::new(true));
        let term = Arc::new(Mutex::new(vt100::Parser::new(
            spec.initial_size.rows,
            spec.initial_size.cols,
            PARSER_SCROLLBACK_LINES,
        )));

        let entry = SessionEntry {
            name: spec.name,
            created_at,
            output_tx: spec.output_tx,
            alive,
            term,
            source: SessionSource::External {
                input_tx: spec.input_tx,
                kill: Some(spec.kill),
                initial_size: spec.initial_size,
            },
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

    /// Write `bytes` to the session's input. For owned sessions this writes
    /// to the PTY writer; for external sessions this forwards to the
    /// spec-provided mpsc Sender. Returns [`crate::Error::NotFound`] if the
    /// id is unknown.
    pub async fn write_input(&self, id: SessionId, bytes: &[u8]) -> Result<()> {
        // Two-phase: extract the per-source handle under the sync mutex,
        // then perform the await outside the lock.
        enum InputHandle {
            Owned(Arc<AsyncMutex<Option<PtyWriter>>>),
            External(mpsc::Sender<Vec<u8>>),
        }

        let handle = {
            let map = self.sessions.lock().expect("registry mutex poisoned");
            let entry = map.get(&id).ok_or(Error::NotFound(id))?;
            match &entry.source {
                SessionSource::Owned { writer, .. } => InputHandle::Owned(writer.clone()),
                SessionSource::External { input_tx, .. } => {
                    InputHandle::External(input_tx.clone())
                }
            }
        };

        match handle {
            InputHandle::Owned(writer) => {
                let mut guard = writer.lock().await;
                let w = guard
                    .as_mut()
                    .ok_or_else(|| Error::Io("pty writer already closed".into()))?;
                w.write_all(bytes).await.map_err(Error::from)
            }
            InputHandle::External(input_tx) => input_tx
                .send(bytes.to_vec())
                .await
                .map_err(|_| Error::Io("external session input channel closed".into())),
        }
    }

    /// Subscribe to the session's output stream. Returns `None` if the id is
    /// unknown.
    pub fn subscribe(&self, id: SessionId) -> Option<broadcast::Receiver<Bytes>> {
        let map = self.sessions.lock().expect("registry mutex poisoned");
        map.get(&id).map(|e| e.output_tx.subscribe())
    }

    /// Feed `bytes` into the session's vt100 parser AND broadcast them to
    /// every live subscriber. Returns the broadcast subscriber count, or
    /// [`crate::Error::NotFound`] if the id is unknown.
    ///
    /// The map mutex is held for the whole "feed parser + broadcast" so that
    /// [`subscribe_with_state`] (which also takes the map mutex) sees an
    /// atomic boundary: a chunk is either delivered via the live broadcast
    /// receiver to the new attacher OR captured in the snapshot — never both,
    /// never neither.
    pub fn record_output(&self, id: SessionId, bytes: Bytes) -> Result<usize> {
        let map = self.sessions.lock().expect("registry mutex poisoned");
        let entry = map.get(&id).ok_or(Error::NotFound(id))?;
        // Lock the parser inside the map lock and hold both for the broadcast
        // to keep this point in time atomic with subscribe_with_state.
        {
            let mut term = entry.term.lock().expect("term mutex poisoned");
            term.process(&bytes);
        }
        // broadcast::Sender::send returns Err only when there are zero
        // subscribers; that's not a failure for us — the parser already
        // captured the bytes for any future attacher's snapshot.
        let count = match entry.output_tx.send(bytes) {
            Ok(n) => n,
            Err(_) => 0,
        };
        Ok(count)
    }

    /// Atomic snapshot + subscribe: returns the current ANSI-serialized screen
    /// state plus a fresh broadcast receiver for live updates. Returns `None`
    /// if the id is unknown.
    ///
    /// The map mutex is held for the whole operation so the snapshot bytes
    /// and the broadcast `subscribe()` are taken at the same instant — see
    /// [`record_output`] for the matching producer-side guarantee.
    pub fn subscribe_with_state(
        &self,
        id: SessionId,
    ) -> Option<(Bytes, broadcast::Receiver<Bytes>)> {
        let map = self.sessions.lock().expect("registry mutex poisoned");
        let entry = map.get(&id)?;
        let snapshot = {
            let term = entry.term.lock().expect("term mutex poisoned");
            Bytes::from(term.screen().contents_formatted())
        };
        let rx = entry.output_tx.subscribe();
        Some((snapshot, rx))
    }

    /// Resize the session's vt100 parser screen. Caller is responsible for
    /// also sending the resize to the upstream PTY (the registry doesn't
    /// own a PTY for external sessions). Returns [`crate::Error::NotFound`]
    /// if the id is unknown.
    pub fn resize(&self, id: SessionId, rows: u16, cols: u16) -> Result<()> {
        let map = self.sessions.lock().expect("registry mutex poisoned");
        let entry = map.get(&id).ok_or(Error::NotFound(id))?;
        let mut term = entry.term.lock().expect("term mutex poisoned");
        term.screen_mut().set_size(rows, cols);
        Ok(())
    }

    /// Kill the session and remove it from the registry. For owned sessions
    /// this signals the watcher to kill+reap the child; for external
    /// sessions this invokes the spec-provided `kill` closure.
    /// Idempotent: killing an already-removed id returns
    /// [`crate::Error::NotFound`].
    pub async fn kill(&self, id: SessionId) -> Result<()> {
        let mut entry = {
            let mut map = self.sessions.lock().expect("registry mutex poisoned");
            map.remove(&id).ok_or(Error::NotFound(id))?
        };

        match &mut entry.source {
            SessionSource::Owned { kill_tx, .. } => {
                // Signal the watcher to kill+reap the child. If the watcher
                // has already finished (natural exit), the send is a no-op.
                if let Some(tx) = kill_tx.take() {
                    let _ = tx.send(());
                }
            }
            SessionSource::External { kill, .. } => {
                // Fire the caller-supplied closure exactly once.
                if let Some(k) = kill.take() {
                    k();
                }
            }
        }

        // Mark dead immediately so racing GETs see alive=false.
        entry.alive.store(false, Ordering::SeqCst);

        // Drop the entry. SessionEntry::Drop handles task abort (owned) or
        // is a no-op for external since `kill` was already taken above.
        drop(entry);
        Ok(())
    }
}
