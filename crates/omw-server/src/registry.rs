//! In-memory registry of live PTY sessions.
//!
//! Owns the `omw_pty::Pty` for each session and a `tokio::sync::broadcast`
//! channel that fans output bytes out to one or more WebSocket subscribers.
//!
//! Thread-safe: registry methods take `&self` (interior mutability via
//! `Mutex`).

use std::collections::{HashMap, VecDeque};
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

/// Capacity (in bytes) of the per-session output scrollback ring buffer. When
/// exceeded, oldest chunks are dropped first. v0.4-thin Stage C: lets a phone
/// that disconnects (e.g. user puts the phone away to wait for a long-running
/// command) reattach later and see what happened in the interim, instead of
/// joining a fresh broadcast that only carries bytes from the moment of
/// reconnection forward.
///
/// 64 KiB is enough for several screens of typical shell output without
/// dragging memory usage. Long-running TUI apps that repaint constantly will
/// roll the ring quickly — that's fine; the user wants the *last* state, not
/// the full session history. Tunable later via [`ServerConfig`] if needed.
const SCROLLBACK_BYTE_CAPACITY: usize = 64 * 1024;

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

/// Bounded byte ring buffer used to replay recent output to a late-attaching
/// subscriber. Stores `Bytes` chunks (no copying) and tracks total byte count
/// so eviction is O(amortised 1). Not Sync on its own — callers wrap in
/// `Mutex` and access under the registry's map mutex for cross-method
/// atomicity.
struct ScrollbackBuf {
    chunks: VecDeque<Bytes>,
    total_bytes: usize,
    cap_bytes: usize,
}

impl ScrollbackBuf {
    fn new(cap_bytes: usize) -> Self {
        Self {
            chunks: VecDeque::new(),
            total_bytes: 0,
            cap_bytes,
        }
    }

    /// Append one chunk. Drops oldest chunks until the total byte count is
    /// within cap. If the chunk itself exceeds the cap, retain only it (the
    /// most recent state is what the user wants).
    fn push(&mut self, chunk: Bytes) {
        if chunk.len() >= self.cap_bytes {
            self.chunks.clear();
            self.total_bytes = chunk.len();
            self.chunks.push_back(chunk);
            return;
        }
        self.total_bytes += chunk.len();
        self.chunks.push_back(chunk);
        while self.total_bytes > self.cap_bytes {
            if let Some(front) = self.chunks.pop_front() {
                self.total_bytes -= front.len();
            } else {
                break;
            }
        }
    }

    /// Snapshot of the current chunks for replay to a fresh subscriber.
    ///
    /// **TUI flicker / duplicate-output mitigation.** Naively replaying every
    /// recorded byte causes the phone's xterm to render every historical
    /// state in sequence — for a TUI session the user perceives this as a
    /// "static printline" of the pre-TUI plain text appearing first, then the
    /// TUI redraw repainting the same content on top. So we trim everything
    /// BEFORE the latest screen-state reset (alt-screen toggle / full clear /
    /// RIS) — those escapes wipe the visible terminal state, so anything
    /// before is cosmetic noise. After the trim, xterm's first replay byte
    /// is the reset itself, and from there we just paint the latest live
    /// state with no pre-history flicker.
    ///
    /// For a non-TUI session (no reset escape ever sent — e.g. the user only
    /// ran `ls`/`cat`/`echo`) no trim happens and the full byte stream is
    /// returned as-is. That preserves the actual scrollback the user wants.
    fn snapshot(&self) -> Vec<Bytes> {
        // Collapse to a single contiguous buffer so the trim search can find
        // escape sequences that straddle chunk boundaries. Bounded to
        // `cap_bytes`, so allocation is cheap.
        let total_len = self.total_bytes;
        if total_len == 0 {
            return Vec::new();
        }
        let mut buf = Vec::with_capacity(total_len);
        for chunk in &self.chunks {
            buf.extend_from_slice(chunk);
        }
        if let Some(reset_at) = find_last_screen_state_reset(&buf) {
            return vec![Bytes::copy_from_slice(&buf[reset_at..])];
        }
        // No reset found — return one coalesced chunk. Coalescing here gives
        // the WS handler one Output frame to enqueue, which xterm processes
        // as one `xterm.write` call (atomic enough that the user sees a
        // single transition rather than a per-chunk flicker).
        vec![Bytes::from(buf)]
    }
}

