use std::sync::Arc;
use warp_core::ui::appearance::Appearance;
use warp_editor::render::element::VerticalExpansionBehavior;
use warpui::{
    elements::{new_scrollable::ScrollableAppearance, ScrollbarWidth},
    platform::WindowStyle,
    App, TypedActionView, ViewHandle, WindowId,
};

#[cfg(target_os = "windows")]
use crate::code::editor::element::{line_number_gutter_width, line_number_text_width};
use crate::{
    cloud_object::model::persistence::CloudModel,
    editor::InteractionState,
    notebooks::editor::keys::NotebookKeybindings,
    server::server_api::{team::MockTeamClient, workspace::MockWorkspaceClient},
    settings_view::keybindings::KeybindingChangedNotifier,
    test_util::settings::initialize_settings_for_tests,
    vim_registers::VimRegisters,
    workspace::{sync_inputs::SyncedInputState, ActiveSession},
    workspaces::user_workspaces::UserWorkspaces,
    AuthStateProvider,
};

use super::{
    gutter_controls_min_width, max_displayed_line_number, CodeEditorRenderOptions, CodeEditorView,
    CodeEditorViewAction,
};
use warp_util::user_input::UserInput;

#[cfg(target_os = "windows")]
fn bundled_hack_font_bytes() -> Vec<Vec<u8>> {
    use std::{fs::read, path::PathBuf};

    [
        "Hack-Regular.ttf",
        "Hack-Italic.ttf",
        "Hack-Bold.ttf",
        "Hack-BoldItalic.ttf",
    ]
    .iter()
    .map(|font_file| {
        let path = [
            env!("CARGO_MANIFEST_DIR"),
            "assets",
            "bundled",
            "fonts",
            "hack",
            font_file,
        ]
        .iter()
        .collect::<PathBuf>();
        read(path).expect("bundled Hack font should be readable")
    })
    .collect()
}

#[cfg(target_os = "windows")]
fn load_real_hack_font_cache() -> (warpui::fonts::Cache, warpui::fonts::FamilyId) {
    let font_bytes = bundled_hack_font_bytes();

    let mut font_cache =
        warpui::fonts::Cache::new(Box::new(warpui::platform::current::FontDB::new()));
    let family = font_cache
        .load_family_from_bytes("Hack", font_bytes)
        .expect("bundled Hack font should load");
    (font_cache, family)
}

#[test]
fn line_number_gutter_uses_zero_based_last_line_index_with_offsets() {
    assert_eq!(max_displayed_line_number(None, 0), 1);
    assert_eq!(max_displayed_line_number(None, 8), 9);
    assert_eq!(max_displayed_line_number(None, 9), 10);
    assert_eq!(max_displayed_line_number(None, 98), 99);
    assert_eq!(max_displayed_line_number(None, 99), 100);
    assert_eq!(max_displayed_line_number(None, 998), 999);
    assert_eq!(max_displayed_line_number(None, 999), 1000);
    assert_eq!(max_displayed_line_number(Some(98), 2), 100);
    assert_eq!(max_displayed_line_number(Some(usize::MAX), 1), usize::MAX);
}

#[test]
fn line_number_gutter_reserves_every_configured_action() {
    assert_eq!(gutter_controls_min_width(20., 0, true, false), 0.);
    assert_eq!(gutter_controls_min_width(20., 0, true, true), 16.);
    assert_eq!(gutter_controls_min_width(20., 1, true, true), 30.);
    assert_eq!(gutter_controls_min_width(20., 2, true, true), 52.);
    assert_eq!(gutter_controls_min_width(20., 3, true, true), 74.);

    assert_eq!(gutter_controls_min_width(20., 1, false, true), 26.);
    assert_eq!(gutter_controls_min_width(20., 2, false, true), 44.);
    assert_eq!(gutter_controls_min_width(20., 3, false, true), 62.);
}

fn initialize_editor(app: &mut App) -> (WindowId, ViewHandle<CodeEditorView>) {
    initialize_settings_for_tests(app);

    // Add all required singleton models for EditorView dependencies
    app.add_singleton_model(|_| Appearance::mock());
    app.add_singleton_model(|_| SyncedInputState::mock());
    app.add_singleton_model(|_| VimRegisters::new());
    app.add_singleton_model(|_| KeybindingChangedNotifier::mock());
    app.add_singleton_model(|_| AuthStateProvider::new_for_test());

    // Add mocks required by rich text editor (used in CommentEditor)
    app.add_singleton_model(CloudModel::mock);
    app.add_singleton_model(|_| ActiveSession::default());
    app.add_singleton_model(NotebookKeybindings::new);

    // Add UserWorkspaces mock (required by EditorView)
    let team_client_mock = Arc::new(MockTeamClient::new());
    let workspace_client_mock = Arc::new(MockWorkspaceClient::new());
    app.add_singleton_model(|ctx| {
        UserWorkspaces::mock(
            team_client_mock.clone(),
            workspace_client_mock.clone(),
            vec![],
            ctx,
        )
    });

    let (window, editor_view) = app.add_window(WindowStyle::NotStealFocus, |ctx| {
        CodeEditorView::new(
            None,
            None,
            CodeEditorRenderOptions::new(VerticalExpansionBehavior::GrowToMaxHeight),
            ctx,
        )
        .with_horizontal_scrollbar_appearance(ScrollableAppearance::new(ScrollbarWidth::Auto, true))
    });

    (window, editor_view)
}

