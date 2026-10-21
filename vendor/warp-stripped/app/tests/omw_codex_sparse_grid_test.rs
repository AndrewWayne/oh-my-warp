// SPDX-License-Identifier: AGPL-3.0-only
#![cfg(all(target_os = "macos", feature = "omw_local"))]

use unicode_width::UnicodeWidthChar;
use warp::terminal::{
    event_listener::ChannelEventListener,
    model::{
        ansi::Processor, blockgrid::BlockGrid, grid::grid_handler::PerformResetGridChecks,
        index::Point, ObfuscateSecrets,
    },
    SizeInfo,
};

fn check_reference(cursor_skips_blanks: bool) {
    let head = concat!(
        "docs/\u{670d}\u{52a1}\u{5668}\u{8fd0}\u{7ef4}\u{4e0e}AI",
        "\u{5de5}\u{5177}\u{5165}\u{95e8}\u{8bb2}"
    );
    let tail = "\u{4e49}.md:1";
    let expected = format!("{head}\u{4e49}.md");
    let text = if cursor_skips_blanks {
        format!("\x1b[?1006h\x1b[3G\u{2022}\x1b[5G{head}\r\n\x1b[5G{tail}\u{ff1a}content")
    } else {
        format!("\x1b[?1006h  \u{2022} {head}\r\n    {tail}\u{ff1a}content")
    };
    let (wakeups, _) = async_channel::unbounded();
    let (events, _) = async_channel::unbounded();
    let (reads, _) = async_broadcast::broadcast(16);
    let mut grid = BlockGrid::new(
        SizeInfo::new_without_font_metrics(8, 40),
        1000,
        ChannelEventListener::new(wakeups, events, reads),
        ObfuscateSecrets::No,
        PerformResetGridChecks::No,
    );
    grid.start();
    Processor::new().parse_bytes(&mut grid, text.as_bytes(), &mut std::io::sink());
    let end_column = 4 + tail.chars().map(|c| c.width().unwrap_or(1)).sum::<usize>() - 1;
    let range = Point::new(0, 4)..=Point::new(1, end_column);
    for (row, segment) in [head, tail].into_iter().enumerate() {
        let cells = segment
            .chars()
            .map(|c| c.width().unwrap_or(1))
            .sum::<usize>();
        for col in 4..4 + cells {
            let point = Point::new(row, col);
            let paths = grid
                .grid_handler()
                .possible_codex_file_paths_at_point(point);
            let candidate = paths.iter().find(|candidate| {
                candidate.path.path == expected
                    && candidate
                        .path
                        .line_and_column_num
                        .is_some_and(|location| location.line_num == 1)
            });
            assert!(
                candidate.is_some(),
                "missing complete citation at {point:?}; skipped blanks: {cursor_skips_blanks}"
            );
            assert_eq!(
                candidate.unwrap().range,
                range,
                "incorrect range at {point:?}"
            );
        }
    }
}

#[test]
fn codex_wrapped_citation_with_explicit_spaces_retains_every_cell() {
    check_reference(false);
}

#[test]
fn codex_wrapped_citation_with_cursor_skipped_blanks_retains_every_cell() {
    check_reference(true);
}

fn check_soft_wrapped_reference(path: &str, columns: usize) {
    let (wakeups, _) = async_channel::unbounded();
    let (events, _) = async_channel::unbounded();
    let (reads, _) = async_broadcast::broadcast(16);
    let mut grid = BlockGrid::new(
        SizeInfo::new_without_font_metrics(16, columns),
        1000,
        ChannelEventListener::new(wakeups, events, reads),
        ObfuscateSecrets::No,
        PerformResetGridChecks::No,
    );
    grid.start();
    Processor::new().parse_bytes(
        &mut grid,
        format!("\x1b[?1006h{path}").as_bytes(),
        &mut std::io::sink(),
    );
    let (mut row, mut col) = (0, 0);
    let mut points = Vec::new();
    for c in path.chars() {
        let width = c.width().unwrap_or(1);
        if col + width > columns {
            row += 1;
            col = 0;
        }
        points.extend((col..col + width).map(|col| Point::new(row, col)));
        col += width;
    }
    let expected_range = *points.first().unwrap()..=*points.last().unwrap();
    let expected_path = path.strip_suffix(":19").unwrap();
    for point in points {
        assert!(
            grid.grid_handler()
                .possible_codex_file_paths_at_point(point)
                .iter()
                .any(|candidate| candidate.path.path == expected_path
                    && candidate.range == expected_range
                    && candidate
                        .path
                        .line_and_column_num
                        .is_some_and(|location| location.line_num == 19)),
            "incomplete soft-wrapped reference at {point:?}, columns={columns}, path={path}"
        );
    }
}

#[test]
fn codex_soft_wrap_preserves_filename_spaces() {
    for columns in [8, 11, 16, 20, 28] {
        check_soft_wrapped_reference("docs/a file with spaces.md:19", columns);
    }
}

#[test]
fn codex_soft_wrap_preserves_cjk_at_column_boundary() {
    for columns in [12, 13, 14, 15, 16, 19, 20] {
        check_soft_wrapped_reference(
            "docs/\u{670d}\u{52a1}\u{5668}\u{8fd0}\u{7ef4}\u{4e0e}AI\u{5de5}\u{5177}\u{5165}\u{95e8}\u{8bb2}\u{4e49}.md:19",
            columns,
        );
    }
}
