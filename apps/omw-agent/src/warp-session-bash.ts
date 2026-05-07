// warp-session-bash.ts — BashOperations implementation for the omw-agent
// stdio kernel. Bridges the pi-agent tool interface to omw-server's bash
// broker over the JSON-RPC 2.0 notification channel (Pattern B per Phase 5a
// progress doc D).
//
// On exec():
//   1. Allocates a unique commandId.
//   2. Registers a per-commandId subscriber on the RPC bridge.
//   3. Emits a bash/exec notification toward the broker (omw-server).
//   4. Awaits bash/finished, invoking opts.onData on each bash/data event.
//   5. AbortSignal / timeout triggers bash/cancel and resolves with snapshot.
//
// `createBashTool` wraps these operations as an AgentTool consumable by the
// pi-agent loop. The tool's `execute` constructs a per-call BashOperations
// so the runtime toolCallId rides along with bash/exec params for audit.

import { Type, type Static } from "typebox";

import type { AgentTool } from "../vendor/pi-agent-core/index.js";

/** Result returned by exec(). */
export interface ExecResult {
    /** Process exit code, or null when the result is a snapshot (timeout / cancel). */
    exitCode: number | null;
    /** True when the result is synthetic — timeout or signal-driven cancellation. */
    snapshot: boolean;
}

/** Options accepted by exec(). */
export interface ExecOptions {
    /** Called with each raw byte chunk as it arrives from the PTY. */
    onData?: (chunk: string) => void;
    /** AbortSignal — abort triggers bash/cancel and resolves with snapshot. */
    signal?: AbortSignal;
    /** Timeout in milliseconds. When elapsed, triggers bash/cancel + snapshot. */
    timeout?: number;
}

/** Minimal bash execution interface used by the agent tool layer. */
export interface BashOperations {
    exec(command: string, cwd: string, opts?: ExecOptions): Promise<ExecResult>;
}

/**
 * Bidirectional bridge to the JSON-RPC stdio channel.
 *
 * `notify` fires a server-bound notification. `registerCommandSubscriber` /
 * `unregisterCommandSubscriber` install per-commandId frame listeners that
 * the serve.ts dispatcher invokes whenever a matching `bash/data`,
 * `bash/finished`, or `bash/cancel` notification arrives from the broker.
 */
export interface RpcBridge {
    notify(method: string, params: Record<string, unknown>): void;
    registerCommandSubscriber(
        commandId: string,
        subscriber: (frame: { method: string; params: any }) => void,
    ): void;
    unregisterCommandSubscriber(commandId: string): void;
}

export interface WarpSessionBashDeps {
    rpc: RpcBridge;
    terminalSessionId: string;
    agentSessionId: string;
    toolCallId: string;
}

/**
 * Create a BashOperations implementation backed by the omw-server bash broker.
 *
 * Each `exec` call:
 *   - Allocates a unique `commandId`.
 *   - Emits `bash/exec` with the command, cwd, and session identifiers.
 *   - Resolves on `bash/finished` with the broker-reported exit code.
 *   - Streams PTY chunks to `opts.onData` via `bash/data` events.
 *   - Sends `bash/cancel` and resolves with `snapshot: true` on timeout or signal.
 */
export function createWarpSessionBashOperations(
    deps: WarpSessionBashDeps,
): BashOperations {
    return {
        async exec(
            command: string,
            cwd: string,
            opts: ExecOptions = {},
        ): Promise<ExecResult> {
            const commandId = `cmd-${Math.random().toString(36).slice(2)}`;
            return new Promise<ExecResult>((resolve) => {
                let timer: ReturnType<typeof setTimeout> | null = null;
                let resolved = false;
                let abortHandler: (() => void) | null = null;

                const finish = (result: ExecResult): void => {
                    if (resolved) return;
                    resolved = true;
                    if (timer !== null) clearTimeout(timer);
                    deps.rpc.unregisterCommandSubscriber(commandId);
                    if (opts.signal && abortHandler) {
                        opts.signal.removeEventListener("abort", abortHandler);
                    }
                    resolve(result);
                };

                deps.rpc.registerCommandSubscriber(commandId, (frame) => {
                    if (frame.method === "bash/data") {
                        const bytes = (frame.params?.bytes as string) ?? "";
                        opts.onData?.(bytes);
                    } else if (frame.method === "bash/finished") {
                        const exitCode =
                            typeof frame.params?.exitCode === "number"
                                ? (frame.params.exitCode as number)
                                : null;
                        const snapshot = frame.params?.snapshot === true;
                        finish({ exitCode, snapshot });
                    }
                });

                deps.rpc.notify("bash/exec", {
                    commandId,
                    command,
                    cwd,
                    terminalSessionId: deps.terminalSessionId,
                    agentSessionId: deps.agentSessionId,
                    toolCallId: deps.toolCallId,
                });

                if (opts.signal) {
                    abortHandler = (): void => {
                        deps.rpc.notify("bash/cancel", { commandId });
                        finish({ exitCode: null, snapshot: true });
                    };
                    if (opts.signal.aborted) {
                        abortHandler();
                        return;
                    }
                    opts.signal.addEventListener("abort", abortHandler);
                }

                if (opts.timeout !== undefined && opts.timeout > 0) {
                    timer = setTimeout(() => {
                        deps.rpc.notify("bash/cancel", { commandId });
                        finish({ exitCode: null, snapshot: true });
                    }, opts.timeout);
                }
            });
        },
    };
}

/** Schema for the `bash` tool's parameters. Kept minimal in v0.4 — the
 *  full pi-coding-agent surface is intentionally not vendored. */
const BashParametersSchema = Type.Object({
    command: Type.String({ description: "Shell command to execute." }),
    cwd: Type.Optional(
        Type.String({ description: "Working directory; defaults to session cwd." }),
    ),
    timeout: Type.Optional(
        Type.Number({ description: "Timeout in milliseconds; default 30000." }),
    ),
});

export interface CreateBashToolDeps {
    rpc: RpcBridge;
    terminalSessionId: string;
    agentSessionId: string;
    /** Default cwd if the model omits it. */
    defaultCwd?: string;
}

/**
 * Build the `bash` AgentTool. Each invocation constructs a fresh
 * BashOperations carrying the runtime toolCallId so bash/exec params are
 * traceable in the audit log.
 */
export function createBashTool(
    deps: CreateBashToolDeps,
): AgentTool<typeof BashParametersSchema, { exitCode: number | null; snapshot: boolean }> {
    return {
        name: "bash",
        label: "Bash",
        description:
            "Execute a shell command in the user's active terminal pane and return its output.",
        parameters: BashParametersSchema,
        execute: async (
            toolCallId: string,
            params: Static<typeof BashParametersSchema>,
            signal?: AbortSignal,
        ) => {
            const ops = createWarpSessionBashOperations({
                rpc: deps.rpc,
                terminalSessionId: deps.terminalSessionId,
                agentSessionId: deps.agentSessionId,
                toolCallId,
            });
            const cwd = params.cwd ?? deps.defaultCwd ?? "";
            const timeout = typeof params.timeout === "number" ? params.timeout : 30_000;
            const captured: string[] = [];
            const result = await ops.exec(params.command, cwd, {
                timeout,
                signal,
                onData: (chunk) => captured.push(chunk),
            });
            const stdout = captured.join("");
            return {
                content: [
                    {
                        type: "text",
                        text:
                            stdout +
                            (result.snapshot
                                ? "\n[snapshot — process did not finish in time]"
                                : `\n[exit ${result.exitCode ?? "unknown"}]`),
                    },
                ],
                details: result,
            };
        },
    };
}
