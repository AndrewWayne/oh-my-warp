// SPDX-License-Identifier: AGPL-3.0-only
//
// omw-authored tests in the in-tree upstream fork, part of the AGPL-3.0
// derivative work. See specs/fork-strategy.md section 3.
// Copyright (C) 2026 omw contributors
// Copyright (C) 2020-2026 Denver Technologies, Inc.

#![cfg(all(feature = "test-exports", feature = "local_fs"))]

use std::path::PathBuf;
use warp::terminal::{links::should_scan_file_paths, view::TerminalEditor, CLIAgent};
use warp::test_exports::{osc8_target, CodeSource, EditorLayout, FileTarget, Osc8Target};
use warp_util::path::LineAndColumnArg;

#[test]
fn codex_mouse_reporting_keeps_file_path_scanning_on_mac() {
    assert_eq!(
        should_scan_file_paths(TerminalEditor::Yes, Some(CLIAgent::Codex)),
        cfg!(all(target_os = "macos", feature = "omw_local"))
    );
    assert!(should_scan_file_paths(TerminalEditor::No, None));
    assert!(should_scan_file_paths(
        TerminalEditor::No,
        Some(CLIAgent::Codex)
    ));
    for agent in [None, Some(CLIAgent::Unknown), Some(CLIAgent::Claude)] {
        assert!(!should_scan_file_paths(TerminalEditor::Yes, agent));
    }
}

#[cfg(all(target_os = "macos", feature = "omw_local"))]
#[test]
fn complete_chinese_path_in_mouse_reporting_terminal_is_scanned() {
    use warp::terminal::event_listener::ChannelEventListener;
    use warp::terminal::model::{
        ansi::Processor,
        blockgrid::BlockGrid,
        grid::grid_handler::{PerformResetGridChecks, TermMode},
        index::Point,
        ObfuscateSecrets,
    };
    use warp::terminal::SizeInfo;

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join(concat!(
        "AI_Learn/02_concepts/",
        "\u{670d}\u{52a1}\u{5668}\u{8fd0}\u{7ef4}\u{4e0e}AI",
        "\u{5de5}\u{5177}\u{5165}\u{95e8}\u{8bb2}\u{4e49}.md",
    ));
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "fixture").unwrap();
    let text = format!("\x1b[?1006h\x1b[36m{}\x1b[0m", file.display());

    for columns in [160, 240] {
        let (wakeups, _wakeup_rx) = async_channel::unbounded();
        let (events, _event_rx) = async_channel::unbounded();
        let (reads, _read_rx) = async_broadcast::broadcast(16);
        let listener = ChannelEventListener::new(wakeups, events, reads);
        let mut grid = BlockGrid::new(
            SizeInfo::new_without_font_metrics(10, columns),
            1000,
            listener,
            ObfuscateSecrets::No,
            PerformResetGridChecks::No,
        );
        grid.start();
        Processor::new().parse_bytes(&mut grid, text.as_bytes(), &mut std::io::sink());

        let handler = grid.grid_handler();
        assert_eq!(
            handler.cursor_point().row,
            0,
            "the entire path must fit on one row"
        );
        assert!(handler.is_mode_set(TermMode::SGR_MOUSE));
        assert!(should_scan_file_paths(
            TerminalEditor::Yes,
            Some(CLIAgent::Codex)
        ));
        for col in 0..handler.cursor_point().col {
            let point = Point::new(0, col);
            let candidates = handler.possible_file_paths_at_point(point);
            let candidate = candidates
                .iter()
                .find(|candidate| candidate.path.path == file.to_str().unwrap())
                .expect("every cell of the unwrapped Chinese path must resolve to the real file");
            assert!(candidate.range.contains(&point));
            assert_eq!(
                candidate.range,
                Point::new(0, 0)..=Point::new(0, handler.cursor_point().col - 1),
                "hovering either half of a Chinese character must highlight through the final d"
            );
            assert!(std::path::Path::new(&candidate.path.path).is_file());
        }
    }
}

