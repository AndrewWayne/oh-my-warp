//! omw integration module.
//!
//! Wired by Wiring 5 ("Combined Overseer + Executor wiring pass"). Hosts the
//! launcher state for the embedded `omw-remote` daemon, which the agent footer
//! "Remote Control" button starts/stops.
//!
//! Gated behind the `omw_local` feature so non-omw_local builds (if any) stay
//! clean. See `vendor/warp-stripped/OMW_LOCAL_BUILD.md`.

pub mod pane_share;
pub mod remote_state;
pub mod tailscale;

#[allow(unused_imports)]
pub use pane_share::{share_pane, PaneShareHandle, ShareError};
pub use remote_state::{OmwRemoteState, OmwRemoteStatus};
