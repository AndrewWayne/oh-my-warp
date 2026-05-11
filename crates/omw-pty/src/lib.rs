//! `omw-pty` — async PTY wrapper around [`portable-pty`].
//!
//! Public surface:
//! - [`Pty::spawn`] launches a child attached to a freshly allocated PTY.
//! - [`Pty::reader`] / [`Pty::writer`] return owned async halves for the
//!   master side. Splitting them lets callers move the reader into one task
//!   and the writer into another.
//! - [`Pty::resize`] adjusts the window size (sends SIGWINCH on Unix; ConPTY
//!   resize on Windows).
//! - [`Pty::kill`] terminates the child; [`Pty::wait`] awaits its exit.
//! - Dropping a [`Pty`] without explicit kill/wait must not hang or leak.
//!
//! See [PRD §8.2](../../../PRD.md#82-components).

pub mod command;
pub mod error;

pub use command::{PtyCommand, PtySize};
pub use error::{PtyError, Result};

use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc as std_mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize as PpSize};
use tokio::sync::{mpsc, oneshot, watch};

/// Message sent from `PtyWriter` to the writer thread. The oneshot ack lets
/// `write_all` / `flush` await confirmation that bytes have actually reached
/// the PTY (or the operation failed).
enum WriterMsg {
    Write(Vec<u8>, oneshot::Sender<io::Result<()>>),
    Flush(oneshot::Sender<io::Result<()>>),
}

/// Exit status of a PTY child. Thin wrapper so we don't leak `portable_pty`
/// types across the crate boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExitStatus {
    /// The numeric exit code, if the platform/portable-pty exposed one.
    /// On Unix, `None` means "terminated by signal".
    pub code: Option<i32>,
    /// Whether the platform considers this a successful exit (code == 0).
    pub success: bool,
}

impl ExitStatus {
    pub fn success(&self) -> bool {
        self.success
    }

    pub fn code(&self) -> Option<i32> {
        self.code
    }
}

/// Owned read half of a PTY master.
///
/// Reads are async; under the hood the implementation runs the blocking
/// `portable_pty::MasterPty::try_clone_reader` reader on a dedicated
/// thread/task and pipes bytes through a channel.
pub struct PtyReader {
    rx: mpsc::UnboundedReceiver<io::Result<Vec<u8>>>,
    /// Leftover bytes from the last chunk that didn't fit in the caller's buf.
    leftover: Vec<u8>,
    /// Position into `leftover` of the first un-yielded byte.
    leftover_pos: usize,
    /// Set to true once we've seen an EOF / error so we keep returning it.
    closed: bool,
}

impl PtyReader {
    /// Read up to `buf.len()` bytes from the PTY master. Returns 0 on EOF
    /// (child closed its end / exited).
    pub async fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }

        // First, drain leftover bytes from a previous chunk.
        if self.leftover_pos < self.leftover.len() {
            let avail = self.leftover.len() - self.leftover_pos;
            let n = avail.min(buf.len());
            buf[..n].copy_from_slice(&self.leftover[self.leftover_pos..self.leftover_pos + n]);
            self.leftover_pos += n;
            if self.leftover_pos >= self.leftover.len() {
                self.leftover.clear();
                self.leftover_pos = 0;
            }
            return Ok(n);
        }

        if self.closed {
            return Ok(0);
        }

        match self.rx.recv().await {
            None => {
                // Channel closed without an explicit EOF marker: treat as EOF.
                self.closed = true;
                Ok(0)
            }
            Some(Ok(chunk)) => {
                if chunk.is_empty() {
                    // EOF marker.
                    self.closed = true;
                    return Ok(0);
                }
                let n = chunk.len().min(buf.len());
                buf[..n].copy_from_slice(&chunk[..n]);
                if n < chunk.len() {
                    self.leftover = chunk;
                    self.leftover_pos = n;
                }
                Ok(n)
            }
            Some(Err(e)) => {
                self.closed = true;
                Err(e)
            }
        }
    }
}

/// Owned write half of a PTY master.
pub struct PtyWriter {
    tx: Option<std_mpsc::Sender<WriterMsg>>,
}