#[cfg(all(target_os = "macos", feature = "omw_local"))]
fn file_path_fixture_grid(
    columns: usize,
    text: &str,
) -> warp::terminal::model::blockgrid::BlockGrid {
    use warp::terminal::event_listener::ChannelEventListener;
    use warp::terminal::model::{
        ansi::Processor, blockgrid::BlockGrid, grid::grid_handler::PerformResetGridChecks,
        ObfuscateSecrets,
    };
    use warp::terminal::SizeInfo;

    let (wakeups, _wakeup_rx) = async_channel::unbounded();
    let (events, _event_rx) = async_channel::unbounded();
    let (reads, _read_rx) = async_broadcast::broadcast(16);
    let listener = ChannelEventListener::new(wakeups, events, reads);
    let mut grid = BlockGrid::new(
        SizeInfo::new_without_font_metrics(24, columns),
        1000,
        listener,
        ObfuscateSecrets::No,
        PerformResetGridChecks::No,
    );
    grid.start();
    Processor::new().parse_bytes(&mut grid, text.as_bytes(), &mut std::io::sink());
    grid
}

#[cfg(all(target_os = "macos", feature = "omw_local"))]
#[test]
fn soft_wrapped_chinese_path_has_one_complete_target_and_range() {
    use warp::terminal::model::{cell::Flags, index::Point};
    let path = concat!(
        "/Users/shuokong/Desktop/cailab/AI_Learn/02_concepts/",
        "\u{670d}\u{52a1}\u{5668}\u{8fd0}\u{7ef4}\u{4e0e}AI",
        "\u{5de5}\u{5177}\u{5165}\u{95e8}\u{8bb2}\u{4e49}.md"
    );

    for columns in [24, 40, 60, 80] {
        let grid = file_path_fixture_grid(columns, &format!("\x1b[?1006h{path}"));
        let handler = grid.grid_handler();
        let mut points = Vec::new();
        for row in 0..24 {
            let grid_row = handler.row(row).unwrap();
            for col in 0..columns {
                let cell = grid_row.get(col).unwrap();
                if cell.c != ' ' && cell.c != '\0'
                    || cell
                        .flags
                        .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
                {
                    points.push(Point::new(row, col));
                }
            }
        }
        let range = *points.first().unwrap()..=*points.last().unwrap();
        for point in points {
            let candidates = handler.possible_file_paths_at_point(point);
            let candidate = candidates
                .iter()
                .find(|candidate| candidate.path.path == path)
                .unwrap_or_else(|| {
                    panic!("missing full soft-wrapped path at {columns} columns, {point:?}")
                });
            assert_eq!(
                candidate.range, range,
                "full soft-wrapped link must include the final d"
            );
        }
    }
}

#[cfg(all(target_os = "macos", feature = "omw_local"))]
#[test]
fn codex_hard_wrap_inside_chinese_filename_keeps_the_complete_path() {
    use unicode_width::UnicodeWidthChar;
    use warp::terminal::model::index::Point;
    let path = concat!(
        "/Users/shuokong/Desktop/cailab/AI_Learn/02_concepts/",
        "\u{670d}\u{52a1}\u{5668}\u{8fd0}\u{7ef4}\u{4e0e}AI",
        "\u{5de5}\u{5177}\u{5165}\u{95e8}\u{8bb2}\u{4e49}.md"
    );

    for columns in [24, 40, 60, 80] {
        let mut fixture = "\x1b[?1006h".to_owned();
        let mut row = 0;
        let mut col = 2;
        let mut points = Vec::new();
        fixture.push_str("\x1b[1;3H");
        for character in path.chars() {
            let width = character.width().unwrap_or(1).max(1);
            if col + width > columns - 2 {
                row += 1;
                col = 2;
                fixture.push_str(&format!("\x1b[{};3H", row + 1));
            }
            fixture.push(character);
            points.extend((col..col + width).map(|column| Point::new(row, column)));
            col += width;
        }
        let grid = file_path_fixture_grid(columns, &fixture);
        let handler = grid.grid_handler();
        let range = *points.first().unwrap()..=*points.last().unwrap();
        for point in points {
            let candidates = handler.possible_file_paths_at_point(point);
            let candidate = candidates
                .iter()
                .find(|candidate| candidate.path.path == path)
                .unwrap_or_else(|| {
                    panic!("missing full Codex hard-wrapped path at {columns} columns, {point:?}")
                });
            assert_eq!(
                candidate.range, range,
                "every row must highlight through the final d"
            );
        }
    }
}

