// SPDX-License-Identifier: AGPL-3.0-only
//
// omw-authored file in the in-tree Warp fork (warpdotdev/warp), part of the
// AGPL-3.0 derivative work. See specs/fork-strategy.md §3.
//
// Copyright (C) 2026 Shenhao Miao and the omw contributors
// Copyright (C) 2020-2026 Denver Technologies, Inc.
//
// This program is free software: you can redistribute it and/or modify it
// under the terms of the GNU Affero General Public License, version 3, as
// published by the Free Software Foundation. See the LICENSE file at the
// repository root for the full text.

//! omw integration module.
//!
//! Wired by Wiring 5 ("Combined Overseer + Executor wiring pass"). Hosts the
//! launcher state for the embedded `omw-remote` daemon, which the agent footer
//! "Remote Control" button starts/stops.
//!
//! Gated behind the `omw_local` feature so non-omw_local builds (if any) stay
//! clean. See `vendor/warp-stripped/OMW_LOCAL_BUILD.md`.

pub mod pair_browser;
pub mod pair_button;
pub mod pair_modal;
pub mod pane_auto_share;
pub mod pane_share;
pub mod qr;
pub mod remote_state;
pub mod tailscale;

#[allow(unused_imports)]
pub use pane_share::{share_pane, PaneShareHandle, ShareError};
pub use remote_state::{OmwRemoteState, OmwRemoteStatus};
