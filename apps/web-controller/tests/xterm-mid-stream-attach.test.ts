/**
 * Reproduce the user's actual share-attach scenario:
 *
 *   1. Claude Code is already running on the laptop (early bytes have flowed
 *      and shaped the screen).
 *   2. The phone attaches mid-stream — the daemon ships a parser-derived
 *      "snapshot" frame, then forwards live updates from that point onward.
 *   3. The phone's xterm should end up in the same state as one that had
 *      seen the full byte stream.
 *
 * The bug we're hunting: the snapshot doesn't fully restore state, OR live
 * incremental deltas (Claude's mode-2026 + ECH-based partial repaints)
 * apply incorrectly to the post-snapshot canvas.
 *
 * Strategy:
 *   - Feed the FULL captured byte stream into a "ground-truth" xterm.
 *     Capture its final buffer.
 *   - Pick a CUT POINT in the stream (right before the user starts typing,
 *     i.e. the BEGIN_MARKER position). Feed [0, cut) into a "parser" xterm
 *     and serialize ITS buffer to ANSI bytes (proxy for what the daemon
 *     parser snapshot would produce).
 *   - Feed [cut, end) — the post-attach live bytes — into a third "phone"
 *     xterm AFTER first writing the snapshot bytes.
 *   - Compare phone xterm's buffer to ground truth.
 *
 * If they match → snapshot+deltas is sufficient. If they diverge → we have
 * a precise diff showing exactly which row the bug is on.
 */
import { Terminal } from "@xterm/xterm";
import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

function rows(term: Terminal): string[] {
  const out: string[] = [];
  const buf = term.buffer.active;
  for (let i = 0; i < term.rows; i++) {
    const line = buf.getLine(i);
    out.push(line ? line.translateToString(true) : "");
  }
  return out;
}

function writeAndWait(term: Terminal, data: Uint8Array): Promise<void> {
  return new Promise((resolve) => term.write(data, resolve));
}

function newTerm(): Terminal {
  const div = document.createElement("div");
  document.body.appendChild(div);
  const t = new Terminal({ cols: 149, rows: 39, allowProposedApi: true });
  t.open(div);
  return t;
}

const here = dirname(fileURLToPath(import.meta.url));
const fixturePath = join(here, "fixtures", "claude-exit-hint.bin");

const BEGIN_MARKER = "----- KEYSTROKES BEGIN -----";
const END_MARKER = "----- KEYSTROKES END -----";

function findMarker(buf: Uint8Array, marker: string): number {
  // Linear search the marker as bytes.
  const m = Buffer.from(marker, "binary");
  for (let i = 0; i + m.length <= buf.length; i++) {
    let ok = true;
    for (let j = 0; j < m.length; j++) {
      if (buf[i + j] !== m[j]) {
        ok = false;
        break;
      }
    }
    if (ok) return i;
  }
  return -1;
}

interface SplitFixture {
  preTyping: Uint8Array;     // what the laptop emitted BEFORE user typed
  liveTyping: Uint8Array;    // bytes emitted DURING user's /exit typing
  fullStream: Uint8Array;    // pre + live concatenated (no markers)
}

function splitFixture(): SplitFixture {
  const fixture = readFileSync(fixturePath);
  const beginIdx = findMarker(fixture, BEGIN_MARKER);
  const endIdx = findMarker(fixture, END_MARKER);
  if (beginIdx < 0 || endIdx < 0) {
    throw new Error("fixture markers not found");
  }
  // pre = [0, beginIdx-1)  (drop the leading \n we inserted).
  // live = [beginIdx + len(BEGIN) + 1, endIdx - 1)
  const pre = fixture.subarray(0, beginIdx - 1);
  const live = fixture.subarray(beginIdx + BEGIN_MARKER.length + 1, endIdx - 1);
  const full = new Uint8Array(pre.length + live.length);
  full.set(pre, 0);
  full.set(live, pre.length);
  return { preTyping: pre, liveTyping: live, fullStream: full };
}

