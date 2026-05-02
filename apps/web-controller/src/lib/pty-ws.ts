// Client-side WS connection to /ws/v1/pty/:session_id with per-frame Ed25519
// signing (device key) on outbound and verification (host pairing key) on
// inbound. See specs/byorc-protocol.md §7.2 + §7.3.
//
// Auth scheme divergence from the spec: the protocol §7.1 calls for the
// signed-request HTTP headers on the WS upgrade, but browser `WebSocket`
// can't set arbitrary headers. Phase I uses a **connect token** carried in
// the URL query: `?ct=<base64url(JSON{ device_id, ts, nonce, sig,
// capability_token, v })>`. The server-side acceptance of `?ct=` is a
// v0.4-cleanup item (tracked Open Question); Phase I tests run against a
// mock WS so we exercise the client side end-to-end.
//
// On the wire each frame is JSON of shape:
//   { v, seq, ts, kind, payload: <b64url>, sig: <b64url(64)> }
// The signed-canonical form is the same JSON minus `sig`, sorted keys, no
// whitespace, payload as base64url. This matches `Frame::canonical_bytes`
// in `crates/omw-remote/src/ws/frame.rs` exactly.

import {
  type CryptoPrivateKey,
  type CryptoPublicKey,
  importPrivateKeyJwk,
  importPublicKeyRaw,
  sign as edSign,
  verify as edVerify,
  _b64u,
} from "./crypto/ed25519";
import { canonicalBytes, bodyHashHex } from "./crypto/canonical";
import type { PairingRecord } from "./storage/idb";

export type FrameKind = "input" | "output" | "control" | "ping" | "pong";

export interface Frame {
  v: number;
  seq: number;
  ts: string;
  kind: FrameKind;
  payload: Uint8Array;
  sig: Uint8Array;
}

export interface PtyConnection {
  sendInput(bytes: Uint8Array): Promise<void>;
  sendControl(payload: object): Promise<void>;
  onOutput(handler: (bytes: Uint8Array) => void): () => void;
  /**
   * Subscribe to inbound `Control` frames. The daemon sends these for
   * out-of-band signals — e.g. an initial `{type:"size", rows, cols}` on
   * attach so the phone xterm can resize itself to match the laptop pane
   * (otherwise cursor-positioning bytes from the TUI clamp to the phone's
   * smaller grid and content piles up at the boundary).
   */
  onControl(handler: (payload: unknown) => void): () => void;
  onClose(handler: (info: { code: number; reason: string }) => void): () => void;
  ping(): Promise<void>;
  close(): void;
}

export interface ConnectOptions {
  pairing: PairingRecord;
  sessionId: string;
  pingIntervalMs?: number;
  /**
   * Optional sink for connection-lifecycle debug events. Useful on devices
   * without DevTools (e.g. iPhone Safari) — the page can render these to a
   * visible panel and surface what's happening during a stuck connection.
   */
  onDebug?: (msg: string) => void;
}

/** Build the canonical JSON bytes for a frame envelope (sig omitted). */
export function frameCanonicalBytes(f: {
  v: number;
  seq: number;
  ts: string;
  kind: FrameKind;
  payload: Uint8Array;
}): Uint8Array {
  // Order matches Rust Frame::canonical_bytes: kind, payload, seq, ts, v.
  const payloadB64 = _b64u.encode(f.payload);
  const s =
    "{" +
    `"kind":${JSON.stringify(f.kind)}` +
    `,"payload":${JSON.stringify(payloadB64)}` +
    `,"seq":${f.seq}` +
    `,"ts":${JSON.stringify(f.ts)}` +
    `,"v":${f.v}` +
    "}";
  return new TextEncoder().encode(s);
}

/** Encode a Frame to wire JSON (sig included). */
export function encodeFrame(f: Frame): string {
  return JSON.stringify({
    v: f.v,
    seq: f.seq,
    ts: f.ts,
    kind: f.kind,
    payload: _b64u.encode(f.payload),
    sig: _b64u.encode(f.sig),
  });
}

interface WireFrame {
  v?: number;
  seq?: number;
  ts?: string;
  kind?: string;
  payload?: string;
  sig?: string;
}

/** Decode a wire-JSON frame; throws on malformed envelope. */
export function decodeFrame(s: string): Frame {
  const w = JSON.parse(s) as WireFrame;
  if (w.v !== 1) throw new Error("unsupported_version");
  if (
    typeof w.seq !== "number" ||
    typeof w.ts !== "string" ||
    typeof w.kind !== "string" ||
    typeof w.payload !== "string" ||
    typeof w.sig !== "string"
  ) {
    throw new Error("malformed_frame");
  }
  if (!isFrameKind(w.kind)) throw new Error("malformed_frame");
  const payload = _b64u.decode(w.payload);
  const sig = _b64u.decode(w.sig);
  if (sig.length !== 64) throw new Error("malformed_frame");
  return { v: 1, seq: w.seq, ts: w.ts, kind: w.kind, payload, sig };
}