#[test]
fn test_interaction_state_prevents_editing() {
    App::test((), |mut app| async move {
        let (_window, editor_view) = initialize_editor(&mut app);

        let text = editor_view.update(&mut app, |view, ctx| {
            view.handle_action(&CodeEditorViewAction::UserTyped(UserInput::new("abc")), ctx);
            view.text(ctx)
        });

        assert_eq!(text.as_str(), "abc");

        // Set to be only selectable
        editor_view.update(&mut app, |view, ctx| {
            view.set_interaction_state(InteractionState::Selectable, ctx);
        });

        let text = editor_view.update(&mut app, |view, ctx| {
            view.handle_action(&CodeEditorViewAction::UserTyped(UserInput::new("def")), ctx);
            view.text(ctx)
        });

        assert_eq!(text.as_str(), "abc");
    });
}

#[test]
fn line_number_gutter_is_absent_when_line_numbers_are_hidden() {
    App::test((), |mut app| async move {
        let (_window, editor_view) = initialize_editor(&mut app);

        editor_view.update(&mut app, |view, ctx| {
            view.display_options.show_line_numbers = false;
            assert!(view.line_number_config(ctx).is_none());
        });
    });
}

#[test]
fn line_number_gutter_tracks_real_model_digit_boundaries() {
    #[cfg(target_os = "windows")]
    let (real_font_cache, real_font_family) = load_real_hack_font_cache();

    App::test((), move |mut app| async move {
        let (_window, editor_view) = initialize_editor(&mut app);
        let mut current_displayed_line_count = 1usize;
        let mut three_line_lens_model_width = None;
        let mut normal_model_widths = std::collections::BTreeMap::new();
        #[cfg(target_os = "windows")]
        let mut three_line_lens_real_width = None;
        #[cfg(target_os = "windows")]
        let mut normal_real_widths = std::collections::BTreeMap::new();

        for target_displayed_line_count in [3usize, 9, 10, 99, 100, 999, 1000] {
            let additional_lines = target_displayed_line_count - current_displayed_line_count;
            let inserted_newlines = "\n".repeat(additional_lines);

            editor_view.update(&mut app, |view, ctx| {
                if !inserted_newlines.is_empty() {
                    view.handle_action(
                        &CodeEditorViewAction::UserTyped(UserInput::new(inserted_newlines)),
                        ctx,
                    );
                }

                let zero_based_last_line_index = view.model.as_ref(ctx).line_count(ctx);
                assert_eq!(zero_based_last_line_index, target_displayed_line_count - 1);

                let expected_max_line_number = if target_displayed_line_count == 3 {
                    view.set_starting_line_number(Some(98));
                    100
                } else {
                    view.set_starting_line_number(None);
                    target_displayed_line_count
                };
                assert_eq!(
                    max_displayed_line_number(
                        view.display_options.starting_line_number,
                        zero_based_last_line_index,
                    ),
                    expected_max_line_number
                );

                let config = view
                    .line_number_config(ctx)
                    .expect("line numbers should be enabled");
                assert!(config.gutter_width.is_finite());
                assert!(
                    config.gutter_width >= 16.,
                    "the gutter must retain 8px padding on each side"
                );

                if target_displayed_line_count == 3 {
                    three_line_lens_model_width = Some(config.gutter_width);
                } else {
                    normal_model_widths.insert(target_displayed_line_count, config.gutter_width);
                }

                #[cfg(target_os = "windows")]
                {
                    // App::test intentionally uses a zero-width fake font backend. Measure the
                    // model-derived maximum with the real Windows backend for boundary assertions.
                    let shaped_width = line_number_text_width(
                        expected_max_line_number,
                        &real_font_cache,
                        real_font_family,
                        warpui::fonts::Properties::default(),
                        13.25,
                    );
                    let real_width = line_number_gutter_width(shaped_width, 0.);
                    if target_displayed_line_count == 3 {
                        three_line_lens_real_width = Some(real_width);
                    } else {
                        normal_real_widths.insert(target_displayed_line_count, real_width);
                    }
                }
            });

            current_displayed_line_count = target_displayed_line_count;
        }

        assert_eq!(
            normal_model_widths.get(&100).copied(),
            three_line_lens_model_width,
            "a lens beginning at 98 and a normal model ending at 100 must size identically"
        );

        #[cfg(target_os = "windows")]
        {
            let width = |line_count| {
                *normal_real_widths
                    .get(&line_count)
                    .expect("boundary width should have been measured")
            };
            assert!(width(9) < width(10), "gutter must grow at 9 -> 10");
            assert_eq!(width(10), width(99));
            assert!(width(99) < width(100), "gutter must grow at 99 -> 100");
            assert_eq!(width(100), width(999));
            assert!(width(999) < width(1000), "gutter must grow at 999 -> 1000");
            assert_eq!(
                Some(width(100)),
                three_line_lens_real_width,
                "the real-font lens and normal model must size identically"
            );
        }
    });
}
