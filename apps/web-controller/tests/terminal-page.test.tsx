import { describe, it, expect, vi, beforeEach } from "vitest";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Routes, Route } from "react-router-dom";
import Terminal from "../src/pages/Terminal";
import { terminalControlBytes } from "../src/lib/terminal-control-bytes";
import {
  _resetDbHandleForTests,
  savePairing,
  type PairingRecord,
} from "../src/lib/storage/idb";
import {
  generateKeypair,
  exportPrivateKeyJwk,
  exportPublicKeyRaw,
} from "../src/lib/crypto/ed25519";

const navigateMock = vi.fn();
vi.mock("react-router-dom", async () => {
  const actual = await vi.importActual<typeof import("react-router-dom")>(
    "react-router-dom",
  );
  return { ...actual, useNavigate: () => navigateMock };
});

const stubConnection = {
  sendInput: vi.fn().mockResolvedValue(undefined),
  sendControl: vi.fn().mockResolvedValue(undefined),
  onOutput: vi.fn(() => () => undefined),
  onControl: vi.fn(() => () => undefined),
  onClose: vi.fn(() => () => undefined),
  ping: vi.fn().mockResolvedValue(undefined),
  close: vi.fn(),
};

vi.mock("../src/lib/pty-ws", () => {
  return {
    connectPty: vi.fn(async () => stubConnection),
  };
});

async function seedPairing(hostId: string): Promise<PairingRecord> {
  const dev = await generateKeypair();
  const host = await generateKeypair();
  const rec: PairingRecord = {
    hostId,
    hostUrl: "https://h.example",
    hostPubkey: await exportPublicKeyRaw(host.publicKey),
    deviceId: "device-aaaa",
    privateKeyJwk: await exportPrivateKeyJwk(dev.privateKey),
    capabilityTokenB64: "CAP_TOK",
    pairedAt: "2026-04-29T00:00:00Z",
    capabilities: ["pty:read", "pty:write"],
  };
  await savePairing(rec);
  return rec;
}

function renderAt(path: string) {
  return render(
    <MemoryRouter initialEntries={[path]}>
      <Routes>
        <Route path="/terminal/:hostId/:sessionId" element={<Terminal />} />
      </Routes>
    </MemoryRouter>,
  );
}

