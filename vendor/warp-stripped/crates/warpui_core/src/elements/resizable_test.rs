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

use super::*;

use crate::{
    App, AppContext, Entity, Event, Presenter, TypedActionView, WindowInvalidation,
    elements::{ConstrainedBox, DispatchEventResult, EventHandler, Rect, ZIndex},
    platform::WindowStyle,
};
use pathfinder_color::ColorU;
use pathfinder_geometry::vector::vec2f;
use std::{
    cell::RefCell,
    collections::HashSet,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

struct View {
    state: ResizableStateHandle,
    starts: Arc<AtomicUsize>,
    resizes: Arc<AtomicUsize>,
    ends: Arc<AtomicUsize>,
    child_mouse_downs: Arc<AtomicUsize>,
}

impl Default for View {
    fn default() -> Self {
        Self {
            state: resizable_state_handle(100.),
            starts: Arc::default(),
            resizes: Arc::default(),
            ends: Arc::default(),
            child_mouse_downs: Arc::default(),
        }
    }
}

impl Entity for View {
    type Event = ();
}

impl crate::core::View for View {
    fn ui_name() -> &'static str {
        "resizable_test_view"
    }

    fn render(&self, _: &AppContext) -> Box<dyn Element> {
        let child_mouse_downs = self.child_mouse_downs.clone();
        let child = EventHandler::new(
            ConstrainedBox::new(Rect::new().finish())
                .with_width(100.)
                .with_height(100.)
                .finish(),
        )
        .on_left_mouse_down(move |_, _, _| {
            child_mouse_downs.fetch_add(1, Ordering::SeqCst);
            DispatchEventResult::StopPropagation
        })
        .finish();

        let starts = self.starts.clone();
        let resizes = self.resizes.clone();
        let ends = self.ends.clone();
        Resizable::new(self.state.clone(), child)
            .with_dragbar_side(DragBarSide::Left)
            .with_bounds_callback(Box::new(|_| (60., 140.)))
            .on_start_resizing(move |_, _| {
                starts.fetch_add(1, Ordering::SeqCst);
            })
            .on_resize(move |_, _| {
                resizes.fetch_add(1, Ordering::SeqCst);
            })
            .on_end_resizing(move |_, _| {
                ends.fetch_add(1, Ordering::SeqCst);
            })
            .finish()
    }
}

impl TypedActionView for View {
    type Action = ();
}

#[test]
fn dragbar_captures_the_full_resize_lifecycle() {
    App::test((), |mut app| async move {
        let (window_id, view) = app.add_window(WindowStyle::NotStealFocus, |_| View::default());
        let mut presenter = Presenter::new(window_id);
        let mut updated = HashSet::new();
        updated.insert(app.root_view_id(window_id).unwrap());
        let invalidation = WindowInvalidation {
            updated,
            ..Default::default()
        };

        app.update(move |ctx| {
            presenter.invalidate(invalidation, ctx);
            let scene = presenter.build_scene(vec2f(300., 100.), 1., None, ctx);
            assert_eq!(scene.z_index(), ZIndex::new(0));
            let presenter = Rc::new(RefCell::new(presenter));

            ctx.simulate_window_event(
                Event::MouseMoved {
                    position: vec2f(2., 50.),
                    cmd: false,
                    shift: false,
                    is_synthetic: false,
                },
                window_id,
                presenter.clone(),
            );
            ctx.simulate_window_event(
                Event::LeftMouseDown {
                    position: vec2f(2., 50.),
                    modifiers: Default::default(),
                    click_count: 1,
                    is_first_mouse: false,
                },
                window_id,
                presenter.clone(),
            );
            ctx.simulate_window_event(
                Event::LeftMouseDragged {
                    position: vec2f(22., 50.),
                    modifiers: Default::default(),
                },
                window_id,
                presenter.clone(),
            );
            ctx.simulate_window_event(
                Event::LeftMouseDragged {
                    position: vec2f(200., 50.),
                    modifiers: Default::default(),
                },
                window_id,
                presenter.clone(),
            );
            ctx.simulate_window_event(
                Event::LeftMouseDragged {
                    position: vec2f(-100., 50.),
                    modifiers: Default::default(),
                },
                window_id,
                presenter.clone(),
            );
            ctx.simulate_window_event(
                Event::LeftMouseUp {
                    position: vec2f(-100., 50.),
                    modifiers: Default::default(),
                },
                window_id,
                presenter,
            );
        });

        view.read(&app, |view, _| {
            assert_eq!(view.starts.load(Ordering::SeqCst), 1);
            assert_eq!(view.resizes.load(Ordering::SeqCst), 3);
            assert_eq!(view.ends.load(Ordering::SeqCst), 1);
            assert_eq!(view.child_mouse_downs.load(Ordering::SeqCst), 0);

            let state = view.state.lock().unwrap();
            assert_eq!(state.size(), 140.);
            assert!(!state.is_resizing());
            assert!(!state.hovering_dragbar);
        });
    });
}

#[test]
fn dragbar_appearance_defaults_are_backward_compatible_and_configurable() {
    let hover_color = Fill::Solid(ColorU::new(10, 20, 30, 255));
    let resizable = Resizable::new(resizable_state_handle(100.), Rect::new().finish())
        .with_dragbar_width(8.)
        .with_dragbar_visual_width(1.)
        .with_dragbar_hover_color(hover_color);

    assert_eq!(resizable.dragbar.width, 8.);
    assert_eq!(resizable.dragbar.visual_width, 1.);
    assert_eq!(resizable.dragbar.hover_color, Some(hover_color));

    let defaults = Resizable::new(resizable_state_handle(100.), Rect::new().finish());
    assert_eq!(defaults.dragbar.width, DRAGBAR_WIDTH);
    assert_eq!(defaults.dragbar.visual_width, DRAGBAR_WIDTH);
    assert_eq!(defaults.dragbar.hover_color, None);
}
