/**
 * End-to-end check: feeding mode-2026 sync-output frames into a real
 * xterm.js Terminal must NOT accumulate prior frame content.
 *
 * Reproduces the user-reported duplicate-render: typing `/`, `e`, `x`, `i`,
 * `t` in Claude Code shows hint text piling up on the phone, instead of
 * each new hint replacing the prior one in place.
 *
 * This is the only environment where the bug actually manifests — vt100
 * (in our Rust tests) parses sync-output correctly and shows a clean
 * single-frame replay. xterm.js renders bytes serially into a DOM buffer,
 * so its handling of `\x1b[?2026h` (Begin Synchronized Update) and
 * `\x1b[?2026l` (End Synchronized Update) is what we have to verify.
 */
import { Terminal } from "@xterm/xterm";
import { describe, expect, it } from "vitest";

function rows(term: Terminal): string[] {
  const out: string[] = [];
  const buf = term.buffer.active;
  for (let i = 0; i < term.rows; i++) {
    const line = buf.getLine(i);
    out.push(line ? line.translateToString(true) : "");
  }
  return out;
}

/**
 * xterm.js's `write` is asynchronous in 5.x — it returns void but processing
 * happens off the call stack. Wrap it in a Promise so tests await each
 * frame and we never inspect the buffer mid-parse.
 */
function writeAndWait(term: Terminal, data: string): Promise<void> {
  return new Promise((resolve) => term.write(data, resolve));
}

/** Print a buffer dump for diagnostic reads. */
function dump(label: string, term: Terminal): string {
  const r = rows(term);
  return [`=== ${label} ===`, ...r.map((row, i) => `row ${i.toString().padStart(2)}: ${JSON.stringify(row)}`)].join("\n");
}

