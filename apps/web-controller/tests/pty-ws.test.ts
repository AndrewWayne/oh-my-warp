import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import {
  connectPty,
  decodeFrame,
  encodeFrame,
  frameCanonicalBytes,
  buildConnectToken,
  type Frame,
} from "../src/lib/pty-ws";
import {
  generateKeypair,
  exportPrivateKeyJwk,
  exportPublicKeyRaw,
  sign,
  verify,
  _b64u,
} from "../src/lib/crypto/ed25519";
import type { PairingRecord } from "../src/lib/storage/idb";

// Hand-rolled WebSocket mock. The browser-native WebSocket can't be
// spun up in jsdom; we install ours via vi.stubGlobal so connectPty()
// uses it transparently.
type Listener = (ev: unknown) => void;
class MockWebSocket {
  static OPEN = 1;
  static CLOSED = 3;
  static instances: MockWebSocket[] = [];
  url: string;
  readyState = 0;
  sent: string[] = [];
  closeCalls: Array<{ code?: number; reason?: string }> = [];
  private listeners: Record<string, Listener[]> = {};
  // Mirror the static OPEN constant on the instance for libraries that
  // reference `ws.OPEN` (we use `WebSocket.OPEN` in connectPty so this is
  // belt + braces).
  OPEN = 1;
  CLOSED = 3;

  constructor(url: string) {
    this.url = url;
    MockWebSocket.instances.push(this);
    // Auto-open on next microtask so connectPty's awaitOpen resolves.
    queueMicrotask(() => {
      this.readyState = MockWebSocket.OPEN;
      this.dispatch("open", {});
    });
  }
  addEventListener(t: string, fn: Listener) {
    (this.listeners[t] ??= []).push(fn);
  }
  removeEventListener(t: string, fn: Listener) {
    const arr = this.listeners[t];
    if (!arr) return;
    const i = arr.indexOf(fn);
    if (i >= 0) arr.splice(i, 1);
  }
  send(data: string) {
    this.sent.push(data);
  }
  close(code?: number, reason?: string) {
    this.closeCalls.push({ code, reason });
    this.readyState = MockWebSocket.CLOSED;
    this.dispatch("close", { code: code ?? 1000, reason: reason ?? "" });
  }
  // Test-side helpers
  emitMessage(s: string) {
    this.dispatch("message", { data: s });
  }
  private dispatch(t: string, evLike: object) {
    const arr = this.listeners[t];
    if (!arr) return;
    for (const fn of arr.slice()) fn(evLike);
  }
}

async function makePairing(): Promise<{
  pairing: PairingRecord;
  hostPriv: Uint8Array;
  hostPub: Uint8Array;
  devicePub: Uint8Array;
}> {
  const device = await generateKeypair();
  const host = await generateKeypair();
  const devJwk = await exportPrivateKeyJwk(device.privateKey);
  const hostPubRaw = await exportPublicKeyRaw(host.publicKey);
  const devicePubRaw = await exportPublicKeyRaw(device.publicKey);
  return {
    pairing: {
      hostId: "h1",
      hostUrl: "https://h.example",
      hostPubkey: hostPubRaw,
      deviceId: "device-aaaa",
      privateKeyJwk: devJwk,
      capabilityTokenB64: "CAP_TOK",
      pairedAt: "2026-04-29T00:00:00Z",
      capabilities: ["pty:read", "pty:write"],
    },
    hostPriv: host.privateKey.raw,
    hostPub: hostPubRaw,
    devicePub: devicePubRaw,
  };
}

async function signFrameAsHost(
  frame: Omit<Frame, "sig">,
  hostPriv: Uint8Array,
): Promise<Frame> {
  const canonical = frameCanonicalBytes(frame);
  const sig = await sign({ type: "ed25519-private", raw: hostPriv }, canonical);
  return { ...frame, sig };
}