function isFrameKind(s: string): s is FrameKind {
  return (
    s === "input" || s === "output" || s === "control" || s === "ping" || s === "pong"
  );
}

/** Build the `?ct=` connect-token bundle (base64url JSON). */
export async function buildConnectToken(args: {
  deviceId: string;
  privateKey: CryptoPrivateKey;
  capabilityTokenB64: string;
  sessionId: string;
  protocolVersion?: number;
}): Promise<string> {
  const v = args.protocolVersion ?? 1;
  const ts = new Date().toISOString();
  const nonceBytes = new Uint8Array(16);
  crypto.getRandomValues(nonceBytes);
  const nonce = _b64u.encode(nonceBytes);
  const path = `/ws/v1/pty/${args.sessionId}`;
  const emptyHash = await bodyHashHex(new Uint8Array(0));
  const canonical = canonicalBytes({
    method: "GET",
    path,
    query: "",
    ts,
    nonce,
    bodySha256Hex: emptyHash,
    deviceId: args.deviceId,
    protocolVersion: v,
  });
  const sig = await edSign(args.privateKey, canonical);
  const bundle = {
    v,
    device_id: args.deviceId,
    ts,
    nonce,
    sig: _b64u.encode(sig),
    capability_token: args.capabilityTokenB64,
  };
  return _b64u.encode(new TextEncoder().encode(JSON.stringify(bundle)));
}

type OutputHandler = (bytes: Uint8Array) => void;
type CloseHandler = (info: { code: number; reason: string }) => void;

/**
 * Open the PTY WebSocket, run the per-frame signed-bridge protocol, and
 * return a handle exposing input/output/heartbeat/close.
 */
