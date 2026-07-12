import { describe, it, expect } from "vitest";

import { parseBashFrame } from "../src/warp-session-bash.js";

describe("parseBashFrame", () => {
  it("parses a bash/data frame with bytes", () => {
    expect(parseBashFrame("bash/data", { commandId: "c1", bytes: "hi" })).toEqual({
      method: "bash/data",
      params: { commandId: "c1", bytes: "hi", exitCode: null, snapshot: false },
    });
  });

  it("parses a bash/finished frame with exitCode + snapshot", () => {
    expect(
      parseBashFrame("bash/finished", { commandId: "c1", exitCode: 0, snapshot: true }),
    ).toEqual({
      method: "bash/finished",
      params: { commandId: "c1", bytes: undefined, exitCode: 0, snapshot: true },
    });
  });

  it("parses a bash/cancel frame", () => {
    const f = parseBashFrame("bash/cancel", { commandId: "c1" });
    expect(f?.method).toBe("bash/cancel");
    expect(f?.params.commandId).toBe("c1");
  });

  it("applies lenient defaults for missing/mistyped fields", () => {
    // Non-string bytes -> undefined; non-numeric exitCode -> null; snapshot
    // truthy only when strictly true.
    expect(
      parseBashFrame("bash/data", { commandId: "c1", bytes: 42 })?.params.bytes,
    ).toBeUndefined();
    expect(
      parseBashFrame("bash/finished", { commandId: "c1", exitCode: "x", snapshot: 1 })
        ?.params,
    ).toEqual({ commandId: "c1", bytes: undefined, exitCode: null, snapshot: false });
  });

  it("rejects non-bash methods and malformed params", () => {
    expect(parseBashFrame("session/create", { commandId: "c1" })).toBeNull();
    expect(parseBashFrame("bash/data", { bytes: "hi" })).toBeNull(); // no commandId
    expect(parseBashFrame("bash/data", null)).toBeNull();
    expect(parseBashFrame("bash/data", "nope")).toBeNull();
    expect(parseBashFrame("bash/data", { commandId: 5 })).toBeNull(); // commandId not a string
  });
});
