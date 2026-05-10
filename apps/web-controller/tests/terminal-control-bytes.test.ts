import { describe, expect, it } from "vitest";
import { terminalControlBytes } from "../src/lib/terminal-control-bytes";

function bytesFor(id: Parameters<typeof terminalControlBytes>[0]): number[] {
  return Array.from(terminalControlBytes(id));
}

describe("terminalControlBytes", () => {
  it.each([
    ["shift-tab", [0x1b, 0x5b, 0x5a]],
    ["esc", [0x1b]],
    ["tab", [0x09]],
    ["ctrl-c", [0x03]],
    ["arrow-up", [0x1b, 0x5b, 0x41]],
    ["arrow-down", [0x1b, 0x5b, 0x42]],
    ["enter", [0x0d]],
    ["ctrl-d", [0x04]],
    ["ctrl-l", [0x0c]],
    ["slash", [0x2f]],
    ["pipe", [0x7c]],
    ["question", [0x3f]],
    ["arrow-left", [0x1b, 0x5b, 0x44]],
    ["arrow-right", [0x1b, 0x5b, 0x43]],
  ] as const)("maps %s to exact terminal bytes", (id, expected) => {
    expect(bytesFor(id)).toEqual(expected);
  });
});
