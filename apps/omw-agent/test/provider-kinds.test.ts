import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { describe, it, expect } from "vitest";

import { PROVIDER_KINDS, isProviderKind } from "../src/provider-kinds.js";

// Cross-stack golden: the checked-in list is the single source of truth for
// the provider-kind vocabulary; the Rust side (omw-config) asserts the same
// file. See specs/provider-kinds.txt.
const here = dirname(fileURLToPath(import.meta.url));
const LIST_PATH = join(here, "../../../specs/provider-kinds.txt");

function parseList(text: string): string[] {
  return text
    .split("\n")
    .map((l) => l.trim())
    .filter((l) => l.length > 0 && !l.startsWith("#"));
}

describe("provider-kind vocabulary", () => {
  const fileKinds = parseList(readFileSync(LIST_PATH, "utf8"));

  it("PROVIDER_KINDS matches the sorted, unique source-of-truth list", () => {
    expect(fileKinds).toEqual([...fileKinds].sort());
    expect(new Set(fileKinds).size).toBe(fileKinds.length);
    expect([...PROVIDER_KINDS]).toEqual(fileKinds);
  });

  it("isProviderKind accepts every listed kind and rejects others", () => {
    for (const k of fileKinds) expect(isProviderKind(k)).toBe(true);
    expect(isProviderKind("gemini")).toBe(false);
    expect(isProviderKind(42)).toBe(false);
    expect(isProviderKind(undefined)).toBe(false);
  });
});
