//! HTTP / WebSocket handlers for the internal session registry and the
//! agent surface.
//!
//! Each submodule corresponds to one route family. Routers are assembled
//! in [`crate::router`] (PTY registry) and [`crate::agent_router`]
//! (agent endpoints).

pub mod agent;
pub mod audit;
pub mod input;
pub mod sessions;
pub mod ws_pty;
