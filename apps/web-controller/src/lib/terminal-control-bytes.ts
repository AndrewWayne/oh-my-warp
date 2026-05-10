export type TerminalControlKey =
  | "shift-tab"
  | "esc"
  | "tab"
  | "ctrl-c"
  | "arrow-up"
  | "arrow-down"
  | "enter"
  | "ctrl-d"
  | "ctrl-l"
  | "slash"
  | "pipe"
  | "question"
  | "arrow-left"
  | "arrow-right";

const enc = new TextEncoder();

export function terminalControlBytes(key: TerminalControlKey): Uint8Array {
  switch (key) {
    case "shift-tab":
      return new Uint8Array([0x1b, 0x5b, 0x5a]);
    case "esc":
      return new Uint8Array([0x1b]);
    case "tab":
      return new Uint8Array([0x09]);
    case "ctrl-c":
      return new Uint8Array([0x03]);
    case "arrow-up":
      return new Uint8Array([0x1b, 0x5b, 0x41]);
    case "arrow-down":
      return new Uint8Array([0x1b, 0x5b, 0x42]);
    case "enter":
      return new Uint8Array([0x0d]);
    case "ctrl-d":
      return new Uint8Array([0x04]);
    case "ctrl-l":
      return new Uint8Array([0x0c]);
    case "slash":
      return enc.encode("/");
    case "pipe":
      return enc.encode("|");
    case "question":
      return enc.encode("?");
    case "arrow-left":
      return new Uint8Array([0x1b, 0x5b, 0x44]);
    case "arrow-right":
      return new Uint8Array([0x1b, 0x5b, 0x43]);
  }
}
