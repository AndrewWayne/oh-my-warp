import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter, Routes, Route } from "react-router-dom";
import Terminal from "../src/pages/Terminal";
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

  it("renders a back button that links to /host/:hostId (Stage C.4)", () => {
    renderAt("/terminal/h1/sess-7");
    const back = screen.getByTestId("terminal-back-button");
    expect(back).toBeInTheDocument();
    expect(back).toHaveAttribute("href", "/host/h1");
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
});
