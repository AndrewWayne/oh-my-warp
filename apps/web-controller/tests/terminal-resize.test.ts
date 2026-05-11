import { describe, expect, it } from "vitest";
import {
  MIN_REMOTE_TERMINAL_COLS,
  MIN_REMOTE_TERMINAL_ROWS,
  shouldSendTerminalResize,
} from "../src/lib/terminal-resize";

describe("terminal resize guard", () => {
  it("drops transient iOS keyboard sizes before they can shrink the remote pty", () => {
    const last = { rows: 25, cols: 42 };

    expect(
      shouldSendTerminalResize(
        { rows: MIN_REMOTE_TERMINAL_ROWS - 1, cols: 42 },
        last,
      ),
    ).toBe(false);
    expect(
      shouldSendTerminalResize(
        { rows: 12, cols: MIN_REMOTE_TERMINAL_COLS - 1 },
        last,
      ),
    ).toBe(false);
  });

  it("sends useful changed sizes and dedupes unchanged ones", () => {
    const last = { rows: 21, cols: 42 };

    expect(shouldSendTerminalResize({ rows: 21, cols: 42 }, last)).toBe(false);
    expect(shouldSendTerminalResize({ rows: 25, cols: 42 }, last)).toBe(true);
    expect(shouldSendTerminalResize({ rows: 21, cols: 80 }, last)).toBe(true);
  });
});