impl PtyWriter {
    /// Write all bytes to the PTY master, returning when the buffer has been
    /// fully accepted by the OS PTY driver.
    pub async fn write_all(&mut self, buf: &[u8]) -> Result<()> {
        if buf.is_empty() {
            return Ok(());
        }
        let tx = self.tx.as_ref().ok_or_else(|| {
            PtyError::Io(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "writer is closed",
            ))
        })?;
        let (ack_tx, ack_rx) = oneshot::channel();
        tx.send(WriterMsg::Write(buf.to_vec(), ack_tx))
            .map_err(|_| {
                PtyError::Io(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "writer thread has exited",
                ))
            })?;
        match ack_rx.await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(PtyError::Io(e)),
            Err(_) => Err(PtyError::Io(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "writer thread dropped ack before responding",
            ))),
        }
    }

    /// Flush any internal buffering. Returns once the writer thread has
    /// flushed the underlying PTY writer.
    pub async fn flush(&mut self) -> Result<()> {
        let tx = self.tx.as_ref().ok_or_else(|| {
            PtyError::Io(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "writer is closed",
            ))
        })?;
        let (ack_tx, ack_rx) = oneshot::channel();
        tx.send(WriterMsg::Flush(ack_tx)).map_err(|_| {
            PtyError::Io(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "writer thread has exited",
            ))
        })?;
        match ack_rx.await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(PtyError::Io(e)),
            Err(_) => Err(PtyError::Io(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "writer thread dropped ack before responding",
            ))),
        }
    }
}

/// Slot for the master PTY, shared across the Pty handle and the watcher
/// thread so the watcher can drop the master once the child has exited
/// (this is what produces an EOF on the reader thread's blocking read,
/// especially on Windows ConPTY where exit alone does not close the pipe).
type MasterSlot = Arc<Mutex<Option<Box<dyn MasterPty + Send>>>>;

/// A PTY master + child handle.
pub struct Pty {
    /// Shared slot for the master so the watcher thread can drop it on
    /// child exit. Pty::resize locks this; Pty::Drop empties it.
    master: MasterSlot,
    /// Separate ChildKiller; usable concurrently with the watcher thread
    /// blocking in child.wait().
    killer: Option<Box<dyn ChildKiller + Send + Sync>>,
    /// Shared "child reaped" flag. Set by the watcher thread once child.wait()
    /// returns. Read by Pty::kill to short-circuit to Ok after exit.
    child_reaped: Arc<std::sync::atomic::AtomicBool>,
    /// Watch channel carrying the ExitStatus once the watcher has reaped the
    /// child. Pty::wait subscribes to this.
    status_rx: watch::Receiver<Option<ExitStatus>>,
    /// One-shot reader half, set on spawn(), taken on first reader().
    reader: Option<PtyReader>,
    /// One-shot writer half, set on spawn(), taken on first writer().
    writer: Option<PtyWriter>,
    /// Internal copy of the writer-channel sender so dropping the user-facing
    /// PtyWriter alone does not close the channel.
    writer_tx: Option<std_mpsc::Sender<WriterMsg>>,
    /// Stop flag that the writer thread polls between `recv_timeout` calls.
    /// Setting this guarantees the writer thread exits within ~100ms even if
    /// an external `PtyWriter` is still alive holding a `Sender` clone.
    writer_stop: Arc<AtomicBool>,
    /// Reader thread join handle.
    reader_thread: Option<thread::JoinHandle<()>>,
    /// Writer thread join handle.
    writer_thread: Option<thread::JoinHandle<()>>,
    /// Watcher thread join handle. Waits on the child and drops the master
    /// when it exits.
    watcher_thread: Option<thread::JoinHandle<()>>,
}

