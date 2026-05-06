// Phase 5a — vitest unit tests for WarpSessionBashOperations.
//
// Tests drive createWarpSessionBashOperations in isolation via a stub
// RpcBridge. The real omw-server broker and the pane PTY are not exercised
// here; those paths are covered by the Rust integration tests in
// crates/omw-server/tests/agent_bash.rs (deferred to follow-up).
//
// File-boundary note: tests in this file are owned by the Test Overseer
// under the TRD protocol. Implementation lives in src/warp-session-bash.ts.

import { describe, expect, it, vi } from "vitest";
import {
    createWarpSessionBashOperations,
    type RpcBridge,
} from "../src/warp-session-bash.js";

/** Build a stub RpcBridge and helpers for driving it from tests. */
function makeRpc() {
    type Subscriber = (frame: { method: string; params: any }) => void;
    const subscribers = new Map<string, Subscriber>();
    const sent: Array<{ method: string; params: any }> = [];

    const rpc: RpcBridge = {
        notify: vi.fn((method: string, params: any) => {
            sent.push({ method, params });
        }),
        registerCommandSubscriber: vi.fn((id: string, sub: Subscriber) => {
            subscribers.set(id, sub);
        }),
        unregisterCommandSubscriber: vi.fn((id: string) => {
            subscribers.delete(id);
        }),
    };

    const dispatch = (commandId: string, frame: { method: string; params: any }): void => {
        subscribers.get(commandId)?.(frame);
    };

    /** Returns the commandId from the most recently emitted bash/exec notification. */
    const lastCommandId = (): string => {
        const exec = [...sent].reverse().find((f) => f.method === "bash/exec");
        if (!exec) throw new Error("no bash/exec notification found");
        return exec.params.commandId as string;
    };

    return { rpc, sent, dispatch, lastCommandId };
}

function makeDeps(rpc: RpcBridge) {
    return {
        rpc,
        terminalSessionId: "term-1",
        agentSessionId: "sess-1",
        toolCallId: "tc-1",
    };
}

describe("WarpSessionBashOperations", () => {
    it("emits bash/exec notification with commandId on exec", async () => {
        const { rpc, sent } = makeRpc();
        const ops = createWarpSessionBashOperations(makeDeps(rpc));

        // Don't await — exec resolves only on bash/finished, never sent here.
        void ops.exec("ls", "/tmp", { timeout: 5_000 });
        // Allow microtask queue to flush so registerCommandSubscriber + notify fire.
        await Promise.resolve();

        expect(sent[0].method).toBe("bash/exec");
        expect(sent[0].params.command).toBe("ls");
        expect(sent[0].params.cwd).toBe("/tmp");
        expect(sent[0].params.terminalSessionId).toBe("term-1");
        expect(typeof sent[0].params.commandId).toBe("string");
        expect(rpc.registerCommandSubscriber).toHaveBeenCalled();
    });

    it("resolves with exitCode on bash/finished", async () => {
        const { rpc, dispatch, lastCommandId } = makeRpc();
        const ops = createWarpSessionBashOperations(makeDeps(rpc));

        const promise = ops.exec("ls", "/tmp", { timeout: 5_000 });
        await Promise.resolve();

        const cmdId = lastCommandId();
        dispatch(cmdId, { method: "bash/finished", params: { exitCode: 0 } });

        const result = await promise;
        expect(result.exitCode).toBe(0);
        expect(result.snapshot).toBe(false);
    });

    it("invokes onData for each bash/data event", async () => {
        const { rpc, dispatch, lastCommandId } = makeRpc();
        const ops = createWarpSessionBashOperations(makeDeps(rpc));

        const onData = vi.fn();
        const promise = ops.exec("cat /dev/stdin", "/tmp", {
            timeout: 5_000,
            onData,
        });
        await Promise.resolve();

        const cmdId = lastCommandId();
        dispatch(cmdId, { method: "bash/data", params: { bytes: "chunk-1" } });
        dispatch(cmdId, { method: "bash/data", params: { bytes: "chunk-2" } });
        dispatch(cmdId, { method: "bash/finished", params: { exitCode: 0 } });

        await promise;
        expect(onData).toHaveBeenCalledTimes(2);
        expect(onData).toHaveBeenNthCalledWith(1, "chunk-1");
        expect(onData).toHaveBeenNthCalledWith(2, "chunk-2");
    });

    it("resolves with snapshot:true on timeout", async () => {
        const { rpc, sent } = makeRpc();
        const ops = createWarpSessionBashOperations(makeDeps(rpc));

        const result = await ops.exec("sleep 60", "/tmp", { timeout: 10 });

        expect(result.exitCode).toBeNull();
        expect(result.snapshot).toBe(true);
        expect(sent.some((f) => f.method === "bash/cancel")).toBe(true);
    });

    it("emits bash/cancel on AbortSignal abort", async () => {
        const { rpc, sent } = makeRpc();
        const ops = createWarpSessionBashOperations(makeDeps(rpc));

        const ctrl = new AbortController();
        const promise = ops.exec("sleep 60", "/tmp", {
            timeout: 60_000,
            signal: ctrl.signal,
        });
        await Promise.resolve();
        ctrl.abort();

        const result = await promise;
        expect(result.exitCode).toBeNull();
        expect(result.snapshot).toBe(true);
        expect(sent.some((f) => f.method === "bash/cancel")).toBe(true);
    });
});
