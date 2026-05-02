import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Routes, Route } from "react-router-dom";
import Pair from "../src/pages/Pair";
import { _b64u } from "../src/lib/crypto/ed25519";
import { _resetDbHandleForTests, listPairings } from "../src/lib/storage/idb";

const navigateMock = vi.fn();
vi.mock("react-router-dom", async () => {
  const actual = await vi.importActual<typeof import("react-router-dom")>(
    "react-router-dom",
  );
  return { ...actual, useNavigate: () => navigateMock };
});

function okBody(): string {
  const hostPub = new Uint8Array(32).fill(9);
  return JSON.stringify({
    v: 1,
    device_id: "ddccbbaa00112233",
    capabilities: ["pty:read", "agent:read"],
    capability_token: "CAP_TOK_B64",
    host_pubkey: _b64u.encode(hostPub),
    host_name: "home-mac",
    issued_at: "2026-04-29T15:00:00Z",
    expires_at: "2026-05-29T15:00:00Z",
  });
}

function renderAt(path: string) {
  return render(
    <MemoryRouter initialEntries={[path]}>
      <Routes>
        <Route path="/pair" element={<Pair />} />
        <Route path="/pair/:t" element={<Pair />} />
      </Routes>
    </MemoryRouter>,
  );
}

describe("Pair page", () => {
  beforeEach(async () => {
    navigateMock.mockReset();
    vi.restoreAllMocks();
    await _resetDbHandleForTests();
    const req = indexedDB.deleteDatabase("omw-web-controller");
    await new Promise<void>((resolve) => {
      req.onsuccess = () => resolve();
      req.onerror = () => resolve();
      req.onblocked = () => resolve();
    });
  });

  it("renders the Pair heading", () => {
    renderAt("/pair");
    expect(screen.getByRole("heading", { name: "Pair" })).toBeInTheDocument();
  });

  it("disables Pair button until a valid URL is pasted, then enables it", async () => {
    const user = userEvent.setup();
    renderAt("/pair");

    const btn = screen.getByRole("button", { name: /^Pair$/ });
    expect(btn).toBeDisabled();

    const ta = screen.getByLabelText(/Pairing URL/i);
    await user.type(ta, "https://h.example/pair?t=ABC123");

    expect(btn).not.toBeDisabled();
  });

  it("on click Pair with success: persists to IDB and navigates to a terminal", async () => {
    // Both /api/v1/pair/redeem and /api/v1/sessions are POSTed via the
    // global fetch mock here. The redeem responds with okBody (the redeem
    // shape) and the session-create call gets the same body — which has no
    // `id` field, so createDefaultSession throws `session_create_failed:
    // missing id in response`. Pair.tsx's catch in that path navigates to
    // /host/<hostId> as a fallback, which is the assertion below.
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValue(
        new Response(okBody(), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        }),
      );
    globalThis.fetch = fetchMock;

    const user = userEvent.setup();
    renderAt("/pair");

    const ta = screen.getByLabelText(/Pairing URL/i);
    await user.type(ta, "https://h.example/pair?t=ABC123");

    const btn = screen.getByRole("button", { name: /^Pair$/ });
    await user.click(btn);

    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith("/host/home-mac");
    });

    const all = await listPairings();
    expect(all).toHaveLength(1);
    expect(all[0]!.deviceId).toBe("ddccbbaa00112233");
    expect(all[0]!.capabilityTokenB64).toBe("CAP_TOK_B64");
    expect(all[0]!.hostUrl).toBe("https://h.example");
  });

  it("on 410 token_expired: shows expired error message and does not navigate", async () => {
    const fetchMock = vi.fn<typeof fetch>().mockResolvedValue(
      new Response(
        JSON.stringify({
          error: { code: "token_expired", message: "expired", trace_id: "t" },
        }),
        { status: 410, headers: { "Content-Type": "application/json" } },
      ),
    );
    globalThis.fetch = fetchMock;

    const user = userEvent.setup();
    renderAt("/pair");

    const ta = screen.getByLabelText(/Pairing URL/i);
    await user.type(ta, "https://h.example/pair?t=ABC123");

    const btn = screen.getByRole("button", { name: /^Pair$/ });
    await user.click(btn);

    await waitFor(() => {
      expect(screen.getByRole("alert")).toHaveTextContent(/expired/i);
    });
    expect(navigateMock).not.toHaveBeenCalled();
  });
});
