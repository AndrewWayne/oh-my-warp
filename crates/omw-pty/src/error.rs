//! Error types for `omw-pty`.

#[derive(Debug, thiserror::Error)]
pub enum PtyError {
    /// Failed to spawn the child process attached to the PTY.
    #[error("failed to spawn pty child: {0}")]
    Spawn(String),

    /// I/O error reading from or writing to the PTY master.
    #[error("pty io error: {0}")]
    Io(#[from] std::io::Error),

    /// Failed to resize the PTY.
    #[error("failed to resize pty: {0}")]
    Resize(String),

    /// Failed to wait on / reap the child process.
    #[error("failed to wait for pty child: {0}")]
    Wait(String),

    /// Failed to kill / signal the child process.
    #[error("failed to kill pty child: {0}")]
    Kill(String),
}

pub type Result<T> = std::result::Result<T, PtyError>;
