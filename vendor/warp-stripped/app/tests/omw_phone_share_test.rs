// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 Shenhao Miao and the omw contributors
// Copyright (C) 2020-2026 Denver Technologies, Inc.
// This file is part of the AGPL-3.0 derivative work. See LICENSE.

#![cfg(all(feature = "omw_local", target_os = "macos"))]

use futures::StreamExt;
use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
use std::time::Duration;
use warp::test_exports::{
    phone_share_transition, should_unshare_for_detach, until_view_dropped, DetachType,
    OmwRemoteState, OmwRemoteStatus, PaneShareHandle, PhoneSharePresentation, PhoneShareTransition,
};
use warpui::EntityId;

fn running() -> OmwRemoteStatus {
    OmwRemoteStatus::Running { pair_url: "http://127.0.0.1/pair?t=qa-secret".into(), tailscale_serving: false }
}

#[test]
fn transition_preserves_other_shares_and_retries_failed_startup() {
    assert_eq!(phone_share_transition(true, &OmwRemoteStatus::Stopped, false, 0), PhoneShareTransition::StartAndShare);
    assert_eq!(phone_share_transition(true, &OmwRemoteStatus::Failed { error: "qa".into() }, false, 0), PhoneShareTransition::StartAndShare);
    assert_eq!(phone_share_transition(true, &OmwRemoteStatus::Starting, false, 0), PhoneShareTransition::Starting);
    assert_eq!(phone_share_transition(true, &running(), false, 1), PhoneShareTransition::Share);
    assert_eq!(phone_share_transition(true, &running(), true, 1), PhoneShareTransition::Unshare { stop_daemon: true });
    assert_eq!(phone_share_transition(true, &running(), true, 2), PhoneShareTransition::Unshare { stop_daemon: false });
    assert_eq!(phone_share_transition(false, &running(), false, 0), PhoneShareTransition::Unavailable);
}

#[test]
fn controls_reflect_this_pane_and_disable_during_pending_work() {
    let a = PhoneSharePresentation::from_state(&running(), true, false);
    let b = PhoneSharePresentation::from_state(&running(), false, false);
    assert!(a.active && !b.active);
    assert_eq!(a.label, "Stop sharing");
    assert_eq!(b.label, "Share with phone");
    for status in [OmwRemoteStatus::Starting, running()] {
        let pending = PhoneSharePresentation::from_state(&status, false, true);
        assert!(pending.disabled && !pending.active);
        assert_eq!(pending.label, "Starting...");
    }
}

#[test]
fn pane_moves_preserve_sharing_while_close_and_hide_release_it() {
    assert!(!should_unshare_for_detach(DetachType::Moved));
    assert!(should_unshare_for_detach(DetachType::Closed));
    assert!(should_unshare_for_detach(DetachType::HiddenForClose));
}

#[tokio::test]
async fn status_and_share_subscriptions_end_when_the_view_is_dropped() {
    let state = OmwRemoteState::new_for_test();
    let (tx, rx) = async_channel::bounded(1);
    let mut status = Box::pin(until_view_dropped(state.phone_status_stream(), rx.clone()));
    let mut shares = Box::pin(until_view_dropped(state.phone_share_stream(), rx));
    assert!(matches!(status.next().await, Some(OmwRemoteStatus::Stopped)));
    assert_eq!(shares.next().await, Some(0));
    state.set_status_for_test(running());
    assert!(matches!(status.next().await, Some(OmwRemoteStatus::Running { .. })));
    drop(tx);
    assert!(tokio::time::timeout(Duration::from_secs(1), status.next()).await.unwrap().is_none());
    assert!(tokio::time::timeout(Duration::from_secs(1), shares.next()).await.unwrap().is_none());
    assert!(state.runtime_handle().is_none(), "header subscriptions must not start a daemon runtime");
}

#[tokio::test]
async fn two_pane_share_registry_is_isolated_and_releases_each_handle_once() {
    let state = OmwRemoteState::new_for_test();
    let a = EntityId::from_usize(10);
    let b = EntityId::from_usize(20);
    let drops = Arc::new(AtomicUsize::new(0));
    let mut changes = Box::pin(state.phone_share_stream());
    let initial = changes.next().await.unwrap();
    for id in [a, b] {
        let drops = drops.clone();
        assert!(state.store_pane_share(id, PaneShareHandle::new_for_test(uuid::Uuid::new_v4(), move || { drops.fetch_add(1, Ordering::Relaxed); })));
    }
    assert_ne!(changes.next().await.unwrap(), initial);
    assert_eq!(state.share_count(), 2);
    state.unshare_pane(a);
    assert!(!state.is_pane_shared(a) && state.is_pane_shared(b));
    assert_eq!(drops.load(Ordering::Relaxed), 1);
    state.unshare_pane(a);
    assert_eq!(drops.load(Ordering::Relaxed), 1);
    state.unshare_pane(b);
    assert_eq!(state.share_count(), 0);
    assert_eq!(drops.load(Ordering::Relaxed), 2);
}
