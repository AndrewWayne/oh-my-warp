//! Auto-share every open Warp pane into the embedded `omw-remote` daemon's
//! [`omw_server::SessionRegistry`] so a paired phone sees the laptop's actual
//! Warp panes (not a sibling shell spawned by the daemon's default-shell
//! path).
//!
//! v0.4-thin scope: ONE-SHOT enumeration on daemon start. Iterates every
//! `local_tty::TerminalManager`-backed terminal pane in the *current window's*
//! workspace, spawns [`super::pane_share::share_pane`] for each on the
//! daemon runtime, and returns the resulting [`PaneShareHandle`]s for
//! [`super::OmwRemoteState`] to hold for the daemon's lifetime.
//!
//! Known limitations (deliberate, deferred to a follow-up):
//! - **Reactive add/remove**: panes opened *after* daemon start are NOT
//!   auto-shared. The user has to stop+start the daemon to refresh, or open
//!   the new pane before clicking the Phone button. Subscribing to
//!   `PaneStackEvent::ViewAdded` / `::ViewRemoved` per stack is doable but
//!   not surgical (every PaneGroup × every PaneStack), and a periodic
//!   re-walk would also work — left for a continuation pass.
//! - **Multi-window**: only panes in the window where the user clicked Phone
//!   are shared. `WorkspaceRegistry::all_workspaces` makes the multi-window
//!   variant straightforward; the simple "current window only" form keeps
//!   the demo loop tight.
//! - **Remote SSH and shared-session-viewer panes**: skipped via the
//!   downcast filter — `share_pane` requires the local `event_loop_tx` /
//!   `pty_reads_tx` channel pair, which only `local_tty::TerminalManager`
//!   exposes. Other manager types (remote_tty, shared_session::viewer,
//!   mock) silently fall through.
//!
//! `share_pane` is async (one trivial `.await` on `register_external`,
//! which is a synchronous mutex insert wearing async clothes). We bounce
//! through the daemon's tokio runtime via `runtime.spawn` + a per-pane
//! [`std::sync::mpsc::sync_channel`] to collect each handle synchronously
//! on the UI thread. The wait is microseconds — no perceptible UI freeze.

use std::sync::Arc;

use warpui::{AppContext, ModelHandle, SingletonEntity, ViewContext, ViewHandle};

use super::pane_share::{share_pane, PaneShareHandle};
use crate::pane_group::pane::PaneStack;
use crate::pane_group::PaneGroup;
use crate::terminal::local_tty::terminal_manager::TerminalManager as LocalTtyManager;
use crate::terminal::terminal_manager::TerminalManager;
use crate::terminal::TerminalView;
use crate::workspace::WorkspaceRegistry;

