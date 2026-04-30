import { describe, it, expect, vi, beforeEach } from "vitest";
import {
  parsePairUrl,
  redeemPairing,
  PairError,
} from "../src/lib/pairing";
import { _b64u } from "../src/lib/crypto/ed25519";

describe("parsePairUrl", () => {
  it("accepts a valid https pair URL", () => {
    const u = parsePairUrl(
      "https://home-mac.tail-1234.ts.net/pair?t=ABC123XYZ",
    );
    expect(u).not.toBeNull();
    expect(u!.baseUrl).toBe("https://home-mac.tail-1234.ts.net");
    expect(u!.token).toBe("ABC123XYZ");
  });

  it("accepts http loopback with port", () => {
    const u = parsePairUrl("http://127.0.0.1:8787/pair?t=DEADBEEF");
    expect(u).not.toBeNull();
    expect(u!.baseUrl).toBe("http://127.0.0.1:8787");
    expect(u!.token).toBe("DEADBEEF");
  });

  it("rejects empty / malformed input", () => {
    expect(parsePairUrl("")).toBeNull();
    expect(parsePairUrl("   ")).toBeNull();
    expect(parsePairUrl("not a url")).toBeNull();
  });

  it("rejects wrong path", () => {
    expect(
      parsePairUrl("https://host.example/pairing?t=ABC123"),
    ).toBeNull();
  });

  it("rejects missing token", () => {
    expect(parsePairUrl("https://host.example/pair")).toBeNull();
  });

  it("rejects non-Crockford token characters", () => {
    expect(
      parsePairUrl("https://host.example/pair?t=ABC$DEF"),
    ).toBeNull();
  });

  it("rejects unsupported scheme", () => {
    expect(parsePairUrl("ftp://host.example/pair?t=ABC")).toBeNull();
  });
});

function makeOkResponse(): Response {
  // host_pubkey: 32 bytes of 0x07 → base64url
  const hostPub = new Uint8Array(32).fill(7);
  const body = {
    v: 1,
    device_id: "a1b2c3d4e5f6a7b8",
    capabilities: ["pty:read", "agent:read", "audit:read"],
    capability_token: "BASE64_TOKEN_HERE",
    host_pubkey: _b64u.encode(hostPub),
    host_name: "home-mac",
    issued_at: "2026-04-29T15:00:00Z",
    expires_at: "2026-05-29T15:00:00Z",
  };
  return new Response(JSON.stringify(body), {
    status: 200,
    headers: { "Content-Type": "application/json" },
  });
}

function makeErrorResponse(status: number, code: string): Response {
  const body = { error: { code, message: code, trace_id: "t-1" } };
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

describe("redeemPairing", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it("posts to /api/v1/pair/redeem with the right body and returns RedeemResult on success", async () => {
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValue(makeOkResponse());

    const result = await redeemPairing(
      { baseUrl: "https://h.example", token: "ABC123" },
      "Mark's iPhone",
      "ios",
      fetchMock,
    );

    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [url, init] = fetchMock.mock.calls[0]!;
    expect(url).toBe("https://h.example/api/v1/pair/redeem");
    expect(init?.method).toBe("POST");
    const headers = init?.headers as Record<string, string>;
    expect(headers["Content-Type"]).toBe("application/json");
    const sent = JSON.parse(init?.body as string);
    expect(sent.v).toBe(1);
    expect(sent.pairing_token).toBe("ABC123");
    expect(sent.device_name).toBe("Mark's iPhone");
    expect(sent.platform).toBe("ios");
    expect(typeof sent.device_pubkey).toBe("string");
    expect(typeof sent.client_nonce).toBe("string");

    expect(result.deviceId).toBe("a1b2c3d4e5f6a7b8");
    expect(result.capabilities).toEqual([
      "pty:read",
      "agent:read",
      "audit:read",
    ]);
    expect(result.capabilityTokenB64).toBe("BASE64_TOKEN_HERE");
    expect(result.hostPubkey.length).toBe(32);
    expect(result.hostUrl).toBe("https://h.example");
    expect(result.hostId).toBe("home-mac");
    expect(result.privateKey.type).toBe("ed25519-private");
    expect(result.privateKeyJwk.kty).toBe("OKP");
  });

  it("throws PairError code=token_expired on 410", async () => {
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValue(makeErrorResponse(410, "token_expired"));

    await expect(
      redeemPairing(
        { baseUrl: "https://h.example", token: "X" },
        "d",
        "web",
        fetchMock,
      ),
    ).rejects.toMatchObject({
      name: "PairError",
      code: "token_expired",
      httpStatus: 410,
    });
  });

  it("throws PairError code=token_already_used on 409", async () => {
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValue(makeErrorResponse(409, "token_already_used"));

    await expect(
      redeemPairing(
        { baseUrl: "https://h.example", token: "X" },
        "d",
        "web",
        fetchMock,
      ),
    ).rejects.toMatchObject({
      code: "token_already_used",
      httpStatus: 409,
    });
  });

  it("throws PairError code=invalid_pubkey on 400", async () => {
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValue(makeErrorResponse(400, "invalid_pubkey"));

    await expect(
      redeemPairing(
        { baseUrl: "https://h.example", token: "X" },
        "d",
        "web",
        fetchMock,
      ),
    ).rejects.toMatchObject({
      code: "invalid_pubkey",
      httpStatus: 400,
    });
  });

  it("throws PairError code=network_error when fetch rejects", async () => {
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockRejectedValue(new TypeError("Failed to fetch"));

    let caught: unknown;
    try {
      await redeemPairing(
        { baseUrl: "https://h.example", token: "X" },
        "d",
        "web",
        fetchMock,
      );
    } catch (e) {
      caught = e;
    }
    expect(caught).toBeInstanceOf(PairError);
    expect((caught as PairError).code).toBe("network_error");
    expect((caught as PairError).httpStatus).toBeUndefined();
  });
});
