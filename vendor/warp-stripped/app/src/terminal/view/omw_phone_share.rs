// SPDX-License-Identifier: AGPL-3.0-only
//
// omw-authored file in the in-tree Warp fork (warpdotdev/warp), part of the
// AGPL-3.0 derivative work. See specs/fork-strategy.md section 3.
//
// Copyright (C) 2026 Shenhao Miao and the omw contributors
// Copyright (C) 2020-2026 Denver Technologies, Inc.
//
// This program is free software: you can redistribute it and/or modify it
// under the terms of the GNU Affero General Public License, version 3, as
// published by the Free Software Foundation. See the LICENSE file at the
// repository root for the full text.

//! Persistent, per-pane phone sharing for local terminal views.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt as _;
use warpui::clipboard::ClipboardContent;
use warpui::r#async::Timer;
use warpui::{AppContext, EntityId, ViewContext};

use super::TerminalView;
use crate::omw::pair_button::pair_button_text;
use crate::omw::{OmwRemoteState, OmwRemoteStatus};
use crate::pane_group::pane::DetachType;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PhoneShareTransition {
    Unavailable,
    Starting,
    StartAndShare,
    Share,
    Unshare { stop_daemon: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PhoneSharePresentation {
    pub label: &'static str,
    pub tooltip: &'static str,
    pub active: bool,
    pub disabled: bool,
}

impl PhoneSharePresentation {
    fn from_state(status: &OmwRemoteStatus, is_shared: bool, is_pending: bool) -> Self {
        let (label, tooltip) = if is_pending {
            pair_button_text(&OmwRemoteStatus::Starting, false)
        } else {
            pair_button_text(status, is_shared)
        };
        Self {
            label,
            tooltip,
            active: !is_pending && matches!(status, OmwRemoteStatus::Running { .. }) && is_shared,
            disabled: is_pending || matches!(status, OmwRemoteStatus::Starting),
        }
    }
}

pub(crate) fn phone_share_transition(
    is_shareable: bool,
    status: &OmwRemoteStatus,
    is_shared: bool,
    share_count: usize,
) -> PhoneShareTransition {
    if matches!(status, OmwRemoteStatus::Starting) {
        return PhoneShareTransition::Starting;
    }
    if is_shared {
        return PhoneShareTransition::Unshare {
            stop_daemon: share_count <= 1,
        };
    }
    if !is_shareable {
        return PhoneShareTransition::Unavailable;
    }

    match status {
        OmwRemoteStatus::Starting => PhoneShareTransition::Starting,
        OmwRemoteStatus::Stopped | OmwRemoteStatus::Failed { .. } => {
            PhoneShareTransition::StartAndShare
        }
        OmwRemoteStatus::Running { .. } => PhoneShareTransition::Share,
    }
}

pub(crate) fn should_unshare_for_detach(detach_type: DetachType) -> bool {
    !matches!(detach_type, DetachType::Moved)
}

fn until_view_dropped<S>(
    stream: S,
    lifetime: async_channel::Receiver<()>,
) -> impl futures::Stream<Item = S::Item>
where
    S: futures::Stream,
{
    stream.take_until(async move { lifetime.recv().await.ok() })
}

pub(crate) fn unshare_phone_pane_after_detach(view_id: EntityId, detach_type: DetachType) {
    if !should_unshare_for_detach(detach_type) {
        return;
    }

    let state = OmwRemoteState::shared();
    if !state.is_pane_shared(view_id) {
        return;
    }

    state.unshare_pane(view_id);
    if state.share_count() == 0 {
        if let Err(error) = state.stop() {
            log::warn!("omw-remote: stop after detached last shared pane failed: {error}");
        }
    }
}

fn surface_pair_modal(state: &OmwRemoteState, ctx: &mut ViewContext<TerminalView>) {
    use crate::omw::pair_modal::{format_pair_modal_text_block, PairModalContent};
    use crate::omw::tailscale::detect_status as detect_tailscale_status;

    let content = PairModalContent {
        status: state.status(),
        tailscale: detect_tailscale_status(),
        paired_device_count: None,
    };

    if let OmwRemoteStatus::Running { pair_url, .. } = &content.status {
        ctx.clipboard()
            .write(ClipboardContent::plain_text(pair_url.clone()));
        let tailnet_host = content.tailscale.local_hostname.as_deref();
        if let Err(error) = crate::omw::pair_browser::open_pair_page(pair_url, tailnet_host) {
            log::warn!("omw-remote: open_pair_page failed: {error}");
        }
    }

    eprintln!(
        "\n=== omw Remote Control ===\n{}\n==========================\n",
        format_pair_modal_text_block(&content)
    );
    log::info!(
        "omw-remote: surfaced pair-modal toast (status={:?})",
        content.status
    );
}

impl TerminalView {
    pub(crate) fn is_omw_phone_shareable(&self, ctx: &AppContext) -> bool {
        crate::omw::pane_auto_share::has_local_tty_manager(self, ctx)
    }

    pub(crate) fn omw_phone_share_presentation(
        &self,
        ctx: &AppContext,
    ) -> Option<PhoneSharePresentation> {
        let state = OmwRemoteState::shared();
        let is_shared = state.is_pane_shared(self.view_id());
        (is_shared || self.is_omw_phone_shareable(ctx)).then(|| {
            PhoneSharePresentation::from_state(
                &state.status(),
                is_shared,
                self.omw_phone_share_pending_generation.is_some(),
            )
        })
    }

    pub(crate) fn register_omw_phone_share_subscriptions(&mut self, ctx: &mut ViewContext<Self>) {
        let state = OmwRemoteState::shared();
        let (lifetime_tx, lifetime_rx) = async_channel::bounded::<()>(1);
        self.omw_phone_share_subscription_lifetime = Some(lifetime_tx);

        let status_lifetime = lifetime_rx.clone();
        let _ = ctx.spawn_stream_local(
            until_view_dropped(state.subscribe_status_stream(), status_lifetime),
            |me, _status: OmwRemoteStatus, ctx| me.refresh_omw_phone_share_ui(ctx),
            |_, _| {},
        );

        let _ = ctx.spawn_stream_local(
            until_view_dropped(state.subscribe_share_stream(), lifetime_rx),
            |me, _version: u64, ctx| me.refresh_omw_phone_share_ui(ctx),
            |_, _| {},
        );
    }

    fn refresh_omw_phone_share_ui(&mut self, ctx: &mut ViewContext<Self>) {
        self.refresh_pane_header(ctx);
        ctx.notify();
    }

    pub(crate) fn toggle_omw_phone_share(&mut self, ctx: &mut ViewContext<Self>) {
        if self.omw_phone_share_pending_generation.is_some() {
            return;
        }

        let state = OmwRemoteState::shared();
        let is_shared = state.is_pane_shared(self.view_id());
        let transition = phone_share_transition(
            self.is_omw_phone_shareable(ctx),
            &state.status(),
            is_shared,
            state.share_count(),
        );

        match transition {
            PhoneShareTransition::Unavailable => {
                self.show_error_toast(
                    "This pane is not backed by a local terminal and cannot be shared with a phone."
                        .to_owned(),
                    ctx,
                );
            }
            PhoneShareTransition::Starting => {}
            PhoneShareTransition::Unshare { stop_daemon } => {
                state.unshare_pane(self.view_id());
                let remaining = state.share_count();
                if stop_daemon && remaining == 0 {
                    if let Err(error) = state.stop() {
                        log::warn!("omw-remote: stop after last unshare failed: {error}");
                    }
                }
                log::info!(
                    "omw-remote: unshared pane {} (remaining_shares={remaining})",
                    self.view_id()
                );
                self.refresh_omw_phone_share_ui(ctx);
            }
            PhoneShareTransition::StartAndShare => {
                self.start_and_defer_omw_phone_share(state, ctx);
            }
            PhoneShareTransition::Share => self.defer_omw_phone_share(false, ctx),
        }
    }

    fn begin_omw_phone_share(
        &mut self,
        started_daemon: bool,
        ctx: &mut ViewContext<Self>,
    ) -> (u64, Arc<AtomicBool>) {
        self.omw_phone_share_generation = self.omw_phone_share_generation.wrapping_add(1);
        let generation = self.omw_phone_share_generation;
        let active = Arc::new(AtomicBool::new(true));
        self.omw_phone_share_pending_generation = Some(generation);
        self.omw_phone_share_pending_started_daemon = started_daemon;
        self.omw_phone_share_pending_active = Some(active.clone());
        self.refresh_omw_phone_share_ui(ctx);
        (generation, active)
    }

    fn start_and_defer_omw_phone_share(
        &mut self,
        state: Arc<OmwRemoteState>,
        ctx: &mut ViewContext<Self>,
    ) {
        let (generation, active) = self.begin_omw_phone_share(true, ctx);
        let state_for_start = state.clone();
        let active_for_start = active.clone();

        ctx.spawn(
            async move {
                let blocking_state = state_for_start.clone();
                let result = tokio::task::spawn_blocking(move || blocking_state.start())
                    .await
                    .map_err(|error| format!("daemon startup worker failed: {error}"))
                    .and_then(|result| result);

                // The view callback may be discarded when a pane closes. Keep
                // rollback coupled to the worker so a cancelled first share
                // cannot leave an empty daemon running.
                if result.is_ok()
                    && !active_for_start.load(Ordering::Acquire)
                    && state_for_start.share_count() == 0
                {
                    if let Err(error) = state_for_start.stop() {
                        log::warn!("omw-remote: cancelled startup rollback failed: {error}");
                    }
                }
                result
            },
            move |me, result, ctx| {
                let request_is_current = me.omw_phone_share_pending_generation == Some(generation)
                    && active.load(Ordering::Acquire);
                if !request_is_current {
                    if result.is_ok() && state.share_count() == 0 {
                        if let Err(error) = state.stop() {
                            log::warn!("omw-remote: stale startup rollback failed: {error}");
                        }
                    }
                    return;
                }

                match result {
                    Ok(()) => me.defer_omw_phone_share_generation(generation, true, ctx),
                    Err(error) => {
                        active.store(false, Ordering::Release);
                        me.omw_phone_share_pending_generation = None;
                        me.omw_phone_share_pending_started_daemon = false;
                        me.omw_phone_share_pending_active = None;
                        log::warn!("omw-remote: start failed: {error}");
                        me.show_error_toast(format!("Phone sharing could not start: {error}"), ctx);
                        me.refresh_omw_phone_share_ui(ctx);
                    }
                }
            },
        );
    }

    fn defer_omw_phone_share(&mut self, surface_first_pair: bool, ctx: &mut ViewContext<Self>) {
        let (generation, _) = self.begin_omw_phone_share(surface_first_pair, ctx);
        self.defer_omw_phone_share_generation(generation, surface_first_pair, ctx);
    }

    fn defer_omw_phone_share_generation(
        &mut self,
        generation: u64,
        surface_first_pair: bool,
        ctx: &mut ViewContext<Self>,
    ) {
        ctx.spawn(Timer::after(Duration::ZERO), move |me, _result, ctx| {
            if me.omw_phone_share_pending_generation != Some(generation) {
                return;
            }
            if let Some(active) = me.omw_phone_share_pending_active.take() {
                active.store(false, Ordering::Release);
            }
            me.omw_phone_share_pending_generation = None;
            me.omw_phone_share_pending_started_daemon = false;

            let state = OmwRemoteState::shared();
            let (Some(registry), Some(runtime)) = (state.pty_registry(), state.runtime_handle())
            else {
                if surface_first_pair && state.share_count() == 0 {
                    let _ = state.stop();
                }
                me.show_error_toast("Phone sharing is not available yet.".to_owned(), ctx);
                me.refresh_omw_phone_share_ui(ctx);
                return;
            };

            let Some(handle) =
                crate::omw::pane_auto_share::share_self_pane(me, ctx, registry, runtime)
            else {
                if surface_first_pair && state.share_count() == 0 {
                    let _ = state.stop();
                }
                me.show_error_toast(
                    "This pane could not be shared with a phone.".to_owned(),
                    ctx,
                );
                me.refresh_omw_phone_share_ui(ctx);
                return;
            };

            let view_id = me.view_id();
            if state.store_pane_share(view_id, handle) {
                log::info!("omw pane_auto_share: shared pane {view_id} on Phone action");
                if surface_first_pair {
                    surface_pair_modal(&state, ctx);
                }
            } else {
                log::debug!("omw pane_auto_share: pane {view_id} was already shared");
            }
            me.refresh_omw_phone_share_ui(ctx);
        });
    }

    pub(crate) fn cancel_pending_omw_phone_share(&mut self) {
        let stop_empty_daemon = self.omw_phone_share_pending_generation.is_some()
            && self.omw_phone_share_pending_started_daemon;
        if let Some(active) = self.omw_phone_share_pending_active.take() {
            active.store(false, Ordering::Release);
        }
        self.omw_phone_share_generation = self.omw_phone_share_generation.wrapping_add(1);
        self.omw_phone_share_pending_generation = None;
        self.omw_phone_share_pending_started_daemon = false;

        if stop_empty_daemon {
            let state = OmwRemoteState::shared();
            if state.share_count() == 0 {
                if let Err(error) = state.stop() {
                    log::warn!("omw-remote: pending first-share rollback failed: {error}");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn running() -> OmwRemoteStatus {
        OmwRemoteStatus::Running {
            pair_url: "http://127.0.0.1:8787/pair?t=test".to_owned(),
            tailscale_serving: false,
        }
    }

    #[test]
    fn omw_phone_share_transition_covers_daemon_and_pane_states() {
        assert_eq!(
            phone_share_transition(true, &OmwRemoteStatus::Stopped, false, 0),
            PhoneShareTransition::StartAndShare
        );
        assert_eq!(
            phone_share_transition(
                true,
                &OmwRemoteStatus::Failed {
                    error: "failed".to_owned()
                },
                false,
                0
            ),
            PhoneShareTransition::StartAndShare
        );
        assert_eq!(
            phone_share_transition(true, &OmwRemoteStatus::Starting, false, 0),
            PhoneShareTransition::Starting
        );
        assert_eq!(
            phone_share_transition(false, &OmwRemoteStatus::Starting, true, 1),
            PhoneShareTransition::Starting
        );
        assert_eq!(
            phone_share_transition(true, &running(), false, 1),
            PhoneShareTransition::Share
        );
        assert_eq!(
            phone_share_transition(true, &running(), true, 1),
            PhoneShareTransition::Unshare { stop_daemon: true }
        );
        assert_eq!(
            phone_share_transition(true, &running(), true, 2),
            PhoneShareTransition::Unshare { stop_daemon: false }
        );
        assert_eq!(
            phone_share_transition(false, &running(), false, 0),
            PhoneShareTransition::Unavailable
        );
    }

    #[test]
    fn omw_phone_share_presentation_is_isolated_per_pane() {
        let status = running();
        let pane_a = PhoneSharePresentation::from_state(&status, true, false);
        let pane_b = PhoneSharePresentation::from_state(&status, false, false);

        assert_eq!(pane_a.label, "Stop sharing");
        assert!(pane_a.active);
        assert_eq!(pane_b.label, "Share with phone");
        assert!(!pane_b.active);
    }

    #[test]
    fn omw_phone_share_starting_is_disabled() {
        let presentation =
            PhoneSharePresentation::from_state(&OmwRemoteStatus::Starting, false, false);
        assert_eq!(presentation.label, "Starting...");
        assert!(presentation.disabled);
        assert!(!presentation.active);

        let pending = PhoneSharePresentation::from_state(&running(), false, true);
        assert_eq!(pending.label, "Starting...");
        assert!(pending.disabled);
        assert!(!pending.active);
    }

    #[test]
    fn omw_phone_share_detach_cleanup_preserves_moves_only() {
        assert!(!should_unshare_for_detach(DetachType::Moved));
        assert!(should_unshare_for_detach(DetachType::HiddenForClose));
        assert!(should_unshare_for_detach(DetachType::Closed));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn omw_phone_share_subscriptions_end_with_view_lifetime() {
        let (lifetime_tx, lifetime_rx) = async_channel::bounded::<()>(1);
        let mut stream = Box::pin(until_view_dropped(
            futures::stream::pending::<()>(),
            lifetime_rx,
        ));

        drop(lifetime_tx);
        let next = tokio::time::timeout(Duration::from_secs(1), stream.next())
            .await
            .expect("view-lifetime stream should terminate promptly");
        assert!(next.is_none());
    }
}
