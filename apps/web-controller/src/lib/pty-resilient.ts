// Resilience wrapper around `connectPty` for the iOS Safari WebSocket
// lifecycle (issue #22 / B2). iOS Safari kills a backgrounded page's
// WebSocket within seconds, with no reconnect. This wrapper:
//
//   - proactively closes the socket on `visibilitychange -> hidden`
//     (clean teardown instead of a zombie iOS is about to kill anyway),
//   - eagerly reconnects on `-> visible` with a FRESH connect-token
//     (each `connectPty` call mints a new ts/nonce/sig; the capability
//     token itself is reused, so no re-pair — see byorc-protocol §3),
//   - forwards output/control handlers across reconnects so the caller
//     registers them once, and
//   - exposes a connection-state signal for a "Reconnecting…" indicator.
//
// Auto-reconnect is scoped to the background/foreground cycle only, and
// only AFTER a first successful connect. An initial connect failure, or an
// unexpected close while foregrounded, is surfaced via `onClose` so the
// caller's existing error / dead-session / Retry handling still runs
// (avoids reconnect loops against a dead session).

import { connectPty, type ConnectOptions, type PtyConnection } from "./pty-ws";

export type ConnState = "connecting" | "connected" | "reconnecting" | "closed";

export interface ResilientPtyConnection {
  sendInput(bytes: Uint8Array): Promise<void>;
  sendControl(payload: object): Promise<void>;
  onOutput(handler: (bytes: Uint8Array) => void): () => void;
  onControl(handler: (payload: unknown) => void): () => void;
  /** Subscribe to connection-state changes (drives the "Reconnecting…" UI). */
  onState(handler: (state: ConnState) => void): () => void;
  /**
   * Fires on a terminal close — initial connect failure or an unexpected
   * foreground close — NOT on a transient background/foreground reconnect.
   */
  onClose(handler: (info: { code: number; reason: string }) => void): () => void;
  /** Current state, read synchronously. */
  state(): ConnState;
  close(): void;
}

type VisibilitySource = Pick<
  Document,
  "visibilityState" | "addEventListener" | "removeEventListener"
>;

export interface ResilientDeps {
  /** Injectable for tests. Defaults to `connectPty`. */
  connect?: (opts: ConnectOptions) => Promise<PtyConnection>;
  /**
   * Injectable for tests. Defaults to the global `document`; pass `null`
   * to disable visibility-driven reconnect entirely.
   */
  visibility?: VisibilitySource | null;
}

export function connectPtyResilient(
  opts: ConnectOptions,
  deps: ResilientDeps = {},
): ResilientPtyConnection {
  const connect = deps.connect ?? connectPty;
  const vis: VisibilitySource | null =
    deps.visibility !== undefined
      ? deps.visibility
      : typeof document !== "undefined"
        ? document
        : null;

  const outputHandlers = new Set<(b: Uint8Array) => void>();
  const controlHandlers = new Set<(p: unknown) => void>();
  const stateHandlers = new Set<(s: ConnState) => void>();
  const closeHandlers = new Set<(info: { code: number; reason: string }) => void>();

  let current: PtyConnection | null = null;
  let curState: ConnState = "connecting";
  let everConnected = false;
  let explicitlyClosed = false;
  let backgrounding = false; // true while WE close for a hidden transition
  let opening = false; // guards against concurrent opens

  function setState(s: ConnState): void {
    if (curState === s) return;
    curState = s;
    for (const h of stateHandlers) {
      try {
        h(s);
      } catch {
        /* swallow */
      }
    }
  }

  function fireClose(info: { code: number; reason: string }): void {
    for (const h of closeHandlers) {
      try {
        h(info);
      } catch {
        /* swallow */
      }
    }
  }

  async function open(): Promise<void> {
    if (explicitlyClosed || opening) return;
    opening = true;
    setState(everConnected ? "reconnecting" : "connecting");
    try {
      const conn = await connect(opts);
      if (explicitlyClosed) {
        conn.close();
        return;
      }
      conn.onOutput((b) => {
        for (const h of outputHandlers) {
          try {
            h(b);
          } catch {
            /* swallow */
          }
        }
      });
      conn.onControl((p) => {
        for (const h of controlHandlers) {
          try {
            h(p);
          } catch {
            /* swallow */
          }
        }
      });
      conn.onClose((info) => onInnerClose(info));
      current = conn;
      everConnected = true;
      setState("connected");
    } catch {
      current = null;
      if (everConnected) {
        // Reconnect attempt failed; a later `-> visible` retries.
        setState("reconnecting");
      } else {
        // Initial connect failed — hand off to the caller's error/Retry UI.
        fireClose({ code: 1006, reason: "connect_failed" });
      }
    } finally {
      opening = false;
    }
  }

  function onInnerClose(info: { code: number; reason: string }): void {
    current = null;
    if (explicitlyClosed) return;
    if (backgrounding) {
      // Expected teardown for a hidden transition; wait for `-> visible`.
      backgrounding = false;
      setState("reconnecting");
      return;
    }
    if (vis && vis.visibilityState === "hidden") {
      // iOS killed it before our own visibility handler ran; wait for visible.
      setState("reconnecting");
      return;
    }
    // Foreground, unexpected close: hand off to the caller (dead-session / Retry).
    fireClose(info);
  }

  function onVisibility(): void {
    if (explicitlyClosed || !vis) return;
    if (vis.visibilityState === "hidden") {
      if (current) {
        backgrounding = true;
        const c = current;
        current = null;
        setState("reconnecting");
        try {
          c.close();
        } catch {
          /* ignore */
        }
      }
    } else if (curState !== "connected") {
      void open();
    }
  }

  if (vis) vis.addEventListener("visibilitychange", onVisibility);
  void open();

  return {
    async sendInput(bytes: Uint8Array): Promise<void> {
      if (!current) throw new Error("not_connected");
      await current.sendInput(bytes);
    },
    async sendControl(payload: object): Promise<void> {
      if (!current) throw new Error("not_connected");
      await current.sendControl(payload);
    },
    onOutput(h) {
      outputHandlers.add(h);
      return () => outputHandlers.delete(h);
    },
    onControl(h) {
      controlHandlers.add(h);
      return () => controlHandlers.delete(h);
    },
    onState(h) {
      stateHandlers.add(h);
      return () => stateHandlers.delete(h);
    },
    onClose(h) {
      closeHandlers.add(h);
      return () => closeHandlers.delete(h);
    },
    state() {
      return curState;
    },
    close() {
      if (explicitlyClosed) return;
      explicitlyClosed = true;
      if (vis) vis.removeEventListener("visibilitychange", onVisibility);
      const c = current;
      current = null;
      if (c) {
        try {
          c.close();
        } catch {
          /* ignore */
        }
      }
      setState("closed");
    },
  };
}
