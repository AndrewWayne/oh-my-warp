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
