import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { describe, it, expect } from "vitest";

import { AGENT_RPC_METHODS } from "../src/rpc-methods.js";

// Cross-stack golden: the checked-in list is the single source of truth for
// the omw-agent <-> omw-server control-contract method vocabulary. The Rust
// side (crates/omw-server) asserts the same file. See specs/agent-rpc-methods.txt.
const here = dirname(fileURLToPath(import.meta.url));
const LIST_PATH = join(here, "../../../specs/agent-rpc-methods.txt");

function parseList(text: string): string[] {
  return text
    .split("\n")
    .map((l) => l.trim())
    .filter((l) => l.length > 0 && !l.startsWith("#"));
}

describe("agent RPC method vocabulary", () => {
  const fileMethods = parseList(readFileSync(LIST_PATH, "utf8"));

  it("the source-of-truth list is sorted and unique", () => {
    expect(fileMethods).toEqual([...fileMethods].sort());
    expect(new Set(fileMethods).size).toBe(fileMethods.length);
  });

  it("every declared TS constant is present in the source-of-truth list", () => {
    for (const m of AGENT_RPC_METHODS) {
      expect(fileMethods).toContain(m);
    }
  });
});
