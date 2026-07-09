import { describe, it, expect, vi } from "vitest";
import {
  connectPtyResilient,
  type ConnState,
  type ResilientDeps,
} from "../src/lib/pty-resilient";
import type { ConnectOptions, PtyConnection } from "../src/lib/pty-ws";

const OPTS = { sessionId: "s1" } as unknown as ConnectOptions;
const flush = () => new Promise((r) => setTimeout(r, 0));

/** A mock PtyConnection whose inbound events can be driven from the test. */
function makeMockConn() {
  let outputH: ((b: Uint8Array) => void) | null = null;
  let controlH: ((p: unknown) => void) | null = null;
  let closeH: ((info: { code: number; reason: string }) => void) | null = null;
  const conn: PtyConnection = {
    sendInput: vi.fn().mockResolvedValue(undefined),
    sendControl: vi.fn().mockResolvedValue(undefined),
    onOutput: (h) => {
      outputH = h;
      return () => undefined;
    },
    onControl: (h) => {
      controlH = h;
      return () => undefined;
    },
    onClose: (h) => {
      closeH = h;
      return () => undefined;
    },
    ping: vi.fn().mockResolvedValue(undefined),
    close: vi.fn(),
  };
  return {
    conn,
    emitOutput: (b: Uint8Array) => outputH?.(b),
    emitControl: (p: unknown) => controlH?.(p),
    emitClose: (info: { code: number; reason: string }) => closeH?.(info),
  };
}

/** A fake visibility source the test can flip and dispatch. */
function makeVisibility(initial: "visible" | "hidden" = "visible") {
  let vstate: "visible" | "hidden" = initial;
  const handlers = new Set<() => void>();
  const vis = {
    get visibilityState() {
      return vstate;
    },
    addEventListener: (_type: string, fn: EventListenerOrEventListenerObject) => {
      handlers.add(fn as () => void);
    },
    removeEventListener: (_type: string, fn: EventListenerOrEventListenerObject) => {
      handlers.delete(fn as () => void);
    },
  } as unknown as NonNullable<ResilientDeps["visibility"]>;
  return {
    vis,
    set: (s: "visible" | "hidden") => {
      vstate = s;
    },
    fire: () => {
      for (const h of [...handlers]) h();
    },
    listenerCount: () => handlers.size,
  };
}

/** connect() that hands out queued mocks in order and counts calls. */
function queuedConnect(mocks: Array<ReturnType<typeof makeMockConn>>) {
  const connect = vi.fn(async () => {
    const next = mocks.shift();
    if (!next) throw new Error("no more mock connections queued");
    return next.conn;
  });
  return connect;
}

describe("connectPtyResilient", () => {
  it("connects, transitions connecting -> connected, forwards output", async () => {
    const a = makeMockConn();
    const vis = makeVisibility("visible");
    const states: ConnState[] = [];
    const out: Uint8Array[] = [];

    const conn = connectPtyResilient(OPTS, {
      connect: queuedConnect([a]),
      visibility: vis.vis,
    });
    conn.onState((s) => states.push(s));
    conn.onOutput((b) => out.push(b));
    await flush();

    expect(conn.state()).toBe("connected");
    expect(states).toContain("connected");
    a.emitOutput(new Uint8Array([1, 2, 3]));
    expect(out).toEqual([new Uint8Array([1, 2, 3])]);
  });

  it("closes on hidden and reconnects on visible with a fresh connect() call", async () => {
    const a = makeMockConn();
    const b = makeMockConn();
    const vis = makeVisibility("visible");
    const connect = queuedConnect([a, b]);

    const conn = connectPtyResilient(OPTS, { connect, visibility: vis.vis });
    const out: Uint8Array[] = [];
    conn.onOutput((x) => out.push(x));
    await flush();
    expect(connect).toHaveBeenCalledTimes(1);
    expect(conn.state()).toBe("connected");

    // Background: socket is torn down, state goes reconnecting.
    vis.set("hidden");
    vis.fire();
    expect(a.conn.close).toHaveBeenCalled();
    expect(conn.state()).toBe("reconnecting");

    // Foreground: a fresh connect() (fresh connect-token) is issued.
    vis.set("visible");
    vis.fire();
    await flush();
    expect(connect).toHaveBeenCalledTimes(2);
    expect(conn.state()).toBe("connected");

    // Output from the NEW socket reaches the same handler registered once.
    b.emitOutput(new Uint8Array([9]));
    expect(out).toEqual([new Uint8Array([9])]);
  });

  it("surfaces an initial connect failure via onClose", async () => {
    const vis = makeVisibility("visible");
    const closed: Array<{ code: number; reason: string }> = [];
    const conn = connectPtyResilient(OPTS, {
      connect: vi.fn().mockRejectedValue(new Error("boom")),
      visibility: vis.vis,
    });
    conn.onClose((info) => closed.push(info));
    await flush();
    expect(closed).toHaveLength(1);
    expect(closed[0].reason).toBe("connect_failed");
  });

  it("surfaces an unexpected foreground close via onClose and does not auto-reconnect", async () => {
    const a = makeMockConn();
    const vis = makeVisibility("visible");
    const connect = queuedConnect([a]);
    const conn = connectPtyResilient(OPTS, { connect, visibility: vis.vis });
    const closed: Array<{ code: number; reason: string }> = [];
    conn.onClose((info) => closed.push(info));
    await flush();

    a.emitClose({ code: 1006, reason: "server_gone" });
    await flush();
    expect(closed).toEqual([{ code: 1006, reason: "server_gone" }]);
    expect(connect).toHaveBeenCalledTimes(1); // no reconnect attempt
  });

  it("explicit close detaches the visibility listener and prevents reconnects", async () => {
    const a = makeMockConn();
    const vis = makeVisibility("visible");
    const connect = queuedConnect([a, makeMockConn()]);
    const conn = connectPtyResilient(OPTS, { connect, visibility: vis.vis });
    await flush();
    expect(vis.listenerCount()).toBe(1);

    conn.close();
    expect(a.conn.close).toHaveBeenCalled();
    expect(conn.state()).toBe("closed");
    expect(vis.listenerCount()).toBe(0);

    // Later visibility churn must not reconnect.
    vis.set("hidden");
    vis.fire();
    vis.set("visible");
    vis.fire();
    await flush();
    expect(connect).toHaveBeenCalledTimes(1);
  });

  it("sendInput forwards to the live socket and rejects while reconnecting", async () => {
    const a = makeMockConn();
    const vis = makeVisibility("visible");
    const conn = connectPtyResilient(OPTS, {
      connect: queuedConnect([a, makeMockConn()]),
      visibility: vis.vis,
    });
    await flush();

    await conn.sendInput(new Uint8Array([7]));
    expect(a.conn.sendInput).toHaveBeenCalledWith(new Uint8Array([7]));

    vis.set("hidden");
    vis.fire();
    await expect(conn.sendInput(new Uint8Array([8]))).rejects.toThrow("not_connected");
  });
});