describe("mid-stream attach (share scenario)", () => {
  it("ground-truth xterm fed full stream renders cleanly", async () => {
    const { fullStream } = splitFixture();
    const t = newTerm();
    await writeAndWait(t, fullStream);

    const r = rows(t);
    const exitMatches = r.filter((row) => row.includes("/exit")).length;
    console.log(`ground-truth: /exit appears in ${exitMatches} rows`);
    console.log("ground-truth row 17:", JSON.stringify(r[17].trimEnd()));
    expect(exitMatches).toBeGreaterThan(0);
    expect(r[17].includes("Exit the CLI")).toBe(true);
  });

  it("phone joining mid-stream WITHOUT a snapshot misses initial state", async () => {
    // This models the failure mode the user originally saw: phone connected,
    // got only post-attach live bytes, never saw the welcome banner / TUI
    // baseline. Live deltas alone shouldn't reconstruct the full screen.
    const { liveTyping } = splitFixture();
    const t = newTerm();
    await writeAndWait(t, liveTyping);

    const r = rows(t);
    console.log("=== phone-without-snapshot rendered state ===");
    for (let i = 0; i < r.length; i++) {
      const trimmed = r[i].trimEnd();
      if (trimmed.length > 0) {
        console.log(`row ${i.toString().padStart(2)}: ${JSON.stringify(trimmed)}`);
      }
    }
    // The welcome banner + intro tips DEFINITELY shouldn't be on screen if
    // phone only got live deltas.
    const welcomeShown = r.some((row) => row.includes("Welcome back"));
    console.log(`phone-no-snapshot: welcome banner visible = ${welcomeShown}`);
    expect(welcomeShown).toBe(false);
  });

  it("FIX VERIFICATION: phone xterm RESIZED to laptop size renders cleanly", async () => {
    // After the proposed fix: phone receives an initial size message from
    // the daemon (laptop pane's size — 149x39 in this fixture) and calls
    // xterm.resize(149, 39) BEFORE processing the snapshot or live bytes.
    // With matching size, cursor positioning lands correctly. Should match
    // the ground-truth render exactly.
    const { fullStream } = splitFixture();

    const div = document.createElement("div");
    document.body.appendChild(div);
    // xterm starts at the default 80x24 (what the React component does today).
    const phone = new Terminal({ cols: 80, rows: 24, allowProposedApi: true });
    phone.open(div);
    // FIX: resize to laptop's size BEFORE processing any bytes.
    phone.resize(149, 39);
    await writeAndWait(phone, fullStream);

    const r = rows(phone);
    const exitMatches = r.filter((row) => row.includes("/exit")).length;
    console.log(`fix-verified: /exit appears in ${exitMatches} rows`);
    console.log(`fix-verified row 17: ${JSON.stringify(r[17].trimEnd())}`);
    expect(exitMatches).toBeGreaterThan(0);
    expect(r[17].includes("Exit the CLI")).toBe(true);
    // Critical: row 17 should NOT contain mashed-together fragments.
    expect(r[17].trimEnd()).toBe("/exit                               Exit the CLI");
  });

  it("phone xterm SMALLER than laptop pane causes accumulation at boundary", async () => {
    // Phone screen is small. xterm.js was created at default size (80x24)
    // and never told that the laptop pane is actually 149x39. Live bytes
    // arrive with absolute cursor positioning targeting rows up to 39 and
    // cols up to 149 — those clamp to the phone's grid boundary, piling
    // multiple writes at the same boundary cell.
    const { fullStream } = splitFixture();

    // Match the conditions: phone xterm at default size, NOT 149x39.
    const div = document.createElement("div");
    document.body.appendChild(div);
    const phone = new Terminal({ cols: 80, rows: 24, allowProposedApi: true });
    phone.open(div);
    await writeAndWait(phone, fullStream);

    const r = rows(phone);
    console.log("=== phone (80x24) rendered ===");
    for (let i = 0; i < r.length; i++) {
      const trimmed = r[i].trimEnd();
      if (trimmed.length > 0) {
        console.log(`row ${i.toString().padStart(2)}: ${JSON.stringify(trimmed)}`);
      }
    }

    // If accumulation is happening, the bottom row (23) will have content
    // smashed from many writes that intended to go to rows 25-39.
    const bottom = r[23].trimEnd();
    console.log(`bottom row (raw): ${JSON.stringify(r[23])}`);
    console.log(`bottom row trimmed length: ${bottom.length}`);
  });

  it("ground-truth vs full-then-replay-from-cut: replay matches", async () => {
    // Sanity: feeding [pre + live] should equal feeding pre then live.
    const { preTyping, liveTyping, fullStream } = splitFixture();

    const truth = newTerm();
    await writeAndWait(truth, fullStream);

    const replay = newTerm();
    await writeAndWait(replay, preTyping);
    await writeAndWait(replay, liveTyping);

    const truthR = rows(truth);
    const replayR = rows(replay);
    let mismatches = 0;
    for (let i = 0; i < truthR.length; i++) {
      if (truthR[i].trimEnd() !== replayR[i].trimEnd()) {
        mismatches++;
        console.log(`row ${i} differs:\n  truth:  ${JSON.stringify(truthR[i].trimEnd())}\n  replay: ${JSON.stringify(replayR[i].trimEnd())}`);
      }
    }
    expect(mismatches).toBe(0);
  });
});
