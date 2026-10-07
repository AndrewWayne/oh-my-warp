// SPDX-License-Identifier: AGPL-3.0-only
//
// omw-authored tests in the in-tree upstream fork, part of the AGPL-3.0
// derivative work. See specs/fork-strategy.md section 3.
// Copyright (C) 2026 omw contributors
// Copyright (C) 2020-2026 Denver Technologies, Inc.

#![cfg(all(feature = "test-exports", feature = "local_fs"))]

use std::path::PathBuf;
use warp::test_exports::{osc8_target, CodeSource, EditorLayout, FileTarget, Osc8Target};
use warp_util::path::LineAndColumnArg;

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
