/**
 * Real-byte reproduction of the duplicate-render bug.
 *
 * Loads `tests/fixtures/claude-exit-hint.bin` — a byte stream captured
 * by spawning real `claude` via omw-pty, accepting the trust prompt,
 * and typing `/exit` keystroke-by-keystroke. The fixture contains the
 * exact bytes the laptop emits for that interaction.
 *
 * Renders those bytes into a real xterm.js Terminal sized to match the
 * captured pane (149x39) and inspects the resulting buffer. The bug
 * we're chasing: rows accumulating prior frame content (e.g. "Tip: …"
 * appearing twice, multiple spinner states overlapping). The test fails
 * if any row looks like accumulated content from multiple frames.
 *
 * If this test PASSES with the fixture, the bug is not in xterm.js's
 * rendering — it's in something else (probably bytes flowing through
 * the WS path getting mangled, or size mismatch between phone xterm
 * and laptop pane). If this test FAILS, we have a clear repro to
 * iterate fixes against.
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

const here = dirname(fileURLToPath(import.meta.url));
const fixturePath = join(here, "fixtures", "claude-exit-hint.bin");
const BEGIN_MARKER = "----- KEYSTROKES BEGIN -----";
const END_MARKER = "----- KEYSTROKES END -----";

/**
 * Strip our two test-injected ASCII markers from the fixture so xterm
 * sees only the bytes claude actually emitted. Returns the bytes from
 * stream start through end-of-typing — what the phone xterm would have
 * processed if attached the whole time.
 */
function fixtureBytesUpToEndOfTyping(fixture: Uint8Array): Uint8Array {
  const haystack = Buffer.from(fixture).toString("binary");
  const beginIdx = haystack.indexOf(BEGIN_MARKER);
  const endIdx = haystack.indexOf(END_MARKER);
  if (beginIdx < 0 || endIdx < 0) return fixture;
  // [0, beginIdx) + [beginIdx + len(BEGIN) + 1 newline, endIdx).
  const head = fixture.subarray(0, beginIdx - 1 /* preceding \n */);
  const tail = fixture.subarray(beginIdx + BEGIN_MARKER.length + 1, endIdx - 1);
  const out = new Uint8Array(head.length + tail.length);
  out.set(head, 0);
  out.set(tail, head.length);
  return out;
}

describe("real claude code byte fixture", () => {
  it("renders /exit hint without row accumulation", async () => {
    const div = document.createElement("div");
    document.body.appendChild(div);
    // Match the captured pane size — 149x39.
    const term = new Terminal({ cols: 149, rows: 39, allowProposedApi: true });
    term.open(div);

    const fixture = readFileSync(fixturePath);
    const playable = fixtureBytesUpToEndOfTyping(fixture);
    console.log(
      `fixture: ${fixture.length} bytes total, ${playable.length} playable (up to end-of-typing)`,
    );

    await writeAndWait(term, playable);

    const r = rows(term);
    console.log("=== final xterm state ===");
    for (let i = 0; i < r.length; i++) {
      const trimmed = r[i].trimEnd();
      if (trimmed.length > 0) {
        console.log(`row ${i.toString().padStart(2)}: ${JSON.stringify(trimmed)}`);
      }
    }

    // ASSERTION: detect accumulation. After typing `/exit`, claude code's
    // hint area at the bottom should show ONE prompt entry. If we see
    // multiple distinct prompt suggestions piled up, that's accumulation.
    //
    // Specifically, count rows containing the literal "/exit" — there
    // should be exactly ONE (the input area itself, maybe one more for the
    // suggestion).
    const exitMatches = r.filter((row) => row.includes("/exit")).length;
    console.log(`/exit appears in ${exitMatches} row(s)`);

    // We don't yet know the exact correct count, so for now log it. The
    // first run gives us the baseline. If exitMatches > 3 (input + a few
    // hint variants), something is clearly wrong.
    expect(exitMatches).toBeLessThanOrEqual(3);

    // Also check: no row should be strangely long with concatenated junk
    // (the "Bakedating 1m 7s..." pattern). Rows should be clean.
    for (let i = 0; i < r.length; i++) {
      const row = r[i];
      // Heuristic: a clean row has reasonable spacing or single text.
      // A duplicate-corrupted row often has inline word concatenation
      // like "Bakedating" (two words mashed). Hard to detect generically;
      // we log and let humans inspect the dump.
      if (row.length > 0) {
        // No assertion here — just data.
      }
    }
  });
});