/// Walk every terminal pane in the current window's workspace, share each
/// `local_tty::TerminalManager`-backed pane into the registry, and return
/// the resulting handles. Skips remote/shared/mock managers via downcast.
///
/// Caller must be running on the Warp UI thread (we use `ctx` to traverse
/// pane groups). The supplied `runtime` handle should be the daemon's
/// runtime — the `share_pane` futures spawn there.
pub fn share_all_local_panes(
    ctx: &mut ViewContext<TerminalView>,
    registry: Arc<omw_server::SessionRegistry>,
    runtime: tokio::runtime::Handle,
) -> Vec<PaneShareHandle> {
    // Snapshot the pane groups (tabs) in the current window. We collect into
    // a Vec so the WorkspaceRegistry borrow ends before we start mutating
    // each pane group via `update`.
    let window_id = ctx.window_id();
    let pane_groups: Vec<ViewHandle<PaneGroup>> =
        match WorkspaceRegistry::as_ref(ctx).get(window_id, ctx) {
            Some(workspace_handle) => workspace_handle
                .as_ref(ctx)
                .tab_views()
                .cloned()
                .collect(),
            None => {
                log::warn!(
                    "omw pane_auto_share: no workspace registered for current window; \
                     nothing to share"
                );
                return Vec::new();
            }
        };

    let mut handles = Vec::new();
    let mut pane_seq: usize = 0;
    for pane_group_handle in pane_groups {
        // Collect (event_loop_tx, pty_reads_tx, pane_name) for every local
        // terminal pane in this pane group. We do the channel cloning inside
        // the for_all_terminal_panes callback (which has the right contexts),
        // then issue the share_pane futures *outside* that closure so each
        // sync_channel recv doesn't sit inside the per-view borrow.
        let mut io_specs: Vec<(LocalIoHandles, String)> = Vec::new();
        pane_group_handle.update(ctx, |pg: &mut PaneGroup, pg_ctx| {
            pg.for_all_terminal_panes(
                |tv: &mut TerminalView, tv_ctx: &mut ViewContext<TerminalView>| {
                    if let Some(io) = local_io_handles_for(tv, tv_ctx) {
                        pane_seq += 1;
                        let name = format!("pane-{pane_seq}");
                        io_specs.push((io, name));
                    }
                },
                pg_ctx,
            );
        });

        for (io, name) in io_specs {
            match spawn_share_and_collect(&runtime, &registry, &name, io) {
                Ok(handle) => handles.push(handle),
                Err(e) => {
                    log::warn!("omw pane_auto_share: share_pane failed for {name}: {e}");
                }
            }
        }
    }

    handles
}

type LocalIoHandles = (
    Arc<parking_lot::Mutex<crate::terminal::local_tty::mio_channel::Sender<crate::terminal::writeable_pty::Message>>>,
    async_broadcast::Sender<Arc<Vec<u8>>>,
);

/// Pull the local-PTY io channels off `tv`'s active manager, if it's a
/// `local_tty::TerminalManager`. Returns `None` for remote SSH panes,
/// shared-session-viewer panes, mock managers, and detached views (no
/// pane_stack). The downcast happens against the concrete impl in
/// `terminal::local_tty::terminal_manager::TerminalManager`.
fn local_io_handles_for(
    tv: &TerminalView,
    ctx: &AppContext,
) -> Option<LocalIoHandles> {
    let stack: ModelHandle<PaneStack<TerminalView>> = tv.pane_stack_handle(ctx)?;
    let manager_handle = stack.as_ref(ctx).active_data().clone();
    let manager_box: &Box<dyn TerminalManager> = manager_handle.as_ref(ctx);
    let local: &LocalTtyManager = manager_box.as_any().downcast_ref::<LocalTtyManager>()?;
    Some((local.event_loop_tx(), local.pty_reads_tx()))
}

/// Share JUST the supplied `TerminalView`'s pane (no workspace iteration).
/// Used by the Phone-click handler to register the active pane the user
/// clicked from. Returns `None` if the pane isn't backed by a
/// `local_tty::TerminalManager` or if `share_pane` errors.
///
/// Why this and not [`share_all_local_panes`]: the iteration version walks
/// every PaneGroup and re-enters every TerminalView's update closure via
/// `for_all_terminal_panes`. That crashed warp-oss on Phone click in two
/// separate attempts (commits 1272ce6 and 49ffbb2). Sharing only the
/// active pane skips iteration entirely — no foreign-view re-entry, no
/// nested PaneGroup borrow.
pub fn share_self_pane(
    me: &TerminalView,
    ctx: &AppContext,
    registry: Arc<omw_server::SessionRegistry>,
    runtime: tokio::runtime::Handle,
) -> Option<PaneShareHandle> {
    let io = local_io_handles_for(me, ctx)?;
    match spawn_share_and_collect(&runtime, &registry, "active-pane", io) {
        Ok(h) => Some(h),
        Err(e) => {
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
    let (event_loop_tx, pty_reads_tx) = io;
    runtime.spawn(async move {
        let result = share_pane(name_owned, event_loop_tx, pty_reads_tx, registry_clone).await;
        let _ = tx.send(result);
    });
    rx.recv()
        .map_err(|e| format!("share-pane channel closed: {e}"))?
        .map_err(|e| e.to_string())
}