#[cfg(all(target_os = "macos", feature = "omw_local"))]
#[test]
fn legacy_codex_directory_wrapping_and_unrelated_line_boundaries_are_preserved() {
    use warp::terminal::model::index::Point;

    let grid = file_path_fixture_grid(80, "\u{2022} root/segment/\r\n  child/\r\n  file.html");
    for point in [Point::new(0, 4), Point::new(1, 4), Point::new(2, 4)] {
        let paths = grid.grid_handler().possible_file_paths_at_point(point);
        let candidate = paths
            .iter()
            .find(|candidate| candidate.path.path == "root/segment/child/file.html")
            .expect("the 0.0.11 directory-boundary continuation rule must remain working");
        assert_eq!(candidate.range, Point::new(0, 2)..=Point::new(2, 10));
    }

    for text in [
        "/root/segment/\r\nchild/file.md",
        "/root/first.md\r\n  second.md",
    ] {
        let grid = file_path_fixture_grid(80, text);
        for point in [Point::new(0, 4), Point::new(1, 4)] {
            let paths = grid.grid_handler().possible_file_paths_at_point(point);
            assert!(
                paths
                    .iter()
                    .all(|candidate| !candidate.range.contains(&Point::new(0, 4))
                        || !candidate.range.contains(&Point::new(1, 4))),
                "ordinary newlines must not be converted to path continuations"
            );
        }
    }
}

#[cfg(all(target_os = "macos", feature = "omw_local"))]
#[test]
fn captured_codex_user_and_assistant_paths_have_complete_ranges_at_each_width() {
    use unicode_width::UnicodeWidthChar;
    use warp::terminal::model::index::Point;

    let captured: serde_json::Value = serde_json::from_str(include_str!(
        "data/omw-file-links/codex-0.161.0-visible-paths.json"
    ))
    .unwrap();
    let path = concat!(
        "/Users/shuokong/Desktop/cailab/AI_Learn/02_concepts/",
        "\u{670d}\u{52a1}\u{5668}\u{8fd0}\u{7ef4}\u{4e0e}AI",
        "\u{5de5}\u{5177}\u{5165}\u{95e8}\u{8bb2}\u{4e49}.md"
    );
    for fixture in captured["fixtures"].as_array().unwrap() {
        let columns = fixture["columns"].as_u64().unwrap() as usize;
        for message in fixture["path_rows"].as_array().unwrap() {
            let lines: Vec<&str> = message
                .as_array()
                .unwrap()
                .iter()
                .map(|line| line.as_str().unwrap())
                .collect();
            let grid =
                file_path_fixture_grid(columns, &format!("\x1b[?1006h{}", lines.join("\r\n")));
            let mut points = Vec::new();
            for (row, line) in lines.iter().enumerate() {
                let width: usize = line.chars().map(|c| c.width().unwrap_or(1).max(1)).sum();
                points.extend((2..width).map(|col| Point::new(row, col)));
            }
            let expected_range = *points.first().unwrap()..=*points.last().unwrap();
            for point in points {
                let paths = grid.grid_handler().possible_file_paths_at_point(point);
                let candidate = paths
                    .iter()
                    .find(|candidate| candidate.path.path == path)
                    .unwrap_or_else(|| {
                        panic!(
                            "captured Codex path at {columns} columns is incomplete at {point:?}"
                        )
                    });
                assert_eq!(candidate.range, expected_range);
            }
        }
    }
}

#[cfg(all(target_os = "macos", feature = "omw_local"))]
#[test]
fn extended_wrap_reconstruction_keeps_absolute_path_and_row_limits() {
    use warp::terminal::model::index::Point;
    let relative = format!("root/{}file.md", "long_filename_".repeat(8));
    let grid = file_path_fixture_grid(24, &relative);
    assert!(
        grid.grid_handler()
            .possible_file_paths_at_point(Point::new(0, 0))
            .iter()
            .all(|candidate| candidate.path.path != relative),
        "the new continuation rules must not expand arbitrary relative path scanning"
    );

    let absolute = format!("/{relative}");
    let grid = file_path_fixture_grid(8, &absolute);
    assert!(
        grid.grid_handler()
            .possible_file_paths_at_point(Point::new(0, 0))
            .iter()
            .all(|candidate| candidate.path.path != absolute),
        "new reconstruction must keep the existing eight-row scan limit"
    );
}