describe("pty-ws", () => {
  beforeEach(() => {
    MockWebSocket.instances.length = 0;
    vi.stubGlobal("WebSocket", MockWebSocket);
  });
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.useRealTimers();
  });

  describe("frameCanonicalBytes", () => {
    it("matches the Rust canonical form: keys sorted (kind,payload,seq,ts,v)", () => {
      const bytes = frameCanonicalBytes({
        v: 1,
        seq: 7,
        ts: "2026-04-29T15:00:00Z",
        kind: "input",
        payload: new TextEncoder().encode("hi"),
      });
      const text = new TextDecoder().decode(bytes);
      // base64url("hi") = "aGk"
      expect(text).toBe(
        '{"kind":"input","payload":"aGk","seq":7,"ts":"2026-04-29T15:00:00Z","v":1}',
      );
    });
  });

  describe("encode/decode", () => {
    it("roundtrips a frame through wire JSON", () => {
      const f: Frame = {
        v: 1,
        seq: 3,
        ts: "2026-04-29T15:00:00Z",
        kind: "output",
        payload: new Uint8Array([1, 2, 3]),
        sig: new Uint8Array(64).fill(7),
      };
      const wire = encodeFrame(f);
      const back = decodeFrame(wire);
      expect(back.seq).toBe(3);
      expect(back.kind).toBe("output");
      expect(Array.from(back.payload)).toEqual([1, 2, 3]);
      expect(Array.from(back.sig)).toEqual(Array.from(new Uint8Array(64).fill(7)));
    });

    it("rejects bad version", () => {
      expect(() => decodeFrame('{"v":2,"seq":0,"ts":"x","kind":"ping","payload":"","sig":""}'),
      ).toThrow();
    });
  });

  describe("buildConnectToken", () => {
    it("produces a base64url-encoded JSON bundle whose sig verifies", async () => {
      const { privateKey, publicKey } = await generateKeypair();
      const tok = await buildConnectToken({
        deviceId: "dev-xyz",
        privateKey,
        capabilityTokenB64: "CT",
        sessionId: "sess-1",
      });
      expect(tok).toMatch(/^[A-Za-z0-9_-]+$/);
      const json = new TextDecoder().decode(_b64u.decode(tok));
      const obj = JSON.parse(json);
      expect(obj.device_id).toBe("dev-xyz");
      expect(obj.capability_token).toBe("CT");
      expect(typeof obj.sig).toBe("string");
      // sig must verify against the canonical-request bytes for the
      // implied GET /ws/v1/pty/sess-1.
      const { canonicalBytes } = await import("../src/lib/crypto/canonical");
      const { bodyHashHex } = await import("../src/lib/crypto/canonical");
      const empty = await bodyHashHex(new Uint8Array(0));
      const canonical = canonicalBytes({
        method: "GET",
        path: "/ws/v1/pty/sess-1",
        query: "",
        ts: obj.ts,
        nonce: obj.nonce,
        bodySha256Hex: empty,
        deviceId: "dev-xyz",
        protocolVersion: 1,
      });
      const ok = await verify(publicKey, canonical, _b64u.decode(obj.sig));
      expect(ok).toBe(true);
    });
  });

  describe("connectPty", () => {
    it("opens WS to wss URL with ?ct= connect-token", async () => {
      const { pairing } = await makePairing();
      const conn = await connectPty({ pairing, sessionId: "S1" });
      expect(MockWebSocket.instances).toHaveLength(1);
      const url = MockWebSocket.instances[0]!.url;
      expect(url.startsWith("wss://h.example/ws/v1/pty/S1?ct=")).toBe(true);
      conn.close();
    });

    it("sendInput emits a signed frame with kind=input verifiable by device pubkey", async () => {
      const { pairing, devicePub } = await makePairing();
      const conn = await connectPty({ pairing, sessionId: "S1" });
      const ws = MockWebSocket.instances[0]!;
      ws.sent.length = 0;

      await conn.sendInput(new TextEncoder().encode("hello"));

      expect(ws.sent).toHaveLength(1);
      const frame = decodeFrame(ws.sent[0]!);
      expect(frame.kind).toBe("input");
      expect(frame.seq).toBe(0);
      expect(new TextDecoder().decode(frame.payload)).toBe("hello");
      const canonical = frameCanonicalBytes(frame);
      const ok = await verify(
        { type: "ed25519-public", raw: devicePub },
        canonical,
        frame.sig,
      );
      expect(ok).toBe(true);
      conn.close();
    });

    it("seq increments across outbound frames", async () => {
      const { pairing } = await makePairing();
      const conn = await connectPty({ pairing, sessionId: "S1" });
      const ws = MockWebSocket.instances[0]!;
      ws.sent.length = 0;
      await conn.sendInput(new Uint8Array([1]));
      await conn.sendInput(new Uint8Array([2]));
      const f0 = decodeFrame(ws.sent[0]!);
      const f1 = decodeFrame(ws.sent[1]!);
      expect(f0.seq).toBe(0);
      expect(f1.seq).toBe(1);
      conn.close();
    });

    it("delivers valid host-signed output frames to onOutput", async () => {
      const { pairing, hostPriv } = await makePairing();
      const conn = await connectPty({ pairing, sessionId: "S1" });
      const ws = MockWebSocket.instances[0]!;
      const got: Uint8Array[] = [];
      conn.onOutput((b) => got.push(b));

      const frame = await signFrameAsHost(
        {
          v: 1,
          seq: 0,
          ts: "2026-04-29T15:00:00Z",
          kind: "output",
          payload: new TextEncoder().encode("PTY-OUT"),
        },
        hostPriv,
      );
      ws.emitMessage(encodeFrame(frame));

      // Wait a microtask cycle for the async verify to land.
      await new Promise((r) => setTimeout(r, 5));
      expect(got).toHaveLength(1);
      expect(new TextDecoder().decode(got[0]!)).toBe("PTY-OUT");
      conn.close();
    });

    it("closes WS on inbound seq regression", async () => {
      const { pairing, hostPriv } = await makePairing();
      const conn = await connectPty({ pairing, sessionId: "S1" });
      const ws = MockWebSocket.instances[0]!;
      const closes: Array<{ code: number; reason: string }> = [];
      conn.onClose((c) => closes.push(c));

      const f1 = await signFrameAsHost(
        {
          v: 1,
          seq: 5,
          ts: "2026-04-29T15:00:00Z",
          kind: "output",
          payload: new Uint8Array([1]),
        },
        hostPriv,
      );
      ws.emitMessage(encodeFrame(f1));
      await new Promise((r) => setTimeout(r, 5));

      const f2 = await signFrameAsHost(
        {
          v: 1,
          seq: 4, // regression
          ts: "2026-04-29T15:00:00Z",
          kind: "output",
          payload: new Uint8Array([2]),
        },
        hostPriv,
      );
      ws.emitMessage(encodeFrame(f2));
      await new Promise((r) => setTimeout(r, 5));

      expect(ws.closeCalls.length).toBeGreaterThanOrEqual(1);
      expect(ws.closeCalls[0]!.code).toBe(4401);
      expect(closes.some((c) => c.code === 4401)).toBe(true);
    });

    it("closes WS on inbound bad signature", async () => {
      const { pairing } = await makePairing();
      const conn = await connectPty({ pairing, sessionId: "S1" });
      const ws = MockWebSocket.instances[0]!;
      const closes: Array<{ code: number; reason: string }> = [];
      conn.onClose((c) => closes.push(c));

      const bogus: Frame = {
        v: 1,
        seq: 0,
        ts: "2026-04-29T15:00:00Z",
        kind: "output",
        payload: new Uint8Array([1, 2, 3]),
        sig: new Uint8Array(64).fill(0xaa), // not a valid ed25519 sig
      };
      ws.emitMessage(encodeFrame(bogus));
      await new Promise((r) => setTimeout(r, 30));

      expect(ws.closeCalls.length).toBeGreaterThanOrEqual(1);
      expect(ws.closeCalls[0]!.code).toBe(4401);
      expect(closes.some((c) => c.code === 4401)).toBe(true);
    });

    it("emits a kind=ping frame after pingIntervalMs", async () => {
      vi.useFakeTimers();
      const { pairing } = await makePairing();
      const conn = await connectPty({ pairing, sessionId: "S1", pingIntervalMs: 50 });
      const ws = MockWebSocket.instances[0]!;
      ws.sent.length = 0;

      await vi.advanceTimersByTimeAsync(60);
      // The ping send is async; flush microtasks.
      await vi.runOnlyPendingTimersAsync();
      vi.useRealTimers();
      // Allow any pending sign() promise to land.
      await new Promise((r) => setTimeout(r, 30));

      expect(ws.sent.length).toBeGreaterThanOrEqual(1);
      const sentKinds = ws.sent.map((s) => decodeFrame(s).kind);
      expect(sentKinds).toContain("ping");
      conn.close();
    });
  });
});