describe("xterm.js mode-2026 sync output", () => {
  it("frames at the same row REPLACE in place, not accumulate", async () => {
    const div = document.createElement("div");
    document.body.appendChild(div);
    const term = new Terminal({ cols: 80, rows: 24, allowProposedApi: true });
    term.open(div);

    // Initial paint: clear, write header on rows 0-1.
    await writeAndWait(term, "\x1b[2J\x1b[H");
    await writeAndWait(term, "\x1b[1;1HClaude Code v2.1.126\r\n");
    await writeAndWait(term, "\x1b[2;1HOpus 4.7\r\n");

    // 5 mode-2026 frames each redrawing row 23 (bottom) with current state.
    // Each frame: BSU + cursor to row 24 col 1 + clear-line + text + ESU.
    for (const state of ["Levitating 1s", "Levitating 2s", "Levitating 3s", "Levitating 4s", "Baked"]) {
      await writeAndWait(term, `\x1b[?2026h\x1b[24;1H\x1b[2K${state}\x1b[?2026l`);
    }

    const r = rows(term);
    console.log(dump("after 5 mode-2026 frames", term));

    // Bottom row must be ONLY "Baked" — the latest state. If we see
    // "Levitating ..." remnants on row 23 or content on rows 22/21 etc,
    // accumulation is happening.
    expect(r[23].trim()).toBe("Baked");

    // Rows 0-1 are the unchanged header.
    expect(r[0].trim()).toBe("Claude Code v2.1.126");
    expect(r[1].trim()).toBe("Opus 4.7");

    // Rows 2-22 must be empty — the spinner frames only touched row 23.
    for (let i = 2; i < 23; i++) {
      expect(r[i].trim(), `row ${i} should be empty after spinner-only updates`).toBe("");
    }
  });

  it("frame split across chunks (BSU in one write, ESU in next)", async () => {
    const div = document.createElement("div");
    document.body.appendChild(div);
    const term = new Terminal({ cols: 80, rows: 24, allowProposedApi: true });
    term.open(div);

    await writeAndWait(term, "\x1b[2J\x1b[H");

    // 3 frames each split into 3 writes: BSU, payload, ESU.
    // Same row, different state — should still REPLACE in place.
    for (const state of ["AAAAAAA", "BBBB", "C"]) {
      await writeAndWait(term, "\x1b[?2026h");
      await writeAndWait(term, `\x1b[24;1H\x1b[2K${state}`);
      await writeAndWait(term, "\x1b[?2026l");
    }
    const r = rows(term);
    console.log(dump("after split frames", term));
    expect(r[23].trim()).toBe("C");
  });

  /**
   * Diagnostic (NOT a regression test): demonstrates that mode-2026 frames
   * which DON'T explicitly clear before writing leave prior content tails
   * in xterm.js. Whether this is the real bug pattern depends on what
   * Claude Code actually emits — capture via `OMW_BYTE_DUMP=path` env var
   * on warp-oss, then enable this test against captured bytes.
   */
  it.skip("frame WITHOUT line-clear leaves prior tail (xterm.js standard behaviour)", async () => {
    const div = document.createElement("div");
    document.body.appendChild(div);
    const term = new Terminal({ cols: 80, rows: 24, allowProposedApi: true });
    term.open(div);

    await writeAndWait(term, "\x1b[2J\x1b[H");
    await writeAndWait(
      term,
      "\x1b[?2026h\x1b[24;1HLevitating 1m 7s (running stop hook)\x1b[?2026l",
    );
    await writeAndWait(term, "\x1b[?2026h\x1b[24;1HBaked\x1b[?2026l");

    const r = rows(term);
    console.log(dump("after no-clear short overwrite", term));
    expect(r[23].trim()).toBe("Bakedating 1m 7s (running stop hook)");
  });

  it("simulates `/exit` keystroke-by-keystroke hint update", async () => {
    const div = document.createElement("div");
    document.body.appendChild(div);
    const term = new Terminal({ cols: 80, rows: 24, allowProposedApi: true });
    term.open(div);

    // Initial paint: prompt at bottom.
    await writeAndWait(term, "\x1b[2J\x1b[H\x1b[24;1H>");

    // Simulating Claude Code's hint area at rows 20-22 with each keystroke.
    // Each frame redraws the hint area atomically inside mode 2026.
    // After `/`, Claude shows multiple hints; subsequent letters narrow them.
    const frames = [
      // After `/`
      "\x1b[?2026h\x1b[20;1H\x1b[2K  /clear\x1b[21;1H\x1b[2K  /exit\x1b[22;1H\x1b[2K  /reset\x1b[24;1H>/\x1b[?2026l",
      // After `/e`
      "\x1b[?2026h\x1b[20;1H\x1b[2K  /exit\x1b[21;1H\x1b[2K\x1b[22;1H\x1b[2K\x1b[24;1H>/e\x1b[?2026l",
      // After `/ex`
      "\x1b[?2026h\x1b[20;1H\x1b[2K  /exit\x1b[21;1H\x1b[2K\x1b[22;1H\x1b[2K\x1b[24;1H>/ex\x1b[?2026l",
      // After `/exi`
      "\x1b[?2026h\x1b[20;1H\x1b[2K  /exit\x1b[21;1H\x1b[2K\x1b[22;1H\x1b[2K\x1b[24;1H>/exi\x1b[?2026l",
      // After `/exit`
      "\x1b[?2026h\x1b[20;1H\x1b[2K  /exit\x1b[21;1H\x1b[2K\x1b[22;1H\x1b[2K\x1b[24;1H>/exit\x1b[?2026l",
    ];
    for (const frame of frames) {
      await writeAndWait(term, frame);
    }

    const r = rows(term);
    console.log(dump("after /exit keystroke sequence", term));

    // After typing `/exit`, only `/exit` hint should remain on row 19 (0-indexed).
    expect(r[19].trim()).toBe("/exit");
    // Rows 20-22 should be empty (cleared by each frame's [2K).
    expect(r[20].trim(), "row 20 should be empty (cleared)").toBe("");
    expect(r[21].trim(), "row 21 should be empty (cleared)").toBe("");
    // Bottom row shows the input.
    expect(r[23].trim()).toBe(">/exit");
  });
});