#[cfg(all(target_os = "macos", feature = "omw_local"))]
#[test]
fn codex_relative_chinese_reference_wrap_keeps_filename_and_location() {
    use warp::terminal::model::index::Point;
    let path = concat!(
        "docs/",
        "\u{670d}\u{52a1}\u{5668}\u{8fd0}\u{7ef4}\u{4e0e}AI",
        "\u{5de5}\u{5177}\u{5165}\u{95e8}\u{8bb2}\u{4e49}.md"
    );
    let text = format!("  \u{2022} docs/\u{670d}\u{52a1}\u{5668}\u{8fd0}\u{7ef4}\u{4e0e}AI\u{5de5}\u{5177}\u{5165}\u{95e8}\u{8bb2}\r\n    \u{4e49}.md:1\u{ff1a}content");
    let grid = file_path_fixture_grid(40, &text);
    for point in [
        Point::new(0, 4),
        Point::new(0, 32),
        Point::new(1, 4),
        Point::new(1, 5),
        Point::new(1, 10),
    ] {
        let paths = grid
            .grid_handler()
            .possible_codex_file_paths_at_point(point);
        let candidate = paths
            .iter()
            .find(|p| p.path.path == path && p.path.line_and_column_num.is_some())
            .expect("Codex's relative reference must reconstruct the complete Chinese filename");
        assert_eq!(candidate.range, Point::new(0, 4)..=Point::new(1, 10));
        assert_eq!(candidate.path.line_and_column_num.unwrap().line_num, 1);
    }
}

#[cfg(all(target_os = "macos", feature = "omw_local"))]
#[test]
fn codex_hyphen_wrap_reconstructs_absolute_and_dot_relative_paths() {
    use warp::terminal::model::index::Point;
    for prefix in ["/Users/test/", "./", "../"] {
        let path = format!("{prefix}oh-my-warp/docs/file.md:12:3");
        let text = format!("\u{2022} {prefix}oh-my-\r\n  warp/docs/file.md:12:3");
        let grid = file_path_fixture_grid(40, &text);
        for point in [Point::new(0, 2), Point::new(1, 2), Point::new(1, 22)] {
            let paths = grid
                .grid_handler()
                .possible_codex_file_paths_at_point(point);
            let candidate = paths
                .iter()
                .find(|p| {
                    p.path.path == path.strip_suffix(":12:3").unwrap()
                        && p.path.line_and_column_num.is_some()
                })
                .expect("early hyphen wrap must retain the full Codex path");
            assert_eq!(candidate.range, Point::new(0, 2)..=Point::new(1, 23));
            assert_eq!(
                candidate.path.line_and_column_num.unwrap(),
                LineAndColumnArg {
                    line_num: 12,
                    column_num: Some(3)
                }
            );
        }
    }
}

#[cfg(all(target_os = "macos", feature = "omw_local"))]
#[test]
fn codex_rollout_directory_uses_current_turn_instead_of_saved_root() {
    let dir = tempfile::tempdir().unwrap();
    let saved = dir.path().join("main");
    let current = dir.path().join("worktree/app");
    std::fs::create_dir_all(&saved).unwrap();
    std::fs::create_dir_all(&current).unwrap();
    let rollout = dir.path().join("rollout-test.jsonl");
    let records = [
        serde_json::json!({"type":"session_meta","payload":{"cwd":saved}}),
        serde_json::json!({"type":"turn_context","payload":{"cwd":saved}}),
        serde_json::json!({"type":"turn_context","payload":{"cwd":current}}),
    ];
    let mut text = records.iter().map(|v| format!("{v}\n")).collect::<String>();
    text.push_str("{\"type\":\"turn_context\",\"payload\":");
    std::fs::write(&rollout, text).unwrap();
    assert_eq!(
        warp::terminal::codex_file_links::directory_from_rollout(&rollout),
        Some(current)
    );
}

#[cfg(all(target_os = "macos", feature = "omw_local"))]
#[test]
fn codex_unknown_or_unbounded_context_does_not_guess_a_directory() {
    let dir = tempfile::tempdir().unwrap();
    let rollout = dir.path().join("rollout-test.jsonl");
    for cwd in ["relative/project", "/nonexistent-omw-path-fixture"] {
        std::fs::write(
            &rollout,
            format!(
                "{}\n",
                serde_json::json!({"type":"session_meta","payload":{"cwd":cwd}})
            ),
        )
        .unwrap();
        assert!(warp::terminal::codex_file_links::directory_from_rollout(&rollout).is_none());
    }
    let meta = serde_json::json!({"type":"session_meta","payload":{"cwd":dir.path()}});
    std::fs::write(
        &rollout,
        format!("{meta}\n{}\n", "x".repeat(2 * 1024 * 1024)),
    )
    .unwrap();
    assert!(
        warp::terminal::codex_file_links::directory_from_rollout(&rollout).is_none(),
        "a bounded tail missing current context must not use stale initial metadata"
    );
}

