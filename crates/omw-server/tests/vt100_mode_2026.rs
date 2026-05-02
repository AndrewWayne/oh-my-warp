//! Diagnostic: does vt100 0.16 process CSI mode 2026 (synchronized output)?
//!
//! Claude Code uses `\x1b[?2026h … \x1b[?2026l` for atomic redraws. If vt100
//! doesn't recognize this mode, inner bytes are either swallowed or
//! processed in a way that diverges from xterm.js. Either case would produce
//! the duplicate-render symptom: the daemon's parser snapshot shows
//! incomplete state, the live phone xterm receives bytes that may be
//! interpreted differently than the laptop processed them.

use std::fmt::Write;

fn row_str(screen: &vt100::Screen, row: u16, width: u16) -> String {
    let mut s = String::with_capacity(width as usize);
    for c in 0..width {
        let cell = screen
            .cell(row, c)
            .map(|cell| cell.contents().to_string())
            .unwrap_or_default();
        if cell.is_empty() {
            s.push(' ');
        } else {
            s.push_str(&cell);
        }
    }
    s
}

fn dump_screen(label: &str, screen: &vt100::Screen, rows: u16, cols: u16) -> String {
    let mut out = String::new();
    writeln!(out, "=== {label} ===").unwrap();
    writeln!(out, "alt={} cursor={:?}", screen.alternate_screen(), screen.cursor_position()).unwrap();
    for r in 0..rows {
        let line = row_str(screen, r, cols);
        let trimmed = line.trim_end();
        if !trimmed.is_empty() {
            writeln!(out, "row {:2}: {:?}", r, trimmed).unwrap();
        }
    }
    out
}

#[test]
fn mode_2026_inner_bytes_reach_screen() {
    let mut p = vt100::Parser::new(24, 80, 0);
    p.process(b"BEFORE\r\n");

    // Mode 2026 BSU + write "INSIDE" + ESU. If vt100 supports 2026, "INSIDE"
    // should appear on row 1 (after the BEFORE\r\n). If it doesn't, "INSIDE"
    // could either appear normally (if BSU/ESU are no-ops) or be swallowed.
    p.process(b"\x1b[?2026h");
    p.process(b"INSIDE");
    p.process(b"\x1b[?2026l");

    eprintln!("{}", dump_screen("after mode-2026 frame", p.screen(), 5, 80));

    let row0 = row_str(p.screen(), 0, 80);
    let row1 = row_str(p.screen(), 1, 80);
    eprintln!("row0 trimmed: {:?}", row0.trim_end());
    eprintln!("row1 trimmed: {:?}", row1.trim_end());

    // We DON'T assert here; this is a diagnostic. Whatever the result, log it.
}

#[test]
fn mode_2026_with_cursor_positioning_inside() {
    let mut p = vt100::Parser::new(24, 80, 0);

    // Pre-populate row 5 column 0 with old content.
    p.process(b"\x1b[6;1HOLD-CONTENT-AT-ROW-5");

    // Mode 2026: move to row 5 col 1, write NEW. Then ESU.
    p.process(b"\x1b[?2026h");
    p.process(b"\x1b[6;1H");      // cursor to (row 6, col 1) which is row=5 in 0-index
    p.process(b"\x1b[2K");          // clear line
    p.process(b"NEW-CONTENT");
    p.process(b"\x1b[?2026l");

    eprintln!("{}", dump_screen("after cursor-positioned mode-2026 frame", p.screen(), 8, 80));
    let row5 = row_str(p.screen(), 5, 80);
    eprintln!("row5 trimmed: {:?}", row5.trim_end());

    // Diagnostic only.
}

#[test]
fn multiple_mode_2026_frames_accumulate_or_replace() {
    let mut p = vt100::Parser::new(24, 80, 0);

    // Frame A: cursor to row 10, write "AAA"
    p.process(b"\x1b[?2026h\x1b[11;1H\x1b[2KAAA\x1b[?2026l");

    // Frame B: cursor to row 10, write "BBB" (should REPLACE AAA)
    p.process(b"\x1b[?2026h\x1b[11;1H\x1b[2KBBB\x1b[?2026l");

    // Frame C: cursor to row 10, write "CCC" (should REPLACE BBB)
    p.process(b"\x1b[?2026h\x1b[11;1H\x1b[2KCCC\x1b[?2026l");

    eprintln!("{}", dump_screen("after 3 mode-2026 frames replacing row 10", p.screen(), 15, 80));

    let row10 = row_str(p.screen(), 10, 80);
    eprintln!("row10 trimmed: {:?}", row10.trim_end());
    // Diagnostic only — we want to know if vt100 replaces correctly or
    // accumulates content.
}
