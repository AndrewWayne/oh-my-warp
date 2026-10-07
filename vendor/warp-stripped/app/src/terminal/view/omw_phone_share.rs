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

//! Mac adaptation of upstream 359d142: persistent, per-pane phone sharing.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_compat::CompatExt as _;
use futures::StreamExt as _;
use warpui::r#async::Timer;
use warpui::{AppContext, EntityId, ViewContext};

use super::TerminalView;
use crate::omw::pair_button::pair_button_text;
use crate::omw::{OmwRemoteState, OmwRemoteStatus};
use crate::pane_group::pane::DetachType;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhoneShareTransition {
    Unavailable,
    Starting,
    StartAndShare,
    Share,
    Unshare { stop_daemon: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhoneSharePresentation {
    pub label: &'static str,
    pub tooltip: &'static str,
    pub active: bool,
    pub disabled: bool,
}

impl PhoneSharePresentation {
    pub fn from_state(status: &OmwRemoteStatus, is_shared: bool, is_pending: bool) -> Self {
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

pub fn phone_share_transition(
    is_shareable: bool,
    status: &OmwRemoteStatus,
    is_shared: bool,
    share_count: usize,
) -> PhoneShareTransition {
    if matches!(status, OmwRemoteStatus::Starting) {
        return PhoneShareTransition::Starting;
    }
    if is_shared {
        return PhoneShareTransition::Unshare { stop_daemon: share_count <= 1 };
    }
    if !is_shareable {
        return PhoneShareTransition::Unavailable;
    }
    match status {
        OmwRemoteStatus::Starting => PhoneShareTransition::Starting,
        OmwRemoteStatus::Stopped | OmwRemoteStatus::Failed { .. } => PhoneShareTransition::StartAndShare,
        OmwRemoteStatus::Running { .. } => PhoneShareTransition::Share,
    }
}

pub fn should_unshare_for_detach(detach_type: DetachType) -> bool {
    !matches!(detach_type, DetachType::Moved)
}

pub fn until_view_dropped<S>(stream: S, lifetime: async_channel::Receiver<()>) -> impl futures::Stream<Item = S::Item>
where S: futures::Stream {
    stream.take_until(async move { lifetime.recv().await.ok() })
}

pub(crate) fn unshare_phone_pane_after_detach(view_id: EntityId, detach_type: DetachType) {
    if !should_unshare_for_detach(detach_type) { return; }
    let state = OmwRemoteState::shared();
    if !state.is_pane_shared(view_id) { return; }
    state.unshare_pane(view_id);
    if state.share_count() == 0 {
        if let Err(error) = state.stop() {
            log::warn!("omw-remote: stop after detached last shared pane failed: {error}");
        }
    }
}

impl TerminalView {
    pub(crate) fn is_omw_phone_shareable(&self, ctx: &AppContext) -> bool {
        crate::omw::pane_auto_share::has_local_tty_manager(self, ctx)
    }

    pub(crate) fn omw_phone_share_presentation(&self, ctx: &AppContext) -> Option<PhoneSharePresentation> {
        let state = OmwRemoteState::shared();
        let is_shared = state.is_pane_shared(self.view_id());
        (is_shared || self.is_omw_phone_shareable(ctx)).then(|| PhoneSharePresentation::from_state(
            &state.status(), is_shared, self.omw_phone_share_pending_generation.is_some(),
        ))
    }

    pub(crate) fn register_omw_phone_share_subscriptions(&mut self, ctx: &mut ViewContext<Self>) {
        let state = OmwRemoteState::shared();
        let (lifetime_tx, lifetime_rx) = async_channel::bounded::<()>(1);
        self.omw_phone_share_subscription_lifetime = Some(lifetime_tx);
        let _ = ctx.spawn_stream_local(
            until_view_dropped(state.phone_status_stream(), lifetime_rx.clone()),
            |me, _status: OmwRemoteStatus, ctx| me.refresh_omw_phone_share_ui(ctx),
            |_, _| {},
        );
        let _ = ctx.spawn_stream_local(
            until_view_dropped(state.phone_share_stream(), lifetime_rx),
            |me, _version: u64, ctx| me.refresh_omw_phone_share_ui(ctx),
            |_, _| {},
        );
    }

    fn refresh_omw_phone_share_ui(&mut self, ctx: &mut ViewContext<Self>) {
        self.refresh_pane_header(ctx);
        ctx.notify();
    }

    pub(crate) fn toggle_omw_phone_share(&mut self, ctx: &mut ViewContext<Self>) {
        if self.omw_phone_share_pending_generation.is_some() { return; }
        let state = OmwRemoteState::shared();
        let transition = phone_share_transition(self.is_omw_phone_shareable(ctx), &state.status(),
            state.is_pane_shared(self.view_id()), state.share_count());
        match transition {
            PhoneShareTransition::Unavailable => self.show_error_toast(
                "This pane is not backed by a local terminal and cannot be shared with a phone.".to_owned(), ctx),
            PhoneShareTransition::Starting => {},
            PhoneShareTransition::Unshare { stop_daemon } => {
                state.unshare_pane(self.view_id());
                if stop_daemon && state.share_count() == 0 {
                    if let Err(error) = state.stop() {
                        log::warn!("omw-remote: stop after last unshare failed: {error}");
                    }
                }
                self.refresh_omw_phone_share_ui(ctx);
            }
            PhoneShareTransition::StartAndShare => self.start_and_defer_omw_phone_share(state, ctx),
            PhoneShareTransition::Share => self.defer_omw_phone_share(false, ctx),
        }
    }

    fn begin_omw_phone_share(&mut self, started_daemon: bool, ctx: &mut ViewContext<Self>) -> (u64, Arc<AtomicBool>) {
        self.omw_phone_share_generation = self.omw_phone_share_generation.wrapping_add(1);
        let generation = self.omw_phone_share_generation;
        let active = Arc::new(AtomicBool::new(true));
        self.omw_phone_share_pending_generation = Some(generation);
        self.omw_phone_share_pending_started_daemon = started_daemon;
        self.omw_phone_share_pending_active = Some(active.clone());
        self.refresh_omw_phone_share_ui(ctx);
        (generation, active)
    }

    fn start_and_defer_omw_phone_share(&mut self, state: Arc<OmwRemoteState>, ctx: &mut ViewContext<Self>) {
        let (generation, active) = self.begin_omw_phone_share(true, ctx);
        let state_for_start = state.clone();
        let active_for_start = active.clone();
        ctx.spawn(async move {
            let blocking_state = state_for_start.clone();
            let result = tokio::task::spawn_blocking(move || blocking_state.start()).await
                .map_err(|error| format!("daemon startup worker failed: {error}"))
                .and_then(|result| result);
            // A closed view loses its callback. Rollback must also live with
            // the worker so cancellation cannot leave an empty daemon running.
            if result.is_ok() && !active_for_start.load(Ordering::Acquire) && state_for_start.share_count() == 0 {
                let _ = state_for_start.stop();
            }
            result
        }.compat(), move |me, result, ctx| {
            if me.omw_phone_share_pending_generation != Some(generation) || !active.load(Ordering::Acquire) {
                if result.is_ok() && state.share_count() == 0 { let _ = state.stop(); }
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
        });
    }

    fn defer_omw_phone_share(&mut self, surface_first_pair: bool, ctx: &mut ViewContext<Self>) {
        let (generation, _) = self.begin_omw_phone_share(surface_first_pair, ctx);
        self.defer_omw_phone_share_generation(generation, surface_first_pair, ctx);
    }

    fn defer_omw_phone_share_generation(&mut self, generation: u64, surface_first_pair: bool, ctx: &mut ViewContext<Self>) {
        ctx.spawn(Timer::after(Duration::ZERO), move |me, _result, ctx| {
            if me.omw_phone_share_pending_generation != Some(generation) { return; }
            if let Some(active) = me.omw_phone_share_pending_active.take() { active.store(false, Ordering::Release); }
            me.omw_phone_share_pending_generation = None;
            me.omw_phone_share_pending_started_daemon = false;
            let state = OmwRemoteState::shared();
            let (Some(registry), Some(runtime)) = (state.pty_registry(), state.runtime_handle()) else {
                if surface_first_pair && state.share_count() == 0 { let _ = state.stop(); }
                me.show_error_toast("Phone sharing is not available yet.".to_owned(), ctx);
                me.refresh_omw_phone_share_ui(ctx);
                return;
            };
            let Some(handle) = crate::omw::pane_auto_share::share_self_pane(me, ctx, registry, runtime) else {
                if surface_first_pair && state.share_count() == 0 { let _ = state.stop(); }
                me.show_error_toast("This pane could not be shared with a phone.".to_owned(), ctx);
                me.refresh_omw_phone_share_ui(ctx);
                return;
            };
            if state.store_pane_share(me.view_id(), handle) && surface_first_pair {
                super::use_agent_footer::surface_pair_modal(&state, ctx);
            }
            me.refresh_omw_phone_share_ui(ctx);
        });
    }

    pub(crate) fn cancel_pending_omw_phone_share(&mut self) {
        let stop_empty_daemon = self.omw_phone_share_pending_generation.is_some() && self.omw_phone_share_pending_started_daemon;
        if let Some(active) = self.omw_phone_share_pending_active.take() { active.store(false, Ordering::Release); }
        self.omw_phone_share_generation = self.omw_phone_share_generation.wrapping_add(1);
        self.omw_phone_share_pending_generation = None;
        self.omw_phone_share_pending_started_daemon = false;
        if stop_empty_daemon {
            let state = OmwRemoteState::shared();
            if state.share_count() == 0 { let _ = state.stop(); }
        }
    }
}