#[cfg(all(target_os = "macos", feature = "omw_local"))]
#[test]
fn codex_explicit_directory_and_worktree_context_choose_correct_duplicate() {
    use std::ffi::OsString;
    use warp::terminal::codex_file_links::directory_for_process;
    use warp::util::file::{absolute_path_if_valid, ShellPathType};
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("main");
    let worktree = dir.path().join("linked/app");
    for root in [&main, &worktree] {
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/main.py"), root.to_str().unwrap()).unwrap();
    }
    let rollout = dir.path().join("rollout-test.jsonl");
    std::fs::write(
        &rollout,
        format!(
            "{}\n",
            serde_json::json!({"type":"turn_context","payload":{"cwd":worktree}})
        ),
    )
    .unwrap();
    for args in [
        vec!["codex", "--worktree"],
        vec!["codex", "--cd", "../linked/app", "resume", "thread"],
    ] {
        let args: Vec<_> = args.into_iter().map(OsString::from).collect();
        let cwd = directory_for_process(Some(&main), &args, Some(&rollout)).unwrap();
        let path = warp_util::path::CleanPathResult::with_line_and_column_number("src/main.py:1");
        let target = absolute_path_if_valid(
            &path,
            ShellPathType::PlatformNative(cwd.to_str().unwrap().into()),
            None,
        )
        .unwrap();
        assert_eq!(
            target.canonicalize().unwrap(),
            worktree.join("src/main.py").canonicalize().unwrap()
        );
    }
    let args: Vec<_> = ["codex", "-C", main.to_str().unwrap(), "resume", "thread"]
        .into_iter()
        .map(OsString::from)
        .collect();
    assert_eq!(
        directory_for_process(Some(&worktree), &args, Some(&rollout)),
        Some(main)
    );
    assert!(
        directory_for_process(Some(&worktree), &[OsString::from("codex")], None).is_none(),
        "unknown active session must not silently use process CWD"
    );
}

#[cfg(all(target_os = "macos", feature = "omw_local"))]
#[test]
fn codex_reconstruction_keeps_limits_and_unrelated_lines_separate() {
    use warp::terminal::model::index::Point;
    for text in [
        "docs/first.md\r\n  second.md",
        "docs/path-\r\n\r\n  file.md",
        "docs/path-\r\n  \u{2022} other/file.md",
    ] {
        let grid = file_path_fixture_grid(80, text);
        assert!(grid
            .grid_handler()
            .possible_codex_file_paths_at_point(Point::new(0, 1))
            .iter()
            .all(|path| path.range.start().row == path.range.end().row));
    }
    let long = format!("docs/{}file.md", "segment-".repeat(20));
    let grid = file_path_fixture_grid(8, &long);
    assert!(grid
        .grid_handler()
        .possible_codex_file_paths_at_point(Point::new(0, 1))
        .iter()
        .all(|path| path.path.path != long));
    let grid = file_path_fixture_grid(
        1600,
        &format!("{}-\r\n  {}", "a/".repeat(700), "b/".repeat(700)),
    );
    assert!(
        grid.grid_handler()
            .possible_codex_file_paths_at_point(Point::new(0, 1))
            .len()
            < 1024
    );
}

#[cfg(all(target_os = "macos", feature = "omw_local"))]
#[test]
fn codex_soft_wrap_preserves_spaces_in_filenames() {
    use warp::terminal::model::index::Point;
    let path = "docs/file with spaces.md:12";
    let grid = file_path_fixture_grid(10, path);
    for point in [Point::new(0, 0), Point::new(1, 0), Point::new(2, 6)] {
        let paths = grid
            .grid_handler()
            .possible_codex_file_paths_at_point(point);
        let candidate = paths
            .iter()
            .find(|p| p.path.path == "docs/file with spaces.md")
            .expect("soft wraps must keep filename spaces at the right margin");
        assert_eq!(candidate.range, Point::new(0, 0)..=Point::new(2, 6));
    }
}

