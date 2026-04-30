//! HTTP / WebSocket handlers for the internal session registry.
//!
//! Each submodule corresponds to one route family. The router itself is
//! assembled in [`crate::router`].

pub mod input;
pub mod sessions;
pub mod ws_pty;
