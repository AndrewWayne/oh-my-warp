import "@testing-library/jest-dom/vitest";
import "fake-indexeddb/auto";

// jsdom doesn't implement matchMedia; xterm.js's CoreBrowserService calls
// it during open(). A no-op stub is enough — we don't assert on rendered
// glyphs.
if (typeof window !== "undefined" && !window.matchMedia) {
  window.matchMedia = (q: string): MediaQueryList =>
    ({
      matches: false,
      media: q,
      onchange: null,
      addListener: () => undefined,
      removeListener: () => undefined,
      addEventListener: () => undefined,
      removeEventListener: () => undefined,
      dispatchEvent: () => false,
    }) as unknown as MediaQueryList;
}
