import { describe, expect, it } from "vitest";
import { computeKeyboardDockEdge } from "../src/lib/keyboard-dock";

describe("computeKeyboardDockEdge", () => {
  it("uses the visual viewport bottom for normal keyboard geometry", () => {
    expect(
      computeKeyboardDockEdge({
        layoutViewportHeight: 844,
        visualViewportHeight: 520,
        visualViewportOffsetTop: 0,
        previousDockEdge: 0,
      }),
    ).toBe(520);
  });

  it("keeps the previous sane dock edge when Safari reports a transient tiny viewport", () => {
    expect(
      computeKeyboardDockEdge({
        layoutViewportHeight: 844,
        visualViewportHeight: 150,
        visualViewportOffsetTop: 0,
        previousDockEdge: 520,
      }),
    ).toBe(520);
  });

  it("falls back to a lower-screen edge when the first keyboard viewport is implausibly tiny", () => {
    expect(
      computeKeyboardDockEdge({
        layoutViewportHeight: 844,
        visualViewportHeight: 150,
        visualViewportOffsetTop: 0,
        previousDockEdge: 0,
      }),
    ).toBeCloseTo(379.8);
  });
});