/// Find the byte offset of the LAST screen-state-establishing escape sequence
/// in `bytes`, returning `None` if none exists. "Establishes screen state"
/// here means an escape after which xterm's visible state is fully
/// determined by the bytes that follow (so anything BEFORE the escape is
/// cosmetic noise on replay):
///
///   - `\x1b[2J` / `\x1b[3J`         Erase in Display
///   - `\x1bc`                       RIS — Reset to Initial State
///   - `\x1b[?1049h` / `\x1b[?1049l` alt-screen enter / exit
///   - `\x1b[?47h`   / `\x1b[?47l`   legacy alt-screen enter / exit
///   - `\x1b[?2026h`                 BEGIN synchronized output — TUI apps
///                                   like Claude Code emit this at the start
///                                   of every atomic redraw frame. Trimming
///                                   to the LAST `2026h` gives the latest
///                                   sync-output frame and discards the
///                                   pre-TUI plain-text intro that would
///                                   otherwise replay as a "static printline"
///                                   before the TUI overdraws it.
///
/// We do a linear forward scan and remember the LAST hit; for ≤ 64 KiB this
/// is microseconds and avoids tricky reverse-scanning of multi-byte
/// sequences that may straddle chunk boundaries.
fn find_last_screen_state_reset(bytes: &[u8]) -> Option<usize> {
    let n = bytes.len();
    let mut last: Option<usize> = None;
    let mut i = 0;
    while i < n {
        if bytes[i] != 0x1b {
            i += 1;
            continue;
        }
        // ESC c — Full Reset
        if matches!(bytes.get(i + 1), Some(b'c')) {
            last = Some(i);
            i += 2;
            continue;
        }
        // ESC [ ...
        if matches!(bytes.get(i + 1), Some(b'[')) {
            // ESC [ 2 J  /  ESC [ 3 J
            if (matches!(bytes.get(i + 2), Some(b'2') | Some(b'3')))
                && matches!(bytes.get(i + 3), Some(b'J'))
            {
                last = Some(i);
                i += 4;
                continue;
            }
            // ESC [ ? 1 0 4 9 h  /  ESC [ ? 1 0 4 9 l
            if matches!(bytes.get(i + 2), Some(b'?'))
                && matches!(bytes.get(i + 3), Some(b'1'))
                && matches!(bytes.get(i + 4), Some(b'0'))
                && matches!(bytes.get(i + 5), Some(b'4'))
                && matches!(bytes.get(i + 6), Some(b'9'))
                && matches!(bytes.get(i + 7), Some(b'h') | Some(b'l'))
            {
                last = Some(i);
                i += 8;
                continue;
            }
            // ESC [ ? 4 7 h  /  ESC [ ? 4 7 l  — legacy alt screen
            if matches!(bytes.get(i + 2), Some(b'?'))
                && matches!(bytes.get(i + 3), Some(b'4'))
                && matches!(bytes.get(i + 4), Some(b'7'))
                && matches!(bytes.get(i + 5), Some(b'h') | Some(b'l'))
            {
                last = Some(i);
                i += 6;
                continue;
            }
            // ESC [ ? 2 0 2 6 h — BEGIN synchronized output (Claude Code et al.).
            // We deliberately ignore the matching `l` (END) — only the BEGIN
            // marks "the latest atomic redraw frame starts here." Trimming
            // to the END would discard the frame's content too.
            if matches!(bytes.get(i + 2), Some(b'?'))
                && matches!(bytes.get(i + 3), Some(b'2'))
                && matches!(bytes.get(i + 4), Some(b'0'))
                && matches!(bytes.get(i + 5), Some(b'2'))
                && matches!(bytes.get(i + 6), Some(b'6'))
                && matches!(bytes.get(i + 7), Some(b'h'))
            {
                last = Some(i);
                i += 8;
                continue;
            }
        }
        i += 1;
    }
    last
}

