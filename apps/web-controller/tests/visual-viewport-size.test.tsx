import { afterEach, describe, expect, it, vi } from "vitest";
import { act, renderHook } from "@testing-library/react";
import { useVisualViewportSize } from "../src/hooks/useVisualViewportSize";

type ViewportListener = (event?: Event) => void;

interface FakeVisualViewport {
  height: number;
  width: number;
  offsetLeft: number;
  offsetTop: number;
  addEventListener: (type: string, l: ViewportListener) => void;
  removeEventListener: (type: string, l: ViewportListener) => void;
  dispatchEvent: (type: string) => void;
}

function fakeViewport(initial: {
  height: number;
  width?: number;
  offsetLeft?: number;
  offsetTop: number;
}): FakeVisualViewport {
  const listeners = new Map<string, Set<ViewportListener>>();
  return {
    height: initial.height,
    width: initial.width ?? 390,
    offsetLeft: initial.offsetLeft ?? 0,
    offsetTop: initial.offsetTop,
    addEventListener(type, l) {
      let set = listeners.get(type);
      if (!set) {
        set = new Set();
        listeners.set(type, set);
      }
      set.add(l);
    },
    removeEventListener(type, l) {
      listeners.get(type)?.delete(l);
    },
    dispatchEvent(type) {
      listeners.get(type)?.forEach((l) => l());
    },
  };
}

const ORIGINAL_VV = (window as unknown as { visualViewport?: unknown })
  .visualViewport;

afterEach(() => {
  Object.defineProperty(window, "visualViewport", {
    configurable: true,
    value: ORIGINAL_VV,
  });
});

describe("useVisualViewportSize", () => {
  it("reads height + offsetTop from window.visualViewport when present", () => {
    const vv = fakeViewport({ height: 600, offsetTop: 0 });
    Object.defineProperty(window, "visualViewport", {
      configurable: true,
      value: vv,
    });

    const { result } = renderHook(() => useVisualViewportSize());
    expect(result.current.height).toBe(600);
    expect(result.current.width).toBe(390);
    expect(result.current.offsetLeft).toBe(0);
    expect(result.current.offsetTop).toBe(0);
  });

  it("falls back to window.innerHeight + 0 when visualViewport is missing", () => {
    Object.defineProperty(window, "visualViewport", {
      configurable: true,
      value: undefined,
    });
    const original = window.innerHeight;
    const originalWidth = window.innerWidth;
    Object.defineProperty(window, "innerHeight", {
      configurable: true,
      value: 812,
    });
    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value: 390,
    });

    try {
      const { result } = renderHook(() => useVisualViewportSize());
      expect(result.current.height).toBe(812);
      expect(result.current.width).toBe(390);
      expect(result.current.offsetLeft).toBe(0);
      expect(result.current.offsetTop).toBe(0);
    } finally {
      Object.defineProperty(window, "innerHeight", {
        configurable: true,
        value: original,
      });
      Object.defineProperty(window, "innerWidth", {
        configurable: true,
        value: originalWidth,
      });
    }
  });

  it("updates on resize and scroll events", () => {
    const vv = fakeViewport({ height: 600, offsetTop: 0 });
    Object.defineProperty(window, "visualViewport", {
      configurable: true,
      value: vv,
    });

    const { result } = renderHook(() => useVisualViewportSize());
    expect(result.current.height).toBe(600);

    act(() => {
      vv.height = 320;
      vv.width = 360;
      vv.offsetLeft = 12;
      vv.offsetTop = 40;
      vv.dispatchEvent("resize");
    });
    expect(result.current.height).toBe(320);
    expect(result.current.width).toBe(360);
    expect(result.current.offsetLeft).toBe(12);
    expect(result.current.offsetTop).toBe(40);

    act(() => {
      vv.offsetTop = 80;
      vv.dispatchEvent("scroll");
    });
    expect(result.current.offsetTop).toBe(80);
  });

  it("removes listeners on unmount", () => {
    const vv = fakeViewport({ height: 500, offsetTop: 0 });
    const removeSpy = vi.spyOn(vv, "removeEventListener");
    Object.defineProperty(window, "visualViewport", {
      configurable: true,
      value: vv,
    });

    const { unmount } = renderHook(() => useVisualViewportSize());
    unmount();

    const types = removeSpy.mock.calls.map((c) => c[0]);
    expect(types).toContain("resize");
    expect(types).toContain("scroll");
  });
});
