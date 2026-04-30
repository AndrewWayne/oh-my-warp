//! Error types for `omw-server`.

/// Errors surfaced from the session registry surface.
///
/// These are mapped to HTTP status codes by the handlers:
/// - [`Error::NotFound`]   → 404
/// - [`Error::Spawn`]      → 500
/// - [`Error::Io`]         → 500
/// - [`Error::BadRequest`] → 400
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// No session with the given id is registered.
    #[error("session not found: {0}")]
    NotFound(crate::SessionId),

    /// Failed to spawn the PTY child for a new session.
    #[error("failed to spawn pty: {0}")]
    Spawn(String),

    /// Underlying PTY I/O failure (write, kill, etc.).
    #[error("pty io error: {0}")]
    Io(String),

    /// Caller-provided input was malformed (e.g. invalid base64, bad JSON).
    #[error("bad request: {0}")]
    BadRequest(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl From<omw_pty::PtyError> for Error {
    fn from(e: omw_pty::PtyError) -> Self {
        match e {
            omw_pty::PtyError::Spawn(s) => Error::Spawn(s),
            other => Error::Io(other.to_string()),
        }
    }
}