impl Pty {
    /// Spawn `cmd` attached to a freshly allocated PTY pair.
    ///
    /// On success, the returned `Pty` owns the child handle and the master
    /// side. The slave side is closed in the parent immediately after spawn.
    pub async fn spawn(cmd: PtyCommand) -> Result<Self> {
        // Building the PTY pair and spawning the child are blocking syscalls.
        // Run them on a blocking thread to avoid stalling the runtime.
        let pty_handle = tokio::task::spawn_blocking(move || -> Result<_> {
            let pty_system = native_pty_system();
            let pair = pty_system
                .openpty(PpSize {
                    rows: cmd.size.rows,
                    cols: cmd.size.cols,
                    pixel_width: 0,
                    pixel_height: 0,
                })
                .map_err(|e| PtyError::Spawn(format!("openpty: {e}")))?;

            let mut command = CommandBuilder::new(&cmd.program);
            command.args(&cmd.args);
            for k in &cmd.env_removes {
                command.env_remove(k);
            }
            for (k, v) in &cmd.envs {
                command.env(k, v);
            }
            if let Some(cwd) = &cmd.cwd {
                command.cwd(cwd);
            }

            let child = pair
                .slave
                .spawn_command(command)
                .map_err(|e| PtyError::Spawn(format!("spawn_command: {e}")))?;
            let killer = child.clone_killer();
            let reader_box = pair
                .master
                .try_clone_reader()
                .map_err(|e| PtyError::Spawn(format!("try_clone_reader: {e}")))?;
            let writer_box = pair
                .master
                .take_writer()
                .map_err(|e| PtyError::Spawn(format!("take_writer: {e}")))?;

            // Drop slave side in the parent so the child's stdin/stdout pipe is
            // fully hooked up and EOF works correctly when we drop the master.
            drop(pair.slave);

            Ok((pair.master, child, killer, reader_box, writer_box))
        })
        .await
        .map_err(|e| PtyError::Spawn(format!("spawn_blocking join: {e}")))??;

        let (master, child, killer, mut reader_box, mut writer_box) = pty_handle;

        // Reader thread: blocking read into Vec<u8> chunks; push through
        // tokio mpsc so the async side can poll them.
        let (reader_tx, reader_rx) = mpsc::unbounded_channel::<io::Result<Vec<u8>>>();
        let reader_thread = thread::Builder::new()
            .name("omw-pty-reader".into())
            .spawn(move || {
                let mut buf = vec![0u8; 4096];
                loop {
                    match reader_box.read(&mut buf) {
                        Ok(0) => {
                            let _ = reader_tx.send(Ok(Vec::new()));
                            break;
                        }
                        Ok(n) => {
                            if reader_tx.send(Ok(buf[..n].to_vec())).is_err() {
                                break;
                            }
                        }
                        Err(_e) => {
                            // I/O error (typically because the master was
                            // dropped or the child exited). Treat as EOF —
                            // the watcher thread will provide the exit
                            // status separately.
                            let _ = reader_tx.send(Ok(Vec::new()));
                            break;
                        }
                    }
                }
            })
            .expect("spawn reader thread");

        // Writer thread: blocking recv from std mpsc; blocking write to the PTY.
        // Polls a stop flag between recv_timeout calls so Drop can force exit
        // even when an external PtyWriter still holds a Sender clone.
        let (writer_tx, writer_rx) = std_mpsc::channel::<WriterMsg>();
        let writer_tx_internal = writer_tx.clone();
        let writer_stop = Arc::new(AtomicBool::new(false));
        let writer_stop_thread = writer_stop.clone();
        let writer_thread = thread::Builder::new()
            .name("omw-pty-writer".into())
            .spawn(move || {
                loop {
                    if writer_stop_thread.load(Ordering::SeqCst) {
                        break;
                    }
                    match writer_rx.recv_timeout(Duration::from_millis(50)) {
                        Ok(WriterMsg::Write(chunk, ack)) => {
                            let res = writer_box
                                .write_all(&chunk)
                                .and_then(|()| writer_box.flush());
                            let failed = res.is_err();
                            let _ = ack.send(res);
                            if failed {
                                break;
                            }
                        }
                        Ok(WriterMsg::Flush(ack)) => {
                            let res = writer_box.flush();
                            let failed = res.is_err();
                            let _ = ack.send(res);
                            if failed {
                                break;
                            }
                        }
                        Err(std_mpsc::RecvTimeoutError::Timeout) => {
                            // Loop and re-check stop flag.
                        }
                        Err(std_mpsc::RecvTimeoutError::Disconnected) => {
                            break;
                        }
                    }
                }
            })
            .expect("spawn writer thread");

        // Master goes into a shared slot so the watcher thread can drop it
        // when the child exits, which is what triggers EOF on the reader.
        let master_slot: MasterSlot = Arc::new(Mutex::new(Some(master)));
        let child_reaped = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (status_tx, status_rx) = watch::channel::<Option<ExitStatus>>(None);

        // Watcher thread: blocking child.wait(); on exit, drop the master so
        // the reader thread's blocking read returns EOF. Publish the status
        // through a watch channel so Pty::wait can await it.
        let watcher_master = master_slot.clone();
        let watcher_reaped = child_reaped.clone();
        let watcher_thread = thread::Builder::new()
            .name("omw-pty-watcher".into())
            .spawn(move || {
                let mut child = child;
                let res = child.wait();
                watcher_reaped.store(true, std::sync::atomic::Ordering::SeqCst);
                let status = match res {
                    Ok(pp_status) => ExitStatus {
                        code: exit_code_from(&pp_status),
                        success: pp_status.success(),
                    },
                    Err(_) => ExitStatus {
                        code: None,
                        success: false,
                    },
                };
                // Drop the master so the reader thread can unblock and EOF.
                if let Ok(mut guard) = watcher_master.lock() {
                    guard.take();
                }
                let _ = status_tx.send(Some(status));
            })
            .expect("spawn watcher thread");

        let reader = PtyReader {
            rx: reader_rx,
            leftover: Vec::new(),
            leftover_pos: 0,
            closed: false,
        };
        let writer = PtyWriter {
            tx: Some(writer_tx),
        };

        Ok(Self {
            master: master_slot,
            killer: Some(killer),
            child_reaped,
            status_rx,
            reader: Some(reader),
            writer: Some(writer),
            writer_tx: Some(writer_tx_internal),
            writer_stop,
            reader_thread: Some(reader_thread),
            writer_thread: Some(writer_thread),
            watcher_thread: Some(watcher_thread),
        })
    }

