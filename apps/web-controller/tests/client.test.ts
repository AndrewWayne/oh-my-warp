import { describe, it, expect, vi, beforeEach } from "vitest";
import { ApiClient } from "../src/lib/api/client";
import {
  generateKeypair,
  verify,
  _b64u,
} from "../src/lib/crypto/ed25519";
import { canonicalBytes, bodyHashHex } from "../src/lib/crypto/canonical";

describe("ApiClient", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it("sends signed GET with correct headers and signature over canonical bytes", async () => {
    const { privateKey, publicKey } = await generateKeypair();
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValue(new Response("{}", { status: 200 }));
    globalThis.fetch = fetchMock;

    const client = new ApiClient({
      deviceId: "device-abc",
      privateKey,
      capabilityTokenB64: "FAKE_CAP_TOKEN",
      baseUrl: "https://host.tailnet.ts.net",
      protocolVersion: 1,
    });

    const res = await client.get("/api/v1/foo");
    expect(res.status).toBe(200);
    expect(fetchMock).toHaveBeenCalledTimes(1);

    const [url, init] = fetchMock.mock.calls[0]!;
    expect(url).toBe("https://host.tailnet.ts.net/api/v1/foo");
    expect(init?.method).toBe("GET");
    const headers = init?.headers as Record<string, string>;
    expect(headers["Authorization"]).toBe("Bearer FAKE_CAP_TOKEN");
    expect(headers["X-Omw-Protocol-Version"]).toBe("1");
    expect(headers["X-Omw-Nonce"]).toMatch(/^[A-Za-z0-9_-]+$/);
    expect(headers["X-Omw-Ts"]).toMatch(
      /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}/
    );
    expect(headers["X-Omw-Signature"]).toMatch(/^[A-Za-z0-9_-]+$/);

    // No body on GET → no Content-Type, undefined body
    expect(headers["Content-Type"]).toBeUndefined();
    expect(init?.body).toBeUndefined();

    // Reconstruct canonical bytes and verify the signature
    const emptyHash = await bodyHashHex(new Uint8Array(0));
    const canonical = canonicalBytes({
      method: "GET",
      path: "/api/v1/foo",
      query: "",
      ts: headers["X-Omw-Ts"]!,
      nonce: headers["X-Omw-Nonce"]!,
      bodySha256Hex: emptyHash,
      deviceId: "device-abc",
      protocolVersion: 1,
    });
    const sig = _b64u.decode(headers["X-Omw-Signature"]!);
    expect(sig.length).toBe(64);
    const ok = await verify(publicKey, canonical, sig);
    expect(ok).toBe(true);
  });

  it("sends Content-Type and JSON body on POST, signature covers body hash", async () => {
    const { privateKey, publicKey } = await generateKeypair();
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValue(new Response("{}", { status: 201 }));
    globalThis.fetch = fetchMock;

    const client = new ApiClient({
      deviceId: "device-xyz",
      privateKey,
      capabilityTokenB64: "T",
      baseUrl: "https://h.example",
      protocolVersion: 1,
    });

    const body = { hello: "world" };
    await client.post("/api/v1/sessions", body);

    const [, init] = fetchMock.mock.calls[0]!;
    const headers = init?.headers as Record<string, string>;
    expect(headers["Content-Type"]).toBe("application/json");

    const sentBytes = init?.body as Uint8Array;
    const expectedBody = new TextEncoder().encode(JSON.stringify(body));
    expect(Array.from(sentBytes)).toEqual(Array.from(expectedBody));

    const bodyHash = await bodyHashHex(expectedBody);
    const canonical = canonicalBytes({
      method: "POST",
      path: "/api/v1/sessions",
      query: "",
      ts: headers["X-Omw-Ts"]!,
      nonce: headers["X-Omw-Nonce"]!,
      bodySha256Hex: bodyHash,
      deviceId: "device-xyz",
      protocolVersion: 1,
    });
    const sig = _b64u.decode(headers["X-Omw-Signature"]!);
    const ok = await verify(publicKey, canonical, sig);
    expect(ok).toBe(true);
  });
});