export async function connectPty(opts: ConnectOptions): Promise<PtyConnection> {
  const pingIntervalMs = opts.pingIntervalMs ?? 15_000;
  const privateKey = await importPrivateKeyJwk(opts.pairing.privateKeyJwk);
  const hostPubKey: CryptoPublicKey = await importPublicKeyRaw(
    opts.pairing.hostPubkey,
  );

  const ct = await buildConnectToken({
    deviceId: opts.pairing.deviceId,
    privateKey,
    capabilityTokenB64: opts.pairing.capabilityTokenB64,
    sessionId: opts.sessionId,
  });

  const wsBase = opts.pairing.hostUrl.replace(/^http/i, (m) =>
    m.toLowerCase() === "https" ? "wss" : "ws",
  );
  const url = `${wsBase}/ws/v1/pty/${opts.sessionId}?ct=${ct}`;

  const dbg = opts.onDebug ?? ((_: string) => {});
  dbg(`open ws ${wsBase}/ws/v1/pty/${opts.sessionId}`);

  const ws = new WebSocket(url);

  let outboundSeq = 0;
  let lastInboundSeq = -1;
  const outputHandlers = new Set<OutputHandler>();
  const controlHandlers = new Set<(payload: unknown) => void>();
  const closeHandlers = new Set<CloseHandler>();
  let closed = false;
  let pingTimer: ReturnType<typeof setInterval> | undefined;
  let inboundCount = 0;

  function fireClose(code: number, reason: string): void {
    if (closed) return;
    closed = true;
    if (pingTimer !== undefined) {
      clearInterval(pingTimer);
      pingTimer = undefined;
    }
    for (const h of closeHandlers) {
      try {
        h({ code, reason });
      } catch {
        /* swallow */
      }
    }
  }

  ws.addEventListener("open", () => {
    dbg("ws open");
  });
  ws.addEventListener("close", (ev) => {
    const e = ev as CloseEvent;
    dbg(`ws close code=${e.code} reason=${e.reason || "(none)"}`);
    fireClose(e.code, e.reason);
  });
  ws.addEventListener("error", () => {
    dbg("ws error event");
    fireClose(1006, "ws_error");
  });

  ws.addEventListener("message", (ev) => {
    const data = (ev as MessageEvent).data;
    if (typeof data !== "string") {
      dbg("msg: binary_unsupported");
      ws.close(4400, "binary_unsupported");
      fireClose(4400, "binary_unsupported");
      return;
    }
    let frame: Frame;
    try {
      frame = decodeFrame(data);
    } catch {
      dbg(`msg: bad_frame ${data.slice(0, 80)}`);
      ws.close(4400, "bad_frame");
      fireClose(4400, "bad_frame");
      return;
    }
    inboundCount += 1;
    dbg(`msg #${inboundCount} kind=${frame.kind} seq=${frame.seq} payload=${frame.payload.length}B`);
    if (frame.seq <= lastInboundSeq) {
      dbg(`msg: seq_regression seq=${frame.seq} <= last=${lastInboundSeq}`);
      ws.close(4401, "seq_regression");
      fireClose(4401, "seq_regression");
      return;
    }
    const canonical = frameCanonicalBytes(frame);
    edVerify(hostPubKey, canonical, frame.sig)
      .then((ok) => {
        if (closed) return;
        if (!ok) {
          dbg(`msg: signature_invalid seq=${frame.seq}`);
          ws.close(4401, "signature_invalid");
          fireClose(4401, "signature_invalid");
          return;
        }
        lastInboundSeq = frame.seq;
        if (frame.kind === "output") {
          for (const h of outputHandlers) {
            try {
              h(frame.payload);
            } catch {
              /* swallow */
            }
          }
        } else if (frame.kind === "control") {
          let parsed: unknown;
          try {
            parsed = JSON.parse(new TextDecoder().decode(frame.payload));
          } catch {
            dbg("msg: control_payload_not_json");
            return;
          }
          dbg(`control payload: ${JSON.stringify(parsed)}`);
          for (const h of controlHandlers) {
            try {
              h(parsed);
            } catch {
              /* swallow */
            }
          }
        }
        // pong: no-op at this layer.
      })
      .catch((err) => {
        if (closed) return;
        dbg(`msg: verify_threw ${err}`);
        ws.close(4401, "signature_invalid");
        fireClose(4401, "signature_invalid");
      });
  });

  function waitOpen(): Promise<void> {
    return new Promise((resolve, reject) => {
      if (ws.readyState === WebSocket.OPEN) {
        dbg("waitOpen: already OPEN");
        resolve();
        return;
      }
      dbg(`waitOpen: readyState=${ws.readyState}`);
      // Diagnostic heartbeat for iOS Safari case where WS stays in
      // CONNECTING for 30s+ before transitioning. Logs the readyState and
      // elapsed every 2s while waiting, so users can see whether the
      // browser is making any progress (readyState changes) or genuinely
      // stalled (Tailscale NAT traversal, iOS background throttling, etc).
      const start = Date.now();
      const tick = setInterval(() => {
        const elapsed = Date.now() - start;
        if (ws.readyState === WebSocket.CONNECTING) {
          dbg(`waitOpen: still CONNECTING, elapsed=${elapsed}ms`);
        } else {
          dbg(`waitOpen: readyState=${ws.readyState}, elapsed=${elapsed}ms`);
        }
      }, 2000);
      const cleanup = () => {
        clearInterval(tick);
        ws.removeEventListener("open", onOpen);
        ws.removeEventListener("close", onClose);
        ws.removeEventListener("error", onErr);
      };
      const onOpen = () => {
        cleanup();
        const elapsed = Date.now() - start;
        dbg(`waitOpen: open event fired after ${elapsed}ms`);
        resolve();
      };
      const onClose = (ev: Event) => {
        cleanup();
        const e = ev as CloseEvent;
        const elapsed = Date.now() - start;
        dbg(`waitOpen: closed before open code=${e.code} reason=${e.reason} elapsed=${elapsed}ms`);
        reject(new Error(`ws_closed:${e.code}`));
      };
      const onErr = () => {
        cleanup();
        const elapsed = Date.now() - start;
        dbg(`waitOpen: error before open, elapsed=${elapsed}ms`);
        reject(new Error("ws_error"));
      };
      ws.addEventListener("open", onOpen);
      ws.addEventListener("close", onClose);
      ws.addEventListener("error", onErr);
    });
  }

  await waitOpen();
  dbg("waitOpen: resolved, returning PtyConnection");

  async function sendFrame(kind: FrameKind, payload: Uint8Array): Promise<void> {
    if (closed) throw new Error("connection_closed");
    const seq = outboundSeq++;
    const ts = new Date().toISOString();
    const canonical = frameCanonicalBytes({ v: 1, seq, ts, kind, payload });
    const sig = await edSign(privateKey, canonical);
    const frame: Frame = { v: 1, seq, ts, kind, payload, sig };
    ws.send(encodeFrame(frame));
  }

  pingTimer = setInterval(() => {
    if (closed || ws.readyState !== WebSocket.OPEN) return;
    const nonce = new Uint8Array(8);
    crypto.getRandomValues(nonce);
    void sendFrame("ping", nonce).catch(() => {
      /* timer-driven, swallow */
    });
  }, pingIntervalMs);

  return {
    async sendInput(bytes: Uint8Array): Promise<void> {
      await sendFrame("input", bytes);
    },
    async sendControl(payload: object): Promise<void> {
      const bytes = new TextEncoder().encode(JSON.stringify(payload));
      await sendFrame("control", bytes);
    },
    onOutput(h) {
      outputHandlers.add(h);
      return () => outputHandlers.delete(h);
    },
    onControl(h) {
      controlHandlers.add(h);
      return () => controlHandlers.delete(h);
    },
    onClose(h) {
      closeHandlers.add(h);
      return () => closeHandlers.delete(h);
    },
    async ping(): Promise<void> {
      const nonce = new Uint8Array(8);
      crypto.getRandomValues(nonce);
      await sendFrame("ping", nonce);
    },
    close(): void {
      if (closed) return;
      try {
        ws.close(1000, "client_close");
      } catch {
        /* ignore */
      }
      fireClose(1000, "client_close");
    },
  };
}