    /// Take the read half. Idempotent contract: returns `Some` once, then
    /// `None` on subsequent calls.
    pub fn reader(&mut self) -> Option<PtyReader> {
        self.reader.take()
    }

    /// Take the write half. Same once-only contract as [`Pty::reader`].
    pub fn writer(&mut self) -> Option<PtyWriter> {
        self.writer.take()
    }

    /// Resize the PTY window. `cols == 0 || rows == 0` is forwarded to
    /// portable-pty; the wrapper does NOT pre-validate (tests pin actual
    /// observed behavior).
    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        let guard = self
            .master
            .lock()
            .map_err(|_| PtyError::Resize("master lock poisoned".into()))?;
        let master = guard
            .as_ref()
            .ok_or_else(|| PtyError::Resize("master already dropped (child exited)".into()))?;
        master
            .resize(PpSize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| PtyError::Resize(e.to_string()))
    }

    /// Send a kill signal to the child. Idempotent: a second call after the
    /// child has already exited returns Ok.
    pub fn kill(&mut self) -> Result<()> {
        if self.child_reaped.load(std::sync::atomic::Ordering::SeqCst) {
            return Ok(());
        }
        if let Some(killer) = self.killer.as_mut() {
            match killer.kill() {
                Ok(()) => Ok(()),
                Err(e) => {
                    if is_already_terminated(&e) {
                        Ok(())
                    } else {
                        Err(PtyError::Kill(e.to_string()))
                    }
                }
            }
        } else {
            Ok(())
        }
    }

    /// Await the child's exit. Resolves immediately if already exited.
    pub async fn wait(&mut self) -> Result<ExitStatus> {
        // Fast path: already published.
        if let Some(s) = *self.status_rx.borrow() {
            return Ok(s);
        }
        // Wait for the watcher thread to publish the exit status.
        loop {
            if self.status_rx.changed().await.is_err() {
                return Err(PtyError::Wait(
                    "watcher thread exited without publishing status".into(),
                ));
            }
            if let Some(s) = *self.status_rx.borrow() {
                return Ok(s);
            }
        }
    }
}

/// Map a portable_pty::ExitStatus into our `Option<i32>` code.
///
/// - On signal-kill (Unix), portable_pty stores `code = 1` and `Some(signal)`,
///   and Display starts with "Terminated by ". We surface `None`.
/// - On normal exit, return Some(code as i32).
fn exit_code_from(status: &portable_pty::ExitStatus) -> Option<i32> {
    let s = format!("{}", status);
    if s.starts_with("Terminated by ") {
        None
    } else {
        Some(status.exit_code() as i32)
    }
}