#[cfg(all(target_os = "macos", feature = "omw_local"))]
#[test]
fn codex_narrow_path_list_keeps_each_reference_start() {
    use warp::terminal::model::index::Point;
    let path = "docs/\u{670d}\u{52a1}\u{5668}\u{8fd0}\u{7ef4}\u{4e0e}AI\u{5de5}\u{5177}\u{5165}\u{95e8}\u{8bb2}\u{4e49}.md";
    let text = "\u{2022} src/main.py\r\n  config/settings.json\r\n  docs/\u{670d}\u{52a1}\u{5668}\u{8fd0}\u{7ef4}\u{4e0e}AI\u{5de5}\r\n  \u{5177}\u{5165}\u{95e8}\u{8bb2}\u{4e49}.md";
    let grid = file_path_fixture_grid(24, text);
    for point in [
        Point::new(2, 2),
        Point::new(2, 22),
        Point::new(3, 2),
        Point::new(3, 14),
    ] {
        let paths = grid
            .grid_handler()
            .possible_codex_file_paths_at_point(point);
        let candidate = paths
            .iter()
            .find(|p| p.path.path == path)
            .expect("a full-width preceding filename must not swallow the next reference start");
        assert_eq!(candidate.range, Point::new(2, 2)..=Point::new(3, 14));
    }
}

#[cfg(all(target_os = "macos", feature = "omw_local"))]
#[test]
fn codex_directory_flags_in_prompt_or_config_are_not_launch_options() {
    use std::ffi::OsString;
    use warp::terminal::codex_file_links::directory_for_process;
    let dir = tempfile::tempdir().unwrap();
    let original = dir.path().join("original");
    let misleading = dir.path().join("misleading");
    std::fs::create_dir_all(&original).unwrap();
    std::fs::create_dir_all(&misleading).unwrap();
    let rollout = dir.path().join("rollout-test.jsonl");
    std::fs::write(
        &rollout,
        format!(
            "{}\n",
            serde_json::json!({"type":"turn_context","payload":{"cwd":original}})
        ),
    )
    .unwrap();
    for args in [
        vec!["codex", "explain", "--cd", misleading.to_str().unwrap()],
        vec!["codex", "-c", "--cd", misleading.to_str().unwrap()],
        vec![
            "codex",
            "resume",
            "--last",
            "explain",
            "--cd",
            misleading.to_str().unwrap(),
        ],
    ] {
        let args: Vec<_> = args.into_iter().map(OsString::from).collect();
        assert_eq!(
            directory_for_process(Some(dir.path()), &args, Some(&rollout)),
            Some(original.clone())
        );
    }
}

#[cfg(all(target_os = "macos", feature = "omw_local"))]
#[test]
fn codex_resumed_directory_reads_only_its_fresh_exact_state_row() {
    use diesel::{connection::SimpleConnection, Connection, RunQueryDsl, SqliteConnection};
    use warp::terminal::codex_file_links::directory_from_state;
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("state_5.sqlite");
    let rollout = dir.path().join("rollout-test.jsonl");
    let saved = dir.path().join("saved");
    let chosen = dir.path().join("chosen");
    std::fs::create_dir_all(&saved).unwrap();
    std::fs::create_dir_all(&chosen).unwrap();
    let mut connection = SqliteConnection::establish(database.to_str().unwrap()).unwrap();
    connection
        .batch_execute("CREATE TABLE threads (rollout_path TEXT, cwd TEXT, updated_at_ms INTEGER);")
        .unwrap();
    for (path, cwd, updated) in [
        (
            rollout.to_str().unwrap(),
            chosen.to_str().unwrap(),
            2000_i64,
        ),
        ("unrelated.jsonl", saved.to_str().unwrap(), 999999_i64),
    ] {
        diesel::sql_query("INSERT INTO threads VALUES (?, ?, ?)")
            .bind::<diesel::sql_types::Text, _>(path)
            .bind::<diesel::sql_types::Text, _>(cwd)
            .bind::<diesel::sql_types::BigInt, _>(updated)
            .execute(&mut connection)
            .unwrap();
    }
    drop(connection);
    let before = std::fs::read(&database).unwrap();
    assert_eq!(directory_from_state(&database, &rollout, 1), Some(chosen));
    assert!(
        directory_from_state(&database, &rollout, 3).is_none(),
        "an old state row must not establish the resumed directory"
    );
    assert!(directory_from_state(&database, &dir.path().join("unknown.jsonl"), 0).is_none());
    assert_eq!(
        before,
        std::fs::read(&database).unwrap(),
        "the external state database must remain unchanged"
    );
    let missing = dir.path().join("missing.sqlite");
    assert!(directory_from_state(&missing, &rollout, 0).is_none());
    assert!(
        !missing.exists(),
        "read-only lookup must never create a database"
    );
}

