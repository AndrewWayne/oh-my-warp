//! WebSocket framing for `omw-remote`. See `specs/byorc-protocol.md` §7.

pub mod auth;
pub mod frame;
pub mod pty;

pub use auth::WsSessionAuth;
pub use frame::{Frame, FrameAuthError, FrameError, FrameKind};
pub use pty::ShellSpec;
