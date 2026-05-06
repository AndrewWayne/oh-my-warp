//! Agent surface — spawns the omw-agent stdio process and bridges its
//! JSON-RPC frames between the GUI WebSocket clients and the kernel.
//!
//! See [`process::AgentProcess`] for the central type. This module ships
//! Phase 2 of the inline-agent stack — text-only sessions, no tools yet.
//! Approval / audit / bash routing arrive in Phases 4 and 5.

pub mod process;

pub use process::{AgentProcess, AgentProcessConfig, AgentProcessError};