#[test]
fn an_existing_filename_with_colon_digits_keeps_its_original_target() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("file:12");
    std::fs::write(&file, "fixture").unwrap();
    let destination = url::Url::from_file_path(&file).unwrap();
    let Some(Osc8Target::File(path, location)) = osc8_target(destination.as_str()) else {
        panic!("expected existing local file");
    };
    assert_eq!(path, file);
    assert_eq!(location, None);
}

#[test]
fn codex_file_url_ranges_keep_start_location() {
    for suffix in ["#L12C3-L20C9", ":12:3-20:9", ":12:3", "#L12:3"] {
        let destination = format!("file:///Users/test/My%20file.rs{suffix}");
        let Some(Osc8Target::File(path, location)) = osc8_target(&destination) else {
            panic!("expected local file target");
        };
        assert_eq!(path, PathBuf::from("/Users/test/My file.rs"));
        assert_eq!(
            location,
            Some(LineAndColumnArg {
                line_num: 12,
                column_num: Some(3)
            })
        );
    }
}

#[test]
fn escaped_filename_characters_are_not_locations() {
    for (destination, expected) in [
        ("file:///tmp/file%3A12%23L20.rs", "/tmp/file:12#L20.rs"),
        ("file:///tmp/file%3A12", "/tmp/file:12"),
        (
            "file:///Users/test/My%20%E6%96%87%E4%BB%B6.md",
            "/Users/test/My \u{6587}\u{4ef6}.md",
        ),
    ] {
        let Some(Osc8Target::File(path, location)) = osc8_target(destination) else {
            panic!("expected local file target");
        };
        assert_eq!(path, PathBuf::from(expected));
        assert_eq!(location, None);
    }
}

#[test]
fn web_links_keep_destination_and_unsupported_schemes_are_rejected() {
    assert!(matches!(
        osc8_target("https://example.com/path"),
        Some(Osc8Target::Web("https://example.com/path"))
    ));
    assert!(osc8_target("javascript:alert(1)").is_none());
    assert!(osc8_target("file://remote.example/tmp/file.rs").is_none());
}

#[test]
fn anchored_markdown_opens_at_a_source_location_without_changing_plain_preview() {
    let location = Some(LineAndColumnArg {
        line_num: 12,
        column_num: Some(3),
    });
    for layout in [EditorLayout::NewTab, EditorLayout::SplitPane] {
        assert_eq!(
            FileTarget::MarkdownViewer(layout).with_line_column(location),
            FileTarget::CodeEditor(layout)
        );
        assert_eq!(
            FileTarget::MarkdownViewer(layout).with_line_column(None),
            FileTarget::MarkdownViewer(layout)
        );
        for target in [
            FileTarget::EnvEditor,
            FileTarget::SystemDefault,
            FileTarget::CodeEditor(layout),
        ] {
            assert_eq!(target.clone().with_line_column(location), target);
        }
    }
}

#[test]
fn link_source_exposes_range_start_without_changing_origin() {
    let source = CodeSource::Link {
        path: PathBuf::from("/tmp/controlled.md"),
        range_start: Some(LineAndColumnArg {
            line_num: 12,
            column_num: Some(3),
        }),
        range_end: Some(LineAndColumnArg {
            line_num: 20,
            column_num: Some(9),
        }),
    };
    assert_eq!(
        source.clone().line_col(),
        Some(LineAndColumnArg {
            line_num: 12,
            column_num: Some(3)
        })
    );
    assert_eq!(
        source.clone().path(),
        Some(PathBuf::from("/tmp/controlled.md"))
    );
    let finder_source = CodeSource::Finder {
        path: PathBuf::from("/tmp/controlled.md"),
    };
    assert_eq!(finder_source.line_col(), None);
    assert_eq!(finder_source.telemetry_source_name(), "finder");
}