describe("Terminal page", () => {
  beforeEach(async () => {
    navigateMock.mockReset();
    stubConnection.sendInput.mockClear();
    stubConnection.sendControl.mockClear();
    stubConnection.onOutput.mockClear();
    stubConnection.onControl.mockClear();
    stubConnection.onClose.mockClear();
    stubConnection.close.mockClear();
    await _resetDbHandleForTests();
    const req = indexedDB.deleteDatabase("omw-web-controller");
    await new Promise<void>((resolve) => {
      req.onsuccess = () => resolve();
      req.onerror = () => resolve();
      req.onblocked = () => resolve();
    });
  });

  it("renders the Terminal heading and host/session params", () => {
    renderAt("/terminal/h1/sess-7");
    expect(screen.getByRole("heading", { name: "Terminal" })).toBeInTheDocument();
    expect(screen.getByText(/sess-7/)).toBeInTheDocument();
  });

  it("wraps long host/session identifiers instead of widening the mobile page", () => {
    renderAt(
      "/terminal/qa-host/11111111-1111-4111-8111-111111111111",
    );

    expect(screen.getByTestId("terminal-shell")).toHaveClass(
      "w-full",
      "min-w-0",
      "overflow-hidden",
    );
    expect(screen.getByTestId("xterm-container")).toHaveClass(
      "min-w-0",
      "overflow-hidden",
    );
    expect(screen.getByTestId("terminal-host-session-meta")).toHaveClass(
      "hidden",
      "break-all",
      "max-w-full",
    );
  });

  it("uses compact mobile terminal chrome so the keyboard mode has more room", () => {
    renderAt("/terminal/h1/sess-7");

    expect(screen.getByTestId("terminal-mobile-toolbar")).toHaveClass(
      "min-h-10",
      "px-2",
      "sm:px-0",
    );
    expect(screen.getByRole("heading", { name: "Terminal" })).toHaveClass(
      "sr-only",
      "sm:not-sr-only",
    );
    expect(screen.getByTestId("xterm-container")).toHaveClass(
      "border-y",
      "sm:rounded",
    );
  });

  it("renders a back button that links to /host/:hostId (Stage C.4)", () => {
    renderAt("/terminal/h1/sess-7");
    const back = screen.getByTestId("terminal-back-button");
    expect(back).toBeInTheDocument();
    expect(back).toHaveAttribute("href", "/host/h1");
  });

  it("sizes the terminal shell to the visible viewport below its page offset", async () => {
    const originalInnerHeight = window.innerHeight;
    const rectSpy = vi
      .spyOn(HTMLElement.prototype, "getBoundingClientRect")
      .mockImplementation(function (this: HTMLElement) {
        const top = this.getAttribute("data-testid") === "terminal-shell" ? 72 : 0;
        return new DOMRect(0, top, 0, 0);
      });
    Object.defineProperty(window, "innerHeight", {
      configurable: true,
      value: 600,
    });

    try {
      renderAt("/terminal/h1/sess-7");
      await waitFor(() => {
        expect(screen.getByTestId("terminal-shell")).toHaveStyle({
          height: "528px",
        });
      });
    } finally {
      rectSpy.mockRestore();
      Object.defineProperty(window, "innerHeight", {
        configurable: true,
        value: originalInnerHeight,
      });
    }
  });

  it("uses the layout viewport while the keyboard is closed to avoid a dead tail below the strip", async () => {
    const originalInnerHeight = window.innerHeight;
    const originalVisualViewport = (
      window as Window & { visualViewport?: VisualViewport }
    ).visualViewport;
    const rectSpy = vi
      .spyOn(HTMLElement.prototype, "getBoundingClientRect")
      .mockImplementation(function (this: HTMLElement) {
        const top = this.getAttribute("data-testid") === "terminal-shell" ? 80 : 0;
        return new DOMRect(0, top, 0, 0);
      });
    Object.defineProperty(window, "innerHeight", {
      configurable: true,
      value: 700,
    });
    Object.defineProperty(window, "visualViewport", {
      configurable: true,
      value: {
        height: 560,
        width: 390,
        offsetLeft: 0,
        offsetTop: 0,
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
      },
    });

    try {
      renderAt("/terminal/h1/sess-7");
      await waitFor(() => {
        expect(screen.getByTestId("terminal-shell")).toHaveStyle({
          height: "620px",
        });
      });
    } finally {
      rectSpy.mockRestore();
      Object.defineProperty(window, "innerHeight", {
        configurable: true,
        value: originalInnerHeight,
      });
      Object.defineProperty(window, "visualViewport", {
        configurable: true,
        value: originalVisualViewport,
      });
    }
  });

  it("uses the visual viewport when the keyboard is open", async () => {
    const originalInnerHeight = window.innerHeight;
    const originalVisualViewport = (
      window as Window & { visualViewport?: VisualViewport }
    ).visualViewport;
    const rectSpy = vi
      .spyOn(HTMLElement.prototype, "getBoundingClientRect")
      .mockImplementation(function (this: HTMLElement) {
        const top = this.getAttribute("data-testid") === "terminal-shell" ? 80 : 0;
        return new DOMRect(0, top, 0, 0);
      });
    Object.defineProperty(window, "innerHeight", {
      configurable: true,
      value: 700,
    });
    Object.defineProperty(window, "visualViewport", {
      configurable: true,
      value: {
        height: 360,
        width: 390,
        offsetLeft: 0,
        offsetTop: 0,
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
      },
    });

    try {
      renderAt("/terminal/h1/sess-7");
      await waitFor(() => {
        expect(screen.getByTestId("terminal-shell")).toHaveStyle({
          height: "280px",
        });
      });
    } finally {
      rectSpy.mockRestore();
      Object.defineProperty(window, "innerHeight", {
        configurable: true,
        value: originalInnerHeight,
      });
      Object.defineProperty(window, "visualViewport", {
        configurable: true,
        value: originalVisualViewport,
      });
    }
  });

  it("redirects to /pair when no PairingRecord exists for the hostId", async () => {
    renderAt("/terminal/missing-host/sess-1");
    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith("/pair");
    });
  });

  it("connects via connectPty and registers an onOutput handler", async () => {
    await seedPairing("h1");
    const { connectPty } = await import("../src/lib/pty-ws");
    renderAt("/terminal/h1/sess-7");

    await waitFor(() => {
      expect(connectPty).toHaveBeenCalled();
    });

    // The page subscribes to inbound output on the connection.
    await waitFor(() => {
      expect(stubConnection.onOutput).toHaveBeenCalled();
    });

    // Connection-status badge transitions to "connected".
    await waitFor(() => {
      expect(screen.getByTestId("conn-status").textContent).toMatch(
        /connected/i,
      );
    });

    // The xterm container div is mounted (we don't assert on xterm
    // internals because jsdom can't measure dimensions).
    expect(screen.getByTestId("xterm-container")).toBeInTheDocument();
  });

  it("disables the shortcut strip while loading/connecting and enables it once connected", async () => {
    await seedPairing("h1");
    renderAt("/terminal/h1/sess-7");

    // Before connectPty resolves, the strip is mounted but every button is
    // disabled — no stale input is queued while connection state is
    // anything other than "connected".
    expect(screen.getByRole("button", { name: /^esc$/i })).toBeDisabled();

    await waitFor(() => {
      expect(screen.getByRole("button", { name: /^esc$/i })).not.toBeDisabled();
    });
  });

  it("shortcut tap routes through the same PtyConnection.sendInput as xterm input", async () => {
    const user = userEvent.setup();
    await seedPairing("h1");
    renderAt("/terminal/h1/sess-7");

    await waitFor(() => {
      expect(screen.getByRole("button", { name: /^esc$/i })).not.toBeDisabled();
    });

    await user.click(screen.getByRole("button", { name: /^esc$/i }));
    expect(stubConnection.sendInput).toHaveBeenCalledTimes(1);
    const sent = stubConnection.sendInput.mock.calls[0][0] as Uint8Array;
    expect(Array.from(sent)).toEqual(Array.from(terminalControlBytes("esc")));
  });

  it("does not call sendInput when buttons are disconnected (disabled)", async () => {
    // Render without a PairingRecord — page redirects to /pair and never
    // reaches "connected", so taps on the strip should not queue input.
    renderAt("/terminal/missing-host/sess-1");
    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith("/pair");
    });
    const buttons = screen.queryAllByRole("button");
    for (const b of buttons) {
      // Skip the back/retry buttons, only check shortcut keys.
      if (/^esc$|^tab$|\^C|shift|arrow|enter|more/i.test(b.textContent ?? "")) {
        expect(b).toBeDisabled();
      }
    }
    expect(stubConnection.sendInput).not.toHaveBeenCalled();
  });

  it("dedupes resize control frames when fit reports unchanged rows/cols", async () => {
    await seedPairing("h1");
    renderAt("/terminal/h1/sess-7");

    await waitFor(() => {
      expect(screen.getByTestId("conn-status").textContent).toMatch(/connected/i);
    });

    const initialResizes = stubConnection.sendControl.mock.calls.filter(
      (c) => (c[0] as { type?: string })?.type === "resize",
    ).length;

    // Fire several window resize events back-to-back. jsdom's xterm reports
    // the same rows/cols (it can't measure), so the scheduler must collapse
    // these into at most one outbound resize frame for the unchanged pair.
    await act(async () => {
      for (let i = 0; i < 5; i++) {
        window.dispatchEvent(new Event("resize"));
      }
      // Wait past the trailing 80ms timer.
      await new Promise((resolve) => setTimeout(resolve, 200));
    });

    const afterResizes = stubConnection.sendControl.mock.calls.filter(
      (c) => (c[0] as { type?: string })?.type === "resize",
    ).length;

    // jsdom's xterm size doesn't change between fit calls, so dedupe should
    // suppress every duplicate frame.
    expect(afterResizes).toBe(initialResizes);
  });

  it("does not crash when fit/resize fires before connect resolves", async () => {
    await seedPairing("h1");
    renderAt("/terminal/h1/sess-7");

    // Fire a resize before connection resolves; runFit must be tolerant of
    // missing connection ref / unmeasurable jsdom dimensions.
    await act(async () => {
      window.dispatchEvent(new Event("resize"));
      window.dispatchEvent(new Event("orientationchange"));
    });

    await waitFor(() => {
      expect(screen.getByTestId("conn-status").textContent).toMatch(/connected/i);
    });
  });
});
