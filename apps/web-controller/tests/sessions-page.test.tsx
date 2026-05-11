import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import Sessions from "../src/pages/Sessions";
import { getPairing } from "../src/lib/storage/idb";
import { listSessions } from "../src/lib/sessions";
import type { PairingRecord } from "../src/lib/storage/idb";

vi.mock("../src/lib/storage/idb", () => ({
  getPairing: vi.fn(),
  deletePairing: vi.fn(),
}));

vi.mock("../src/lib/sessions", () => ({
  createDefaultSession: vi.fn(),
  deleteSession: vi.fn(),
  listSessions: vi.fn(),
}));

const pairing: PairingRecord = {
  hostId: "qa-host",
  hostUrl: "http://127.0.0.1:8787",
  hostPubkey: new Uint8Array(32).fill(9),
  deviceId: "device-qa",
  privateKeyJwk: { kty: "OKP", crv: "Ed25519", d: "x", x: "y" },
  capabilityTokenB64: "QA_CAP",
  pairedAt: "2026-05-09T00:00:00Z",
  capabilities: ["pty:read", "pty:write"],
};

const session = {
  id: "11111111-1111-4111-8111-111111111111",
  name: "qa-shell",
  createdAt: "2026-05-09T00:00:00Z",
  alive: true,
};

function renderAt(path: string) {
  return render(
    <MemoryRouter initialEntries={[path]}>
      <Routes>
        <Route path="/host/:hostId" element={<Sessions />} />
        <Route
          path="/terminal/:hostId/:sessionId"
          element={<div data-testid="terminal-route">terminal route</div>}
        />
      </Routes>
    </MemoryRouter>,
  );
}

describe("Sessions page", () => {
  beforeEach(() => {
    vi.mocked(getPairing).mockReset();
    vi.mocked(listSessions).mockReset();
    vi.mocked(getPairing).mockResolvedValue(pairing);
    vi.mocked(listSessions).mockResolvedValue([session]);
  });

  it("does not auto-open the only alive session when the user navigates back to sessions", async () => {
    renderAt("/host/qa-host");

    await waitFor(() => {
      expect(screen.getByRole("heading", { name: "qa-host" })).toBeInTheDocument();
    });
    expect(screen.getByRole("button", { name: "Open" })).toBeInTheDocument();
    expect(screen.queryByTestId("terminal-route")).not.toBeInTheDocument();
  });

  it("auto-opens the only alive session when arriving from the pair flow", async () => {
    renderAt("/host/qa-host?auto=1");

    await waitFor(() => {
      expect(screen.getByTestId("terminal-route")).toBeInTheDocument();
    });
  });
});