/// Best-effort detection of "process is already gone" for kill idempotency.
///
/// Narrow on purpose: only conditions whose interpretation under our PTY
/// stack is "the process is no longer there (or shortly will not be)" map
/// to Ok. Genuine privilege/IO failures must propagate as `PtyError::Kill`.
///
/// - All platforms: `NotFound` (ESRCH on Unix; ERROR_NOT_FOUND wrapped on
///   Windows).
/// - Windows: explicit raw-OS-error matches. Identified by raw code, not by
///   the broad `io::ErrorKind`, so unrelated `InvalidInput` /
///   `PermissionDenied` / `TimedOut` errors propagate.
///   - 5    (ERROR_ACCESS_DENIED) — handle to an exited process can yield this
///   - 6    (ERROR_INVALID_HANDLE) — process was reaped, handle is stale
///   - 87   (ERROR_INVALID_PARAMETER) — observed from portable-pty / ConPTY
///     when the underlying job-object termination races with child exit
///     on Windows; the kill request takes effect regardless
///   - 1460 (ERROR_TIMEOUT) — TerminateProcess after exit
///   - 1168 (ERROR_NOT_FOUND)
fn is_already_terminated(err: &io::Error) -> bool {
    if err.kind() == io::ErrorKind::NotFound {
        return true;
    }
    #[cfg(windows)]
    {
        if let Some(code) = err.raw_os_error() {
            if matches!(code, 5 | 6 | 87 | 1460 | 1168) {
                return true;
            }
        }
    }
    false
}

impl Drop for Pty {
    fn drop(&mut self) {
        // 1. Try to kill the child if still alive. Ignore errors.
        if !self.child_reaped.load(std::sync::atomic::Ordering::SeqCst) {
            if let Some(killer) = self.killer.as_mut() {
                let _ = killer.kill();
            }
        }

        // 2. Signal the writer thread to stop, then drop all senders we hold.
        //    The stop flag ensures the thread exits within ~100ms even if an
        //    external `PtyWriter` is still alive holding its own Sender clone.
        self.writer_stop.store(true, Ordering::SeqCst);
        self.writer_tx.take();
        if let Some(w) = self.writer.as_mut() {
            w.tx.take();
        }

        // 3. Drop the master PTY (if still present). On Windows ConPTY this
        //    is what unblocks the reader's blocking read with EOF/error.
        //    The watcher thread may also have already taken it; that's fine.
        if let Ok(mut guard) = self.master.lock() {
            guard.take();
        }

        // 4. Join the watcher thread first — it owns the child handle. Bound
        //    the wait so a stuck wait() on a process that refuses to die does
        //    not hang Drop forever.
        if let Some(h) = self.watcher_thread.take() {
            join_with_timeout(h, Duration::from_millis(2500));
        }

        // 5. Join reader & writer threads. With master dropped and writer
        //    channel closed, both should exit promptly.
        if let Some(h) = self.reader_thread.take() {
            join_with_timeout(h, Duration::from_millis(1000));
        }
        if let Some(h) = self.writer_thread.take() {
            join_with_timeout(h, Duration::from_millis(1000));
        }
    }
}

/// Join a thread, but give up after `deadline`. We can't actually interrupt
/// a stuck thread on stable Rust, so on timeout we simply leak the handle.
/// Callers use this only on Drop, where leaking is preferable to hanging.
fn join_with_timeout(handle: thread::JoinHandle<()>, deadline: Duration) {
    let start = Instant::now();
    // Probe for completion via a short busy-loop. JoinHandle has no
    // "try_join", so we spawn a helper that signals via a channel. To avoid
    // an extra thread, we use the simplest approach: spawn a tiny waiter
    // that joins and signals; if it times out, we detach.
    let (done_tx, done_rx) = std_mpsc::channel::<()>();
    let waiter = thread::spawn(move || {
        let _ = handle.join();
        let _ = done_tx.send(());
    });
    while start.elapsed() < deadline {
        if done_rx.recv_timeout(Duration::from_millis(50)).is_ok() {
            let _ = waiter.join();
            return;
        }
    }
    // Timed out — detach. The waiter thread will eventually finish (or not)
    // independently.
    drop(done_rx);
    drop(waiter);
}
