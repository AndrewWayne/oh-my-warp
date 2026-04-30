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
    _private: (),
}

impl PtyReader {
    /// Read up to `buf.len()` bytes from the PTY master. Returns 0 on EOF
    /// (child closed its end / exited).
    pub async fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
        unimplemented!("Executor: implement async read from PTY master")
    }
}

/// Owned write half of a PTY master.
pub struct PtyWriter {
    _private: (),
}

impl PtyWriter {
    /// Write all bytes to the PTY master, returning when the buffer has been
    /// fully accepted by the OS PTY driver.
    pub async fn write_all(&mut self, _buf: &[u8]) -> Result<()> {
        unimplemented!("Executor: implement async write_all to PTY master")
    }

    /// Flush any internal buffering. Pre-fork this is a no-op; declared
    /// in case the bridge buffers writes through a channel.
    pub async fn flush(&mut self) -> Result<()> {
        unimplemented!("Executor: implement flush")
    }
}

/// A PTY master + child handle.
pub struct Pty {
    _private: (),
}

impl Pty {
    /// Spawn `cmd` attached to a freshly allocated PTY pair.
    ///
    /// On success, the returned `Pty` owns the child handle and the master
    /// side. The slave side is closed in the parent immediately after spawn.
    pub async fn spawn(_cmd: PtyCommand) -> Result<Self> {
        unimplemented!("Executor: spawn child via portable_pty under PTY")
    }

    /// Take the read half. Idempotent contract: returns `Some` once, then
    /// `None` on subsequent calls.
    pub fn reader(&mut self) -> Option<PtyReader> {
        unimplemented!("Executor: hand out PtyReader once")
    }

    /// Take the write half. Same once-only contract as [`Pty::reader`].
    pub fn writer(&mut self) -> Option<PtyWriter> {
        unimplemented!("Executor: hand out PtyWriter once")
    }

    /// Resize the PTY window. `cols == 0 || rows == 0` is forwarded to
    /// portable-pty; the wrapper does NOT pre-validate (tests pin actual
    /// observed behavior).
    pub fn resize(&self, _cols: u16, _rows: u16) -> Result<()> {
        unimplemented!("Executor: forward to MasterPty::resize")
    }

    /// Send a kill signal to the child. On Unix this is SIGKILL; on Windows
    /// it terminates the process. Idempotent if the child has already exited.
    pub fn kill(&mut self) -> Result<()> {
        unimplemented!("Executor: kill child")
    }

    /// Await the child's exit. Resolves immediately if already exited.
    pub async fn wait(&mut self) -> Result<ExitStatus> {
        unimplemented!("Executor: wait on child via spawn_blocking")
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        // Executor: ensure no thread/task leaks and no hang on drop.
        // No-op stub here; drop semantics are pinned by tests/lifecycle_drop.rs.
    }
}
