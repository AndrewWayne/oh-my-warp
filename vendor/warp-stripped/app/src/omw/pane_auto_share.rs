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

//! Share a Warp terminal pane into the embedded `omw-remote` daemon's
//! [`omw_server::SessionRegistry`] so a paired phone sees the laptop's actual
//! Warp pane (not a sibling shell spawned by the daemon's default-shell
//! path).
//!
//! Scope: the Phone-click handler shares the *active* pane via
//! [`share_self_pane`]. Whole-workspace ("share every pane") auto-share is
//! deliberately NOT implemented here — the iteration approach crashed
//! warp-oss on Phone click twice; see [`share_self_pane`] for the mechanism
//! and TODO.md ("Multi-pane auto-share") for the deferred design notes and
//! the GUI-harness prerequisite for revisiting it.
//!
//! Only `local_tty::TerminalManager`-backed panes can be shared:
//! [`super::pane_share::share_pane`] requires the local `event_loop_tx` /
//! `pty_reads_tx` channel pair, which only that manager exposes. Remote SSH,
//! shared-session-viewer, and mock managers yield `None` from
//! [`local_io_handles_for`] and are skipped.
//!
//! `share_pane` is async (one trivial `.await` on `register_external`,
//! which is a synchronous mutex insert wearing async clothes). We bounce
//! through the daemon's tokio runtime via `runtime.spawn` + a per-pane
//! [`std::sync::mpsc::sync_channel`] to collect each handle synchronously
//! on the UI thread. The wait is microseconds — no perceptible UI freeze.

use std::sync::Arc;

use warpui::{AppContext, ModelHandle};

use super::pane_share::{share_pane, PaneShareHandle};
use crate::pane_group::pane::PaneStack;
use crate::terminal::local_tty::terminal_manager::TerminalManager as LocalTtyManager;
use crate::terminal::terminal_manager::TerminalManager;
use crate::terminal::TerminalView;

pub(crate) type LocalIoHandles = (
    Arc<parking_lot::Mutex<crate::terminal::local_tty::mio_channel::Sender<crate::terminal::writeable_pty::Message>>>,
    async_broadcast::Sender<Arc<Vec<u8>>>,
    crate::terminal::SizeInfo,
);

/// Pull the local-PTY io channels off `tv`'s active manager, if it's a
/// `local_tty::TerminalManager`. Returns `None` for remote SSH panes,
/// shared-session-viewer panes, mock managers, and detached views (no
/// pane_stack). The downcast happens against the concrete impl in
/// `terminal::local_tty::terminal_manager::TerminalManager`.
pub(crate) fn local_io_handles_for(
    tv: &TerminalView,
    ctx: &AppContext,
) -> Option<LocalIoHandles> {
    let stack: ModelHandle<PaneStack<TerminalView>> = tv.pane_stack_handle(ctx)?;
    let manager_handle = stack.as_ref(ctx).active_data().clone();
    let manager_box: &Box<dyn TerminalManager> = manager_handle.as_ref(ctx);
    let local: &LocalTtyManager = manager_box.as_any().downcast_ref::<LocalTtyManager>()?;
    Some((local.event_loop_tx(), local.pty_reads_tx(), local.current_size_info()))
}

/// Share JUST the supplied `TerminalView`'s pane (no workspace iteration).
/// Used by the Phone-click handler to register the active pane the user
/// clicked from. Returns `None` if the pane isn't backed by a
/// `local_tty::TerminalManager` or if `share_pane` errors.
///
/// Why only the active pane, and not a walk over every pane: the iteration
/// version walked every PaneGroup and re-entered every TerminalView's update
/// closure via `for_all_terminal_panes`. That crashed warp-oss on Phone click
/// in two separate attempts (commits 1272ce6 and 49ffbb2), so the helper was
/// removed rather than left compiled — see TODO.md ("Multi-pane auto-share")
/// for the deferred design. Sharing only the active pane skips iteration
/// entirely — no foreign-view re-entry, no nested PaneGroup borrow.
pub fn share_self_pane(
    me: &TerminalView,
    ctx: &AppContext,
    registry: Arc<omw_server::SessionRegistry>,
    runtime: tokio::runtime::Handle,
) -> Option<PaneShareHandle> {
    let Some(io) = local_io_handles_for(me, ctx) else {
        // stderr-direct so users running warp-oss from PowerShell see the
        // diagnostic without configuring log filters. The negative case is
        // important to surface: it means "we ran, but the active manager
        // wasn't a local_tty pane" — which would silently leave the
        // Sessions list empty and the user wondering.
        eprintln!(
            "[omw-debug] share_self_pane: active pane is NOT a local_tty manager \
             (could be remote SSH, shared-session viewer, or detached); skipping"
        );
        return None;
    };
    // Multi-pane share: each session needs a recognizable name in the phone's
    // /api/v1/sessions list. Prefer the pane's local cwd; fall back to
    // `pane-{view_id}` so the rows stay distinguishable even when no cwd is
    // available (e.g. a detached pane that hasn't established a session yet).
    let view_id = me.view_id();
    let name = me
        .pwd_if_local(ctx)
        .unwrap_or_else(|| format!("pane-{view_id}"));
    match spawn_share_and_collect(&runtime, &registry, &name, io) {
        Ok(h) => {
            eprintln!(
                "[omw-debug] share_self_pane: registered pane {view_id} as session {} (name={name:?})",
                h.session_id
            );
            Some(h)
        }
        Err(e) => {
            eprintln!("[omw-debug] share_self_pane: spawn_share_and_collect failed: {e}");
            log::warn!("omw pane_auto_share: share_self_pane failed: {e}");
            None
        }
    }
}

/// Spawn `share_pane` on the daemon runtime and synchronously collect the
/// resulting `PaneShareHandle`. The future's only `.await` is on
/// `register_external`, which is a non-blocking mutex insert — total wall
/// time is in microseconds.
fn spawn_share_and_collect(
    runtime: &tokio::runtime::Handle,
    registry: &Arc<omw_server::SessionRegistry>,
    name: &str,
    io: LocalIoHandles,
) -> Result<PaneShareHandle, String> {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let registry_clone = registry.clone();
    let name_owned = name.to_string();
    let (event_loop_tx, pty_reads_tx, current_size) = io;
    runtime.spawn(async move {
        let result = share_pane(
            name_owned,
            event_loop_tx,
            pty_reads_tx,
            current_size,
            registry_clone,
        )
        .await;
        let _ = tx.send(result);
    });
    rx.recv()
        .map_err(|e| format!("share-pane channel closed: {e}"))?
        .map_err(|e| e.to_string())
}