/// Internal state for one live session, regardless of source.
struct SessionEntry {
    name: String,
    created_at: DateTime<Utc>,
    /// Broadcast channel carrying output chunks to any number of subscribers
    /// (WS clients, tests). For owned sessions this is the registry-created
    /// Sender. For external sessions this is the spec-provided Sender.
    output_tx: broadcast::Sender<Bytes>,
    /// Bounded recent-output ring buffer. v0.4-thin Stage C: replayed to new
    /// subscribers via [`SessionRegistry::subscribe_with_scrollback`] so a
    /// phone that disconnects and reattaches sees recent output instead of a
    /// blank screen.
    ///
    /// Populated by [`SessionRegistry::record_output`] for external sessions
    /// (called by the pane_share output pump in warp-oss) and by the owned
    /// session's internal output pump for registry-spawned PTYs. Direct
    /// `output_tx.send(...)` calls (legacy / test path) bypass the scrollback
    /// — that's fine for the existing ws_via_external_session test which
    /// doesn't exercise scrollback.
    scrollback: Arc<Mutex<ScrollbackBuf>>,
    /// Set to `false` when the child is reaped (owned) or `kill` is invoked
    /// (external).
    alive: Arc<AtomicBool>,
    /// Per-source state.
    source: SessionSource,
}

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
        let scrollback = Arc::new(Mutex::new(ScrollbackBuf::new(SCROLLBACK_BYTE_CAPACITY)));

        // Output pump: async read loop into scrollback + broadcast. Order
        // (scrollback push BEFORE broadcast send) matches `record_output` so
        // a `subscribe_with_scrollback` call concurrent with output sees a
        // chunk EITHER in the snapshot OR on the live receiver — never both
        // and never neither.
        let output_tx_pump = output_tx.clone();
        let scrollback_for_pump = scrollback.clone();
        let output_pump = tokio::spawn(async move {
            let mut buf = vec![0u8; 4096];
            loop {
                match reader.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        let chunk = Bytes::copy_from_slice(&buf[..n]);
                        scrollback_for_pump
                            .lock()
                            .expect("scrollback poisoned")
                            .push(chunk.clone());
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
            scrollback,
            alive,
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
    pub async fn register_external(&self, spec: ExternalSessionSpec) -> Result<SessionId> {
        let id: SessionId = uuid::Uuid::new_v4();
        let created_at = Utc::now();
        let alive = Arc::new(AtomicBool::new(true));
        let scrollback = Arc::new(Mutex::new(ScrollbackBuf::new(SCROLLBACK_BYTE_CAPACITY)));

        let entry = SessionEntry {
            name: spec.name,
            created_at,
            output_tx: spec.output_tx,
            scrollback,
            alive,
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

    /// Subscribe AND atomically snapshot the recent-output ring buffer so the
    /// caller can replay past output before the live broadcast catches up.
    ///
    /// Returns `(scrollback_chunks, live_receiver)` or `None` if the id is
    /// unknown. The two halves are taken under the same map mutex that
    /// [`record_output`] uses, so a chunk recorded concurrently is delivered
    /// either via the snapshot or via the live receiver — never both, never
    /// neither.
    ///
    /// **Important contract for callers:** if a producer pushes bytes via
    /// [`broadcast::Sender::send`] directly (instead of through
    /// [`record_output`]), those bytes go to the live receiver but are NOT
    /// captured in scrollback. The legacy `output_tx`-direct-send path used
    /// by some tests works fine for live delivery; only the production
    /// pane-share path needs scrollback semantics, and that path uses
    /// [`record_output`].
    pub fn subscribe_with_scrollback(
        &self,
        id: SessionId,
    ) -> Option<(Vec<Bytes>, broadcast::Receiver<Bytes>)> {
        let map = self.sessions.lock().expect("registry mutex poisoned");
        let entry = map.get(&id)?;
        let snapshot = entry
            .scrollback
            .lock()
            .expect("scrollback poisoned")
            .snapshot();
        let rx = entry.output_tx.subscribe();
        Some((snapshot, rx))
    }

    /// Record one chunk of output for a session: appends to the recent-output
    /// ring buffer AND broadcasts to live subscribers. Atomic with respect to
    /// [`subscribe_with_scrollback`] (both take the registry's map mutex).
    ///
    /// Returns the number of subscribers the broadcast send reached, or
    /// [`crate::Error::NotFound`] if the id is unknown. A `0` return is not
    /// an error — the registry stores the bytes in scrollback for any future
    /// subscribers.
    ///
    /// Used by the warp-oss pane-share output pump (and by the owned-session
    /// output pump internally). Direct `output_tx.send(...)` still works for
    /// legacy callers but those bytes won't appear in scrollback.
    pub fn record_output(&self, id: SessionId, chunk: Bytes) -> Result<usize> {
        let map = self.sessions.lock().expect("registry mutex poisoned");
        let entry = map.get(&id).ok_or(Error::NotFound(id))?;
        entry
            .scrollback
            .lock()
            .expect("scrollback poisoned")
            .push(chunk.clone());
        // `send` returns Err(SendError) only when there are zero subscribers;
        // the chunk is already in scrollback so that's fine — count of 0.
        let n = entry.output_tx.send(chunk).unwrap_or(0);
        Ok(n)
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

#[cfg(test)]
mod scrollback_tests {
    use super::*;

    #[test]
    fn find_last_screen_state_reset_returns_none_when_no_reset() {
        assert_eq!(find_last_screen_state_reset(b"hello world\n"), None);
        // ESC followed by some non-reset CSI should not match.
        assert_eq!(find_last_screen_state_reset(b"\x1b[31mred\x1b[0m"), None);
    }

    #[test]
    fn find_last_screen_state_reset_finds_clear_screen() {
        let bytes = b"intro text\x1b[2Jcleared content";
        let pos = find_last_screen_state_reset(bytes).expect("must find");
        // 10 = len("intro text"); ESC starts there.
        assert_eq!(pos, 10);
    }

    #[test]
    fn find_last_screen_state_reset_finds_alt_screen_enter() {
        let bytes = b"plain\x1b[?1049htui-content";
        let pos = find_last_screen_state_reset(bytes).expect("must find");
        assert_eq!(pos, 5);
    }

    #[test]
    fn find_last_screen_state_reset_finds_sync_output_begin() {
        // The Claude Code case: pre-TUI plain text, then a sync-output
        // frame BEGIN. Trim should return the offset of the BEGIN.
        let bytes = b"Welcome to Claude Code\n\x1b[?2026hframe-content";
        let pos = find_last_screen_state_reset(bytes).expect("must find");
        assert_eq!(pos, 23); // len("Welcome to Claude Code\n") = 23
    }

    #[test]
    fn find_last_screen_state_reset_picks_latest_sync_frame_begin() {
        // Multiple sync-output frames — typical of a long-running TUI session.
        // We want the LATEST `\x1b[?2026h`, since each frame is a full
        // redraw and the latest one represents the current visible state.
        let bytes =
            b"intro\x1b[?2026hframe1\x1b[?2026l\x1b[?2026hframe2\x1b[?2026l\x1b[?2026hframe3";
        let pos = find_last_screen_state_reset(bytes).expect("must find");
        // bytes through end of frame2's `\x1b[?2026l`, the next `\x1b[?2026h`
        // is the start of frame3 — that's the trim point.
        assert_eq!(&bytes[pos..pos + 8], b"\x1b[?2026h");
    }

    #[test]
    fn find_last_screen_state_reset_ignores_sync_output_end() {
        // `\x1b[?2026l` alone (no preceding BEGIN in this test slice) is NOT
        // a screen-state-reset — only the BEGIN counts.
        let bytes = b"foo\x1b[?2026lbar";
        assert_eq!(find_last_screen_state_reset(bytes), None);
    }

    #[test]
    fn find_last_screen_state_reset_returns_latest_when_multiple() {
        // Two clears: a `[2J` early and a `[?1049h` later. The alt-screen
        // enter is the LATEST, so its offset is what we expect to trim from.
        // `a` (1) `\x1b[2J` (4) `b` (1) -> the second ESC starts at byte 6.
        let bytes = b"a\x1b[2Jb\x1b[?1049hc";
        let pos = find_last_screen_state_reset(bytes).expect("must find");
        assert_eq!(pos, 6);
    }

    #[test]
    fn snapshot_trims_before_last_clear_when_present() {
        let mut buf = ScrollbackBuf::new(1024);
        buf.push(Bytes::from_static(b"intro line\n"));
        buf.push(Bytes::from_static(b"more intro\n\x1b[?1049h"));
        buf.push(Bytes::from_static(b"tui body"));
        let snap = buf.snapshot();
        // One coalesced chunk that starts at the alt-screen enter escape.
        assert_eq!(snap.len(), 1);
        assert_eq!(&snap[0][..], b"\x1b[?1049htui body");
    }

    #[test]
    fn snapshot_coalesces_when_no_reset_in_buffer() {
        let mut buf = ScrollbackBuf::new(1024);
        buf.push(Bytes::from_static(b"line 1\n"));
        buf.push(Bytes::from_static(b"line 2\n"));
        buf.push(Bytes::from_static(b"line 3\n"));
        let snap = buf.snapshot();
        // No clear -> coalesce all three chunks into one Bytes.
        assert_eq!(snap.len(), 1);
        assert_eq!(&snap[0][..], b"line 1\nline 2\nline 3\n");
    }

    #[test]
    fn snapshot_handles_empty_buffer() {
        let buf = ScrollbackBuf::new(1024);
        assert!(buf.snapshot().is_empty());
    }

    #[test]
    fn snapshot_handles_reset_split_across_chunks() {
        // The escape sequence straddles two chunks — coalescing inside
        // snapshot is what makes this find-able.
        let mut buf = ScrollbackBuf::new(1024);
        buf.push(Bytes::from_static(b"intro\x1b"));
        buf.push(Bytes::from_static(b"[?1049htui"));
        let snap = buf.snapshot();
        assert_eq!(snap.len(), 1);
        assert_eq!(&snap[0][..], b"\x1b[?1049htui");
    }
}
