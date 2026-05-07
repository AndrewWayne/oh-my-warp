// Phase 1 protocol round-trip test for `runStdioServer`.
//
// Spins up:
//   1. A mock OpenAI-compatible HTTP server on a random localhost port that
//      replies with a fixed SSE chunk sequence on POST /chat/completions.
//   2. `runStdioServer` driven by in-memory PassThrough streams (no child
//      process — vitest doesn't have access to the compiled dist/ here, and
//      the protocol surface we care about is identical).
//
// Asserts the canonical happy path:
//   - session/create returns a sessionId
//   - session/prompt acknowledges synchronously
//   - assistant/delta notifications stream in
//   - turn/finished arrives with cancelled: false
//
// File-boundary note: tests in this file are owned by the Test Overseer
// under the TRD protocol. Implementation lives in src/serve.ts and
// src/session.ts.

import { createServer, type Server } from "node:http";
import { AddressInfo } from "node:net";
import { PassThrough } from "node:stream";
import { setTimeout as delay } from "node:timers/promises";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { runStdioServer } from "../src/serve.js";

interface JsonRpcFrame {
	jsonrpc: "2.0";
	id?: string | number | null;
	method?: string;
	params?: unknown;
	result?: unknown;
	error?: { code: number; message: string };
}

interface MockServer {
	server: Server;
	url: string;
	close: () => Promise<void>;
}

// Builds a minimal OpenAI-compatible /chat/completions endpoint that always
// returns the same SSE chunks. `chunks` is interleaved with `delayMs` so
// cancel tests can race a cancel against the stream.
function startMockProvider(opts: {
	chunks: string[];
	delayBetweenMs?: number;
}): Promise<MockServer> {
	return new Promise((resolve) => {
		const server = createServer((req, res) => {
			if (req.url !== "/chat/completions" || req.method !== "POST") {
				res.statusCode = 404;
				res.end();
				return;
			}
			// Drain request body — the provider doesn't care about content
			// but the SDK won't finish writing until the request is consumed.
			req.on("data", () => undefined);
			req.on("end", async () => {
				res.statusCode = 200;
				res.setHeader("Content-Type", "text/event-stream");
				res.setHeader("Cache-Control", "no-cache");
				res.setHeader("Connection", "keep-alive");
				try {
					for (const chunk of opts.chunks) {
						res.write(`data: ${chunk}\n\n`);
						if (opts.delayBetweenMs && opts.delayBetweenMs > 0) {
							await delay(opts.delayBetweenMs);
						}
					}
					res.write("data: [DONE]\n\n");
					res.end();
				} catch {
					// Connection torn down by client (cancel test). Best-effort.
					try {
						res.end();
					} catch {
						// already closed
					}
				}
			});
		});
		server.listen(0, "127.0.0.1", () => {
			const addr = server.address() as AddressInfo;
			resolve({
				server,
				url: `http://127.0.0.1:${addr.port}`,
				close: () => new Promise((r) => server.close(() => r())),
			});
		});
	});
}

// Make a single OpenAI SSE chunk (data: <chunk>) with one delta-content piece.
function deltaChunk(content: string, finishReason?: string): string {
	return JSON.stringify({
		id: "chatcmpl-test",
		object: "chat.completion.chunk",
		created: 1700000000,
		model: "test-model",
		choices: [
			{
				index: 0,
				delta: finishReason ? {} : { content },
				finish_reason: finishReason ?? null,
			},
		],
	});
}

interface RunningServer {
	stdin: PassThrough;
	stdout: PassThrough;
	stderr: PassThrough;
	frames: JsonRpcFrame[];
	pendingFrame: () => Promise<JsonRpcFrame>;
	pendingNotification: (method: string) => Promise<JsonRpcFrame>;
	stop: () => Promise<void>;
}

// Boots `runStdioServer` against in-memory streams and returns helpers for
// driving and reading the protocol. The returned `pendingFrame` /
// `pendingNotification` block until the matching frame arrives so tests
// don't race ordering.
function startServer(opts: {
	getApiKey: (keyRef: string) => Promise<string | undefined>;
}): RunningServer {
	const stdin = new PassThrough();
	const stdout = new PassThrough();
	const stderr = new PassThrough();
	const frames: JsonRpcFrame[] = [];
	const waiters: Array<{
		match: (f: JsonRpcFrame) => boolean;
		resolve: (f: JsonRpcFrame) => void;
	}> = [];

	let buf = "";
	stdout.on("data", (chunk: Buffer) => {
		buf += chunk.toString("utf8");
		let nl: number;
		while ((nl = buf.indexOf("\n")) >= 0) {
			const line = buf.slice(0, nl);
			buf = buf.slice(nl + 1);
			if (!line.trim()) continue;
			let frame: JsonRpcFrame;
			try {
				frame = JSON.parse(line) as JsonRpcFrame;
			} catch {
				continue;
			}
			// Hand the frame to the first matching waiter, or queue it.
			// Never both — otherwise a subsequent pendingFrame() would
			// re-return a frame already delivered.
			const idx = waiters.findIndex((w) => w.match(frame));
			if (idx >= 0) {
				waiters.splice(idx, 1)[0].resolve(frame);
			} else {
				frames.push(frame);
			}
		}
	});

	const pendingFrame = (): Promise<JsonRpcFrame> => {
		if (frames.length > 0) return Promise.resolve(frames.shift()!);
		return new Promise((resolve) => waiters.push({ match: () => true, resolve }));
	};

	const pendingNotification = (method: string): Promise<JsonRpcFrame> => {
		const idx = frames.findIndex((f) => f.method === method);
		if (idx >= 0) return Promise.resolve(frames.splice(idx, 1)[0]);
		return new Promise((resolve) =>
			waiters.push({ match: (f) => f.method === method, resolve }),
		);
	};

	// runStdioServer returns when stdin closes; we don't await it from the
	// test (we close stdin in stop()).
	const serverDone = runStdioServer({ stdin, stdout, stderr, getApiKey: opts.getApiKey });

	const stop = async (): Promise<void> => {
		stdin.end();
		await serverDone;
	};

	return { stdin, stdout, stderr, frames, pendingFrame, pendingNotification, stop };
}

function send(stdin: PassThrough, req: JsonRpcFrame): void {
	stdin.write(`${JSON.stringify(req)}\n`);
}

describe("runStdioServer protocol round-trip", () => {
	let mock: MockServer | null = null;

	beforeEach(() => {
		mock = null;
	});

	afterEach(async () => {
		if (mock) await mock.close();
	});

	it("session/create + session/prompt + assistant deltas + turn/finished", async () => {
		mock = await startMockProvider({
			chunks: [
				deltaChunk("Hello"),
				deltaChunk(" world"),
				deltaChunk("", "stop"),
			],
		});

		const server = startServer({
			getApiKey: async () => "sk-test",
		});
		try {
			send(server.stdin, {
				jsonrpc: "2.0",
				id: 1,
				method: "session/create",
				params: {
					providerConfig: {
						kind: "openai-compatible",
						key_ref: "omw/test",
						base_url: mock.url,
					},
					model: "test-model",
				},
			});
			const createReply = await server.pendingFrame();
			expect(createReply.id).toBe(1);
			const sessionId = (createReply.result as { sessionId: string }).sessionId;
			expect(typeof sessionId).toBe("string");
			expect(sessionId.length).toBeGreaterThan(0);

			send(server.stdin, {
				jsonrpc: "2.0",
				id: 2,
				method: "session/prompt",
				params: { sessionId, prompt: "say hi" },
			});
			const promptAck = await server.pendingFrame();
			expect(promptAck.id).toBe(2);
			expect(promptAck.result).toEqual({ ok: true });

			// Collect all deltas until turn/finished.
			const deltas: string[] = [];
			let turnFinished: JsonRpcFrame | null = null;
			while (turnFinished === null) {
				const frame = await server.pendingFrame();
				if (frame.method === "assistant/delta") {
					deltas.push((frame.params as { delta: string }).delta);
				} else if (frame.method === "turn/finished") {
					turnFinished = frame;
				}
			}
			expect(deltas.length).toBeGreaterThan(0);
			expect(deltas.join("")).toContain("Hello");
			expect(turnFinished.params).toMatchObject({ sessionId, cancelled: false });
		} finally {
			await server.stop();
		}
	}, 15000);

	it("session/create rejects when providerConfig is malformed", async () => {
		const server = startServer({ getApiKey: async () => undefined });
		try {
			send(server.stdin, {
				jsonrpc: "2.0",
				id: 1,
				method: "session/create",
				params: { providerConfig: { kind: "bogus" }, model: "x" },
			});
			const reply = await server.pendingFrame();
			expect(reply.id).toBe(1);
			expect(reply.error).toBeDefined();
			expect(reply.error?.code).toBe(-32602);
		} finally {
			await server.stop();
		}
	});

	it("session/prompt against unknown sessionId returns an error", async () => {
		const server = startServer({ getApiKey: async () => undefined });
		try {
			send(server.stdin, {
				jsonrpc: "2.0",
				id: 7,
				method: "session/prompt",
				params: { sessionId: "no-such-session", prompt: "hi" },
			});
			const reply = await server.pendingFrame();
			expect(reply.id).toBe(7);
			expect(reply.error?.code).toBe(-32602);
			expect(reply.error?.message).toContain("unknown sessionId");
		} finally {
			await server.stop();
		}
	});

	// Regression: when the upstream provider rejects (e.g. OpenAI 401
	// because the keychain entry was empty / not configured), pi-ai
	// folds the error into an AssistantMessage with `stopReason:
	// "error"` and `errorMessage`. The agent loop emits a `message_end`
	// event for that message; `translateEvent` previously dropped it,
	// leaving the WS client with only `turn/finished` and no idea why
	// nothing rendered. The fix surfaces the embedded errorMessage as
	// an `error` notification so the inline-pump can render it.
	it("provider 401 surfaces as an `error` notification (not silent turn/finished)", async () => {
		const provider = await startMockProvider401();
		const server = startServer({ getApiKey: async () => undefined });
		try {
			send(server.stdin, {
				jsonrpc: "2.0",
				id: 1,
				method: "session/create",
				params: {
					providerConfig: {
						kind: "openai-compatible",
						key_ref: "omw/test",
						base_url: provider.url,
					},
					model: "test-model",
				},
			});
			const createReply = await server.pendingFrame();
			expect(createReply.id).toBe(1);
			const sessionId = (createReply.result as { sessionId: string }).sessionId;

			send(server.stdin, {
				jsonrpc: "2.0",
				id: 2,
				method: "session/prompt",
				params: { sessionId, prompt: "say hi" },
			});
			const ack = await server.pendingFrame();
			expect(ack.id).toBe(2);
			expect(ack.result).toEqual({ ok: true });

			// Collect notifications until turn/finished. We expect to
			// see an `error` notification BEFORE turn/finished, with a
			// non-empty message that mentions auth / 401 / unauthorized.
			let errorNotif: JsonRpcFrame | null = null;
			let turnFinished: JsonRpcFrame | null = null;
			while (turnFinished === null) {
				const frame = await server.pendingFrame();
				if (frame.method === "error") {
					errorNotif = frame;
				} else if (frame.method === "turn/finished") {
					turnFinished = frame;
				}
			}
			expect(errorNotif, "expected an error notification before turn/finished").not.toBeNull();
			const errParams = errorNotif!.params as { sessionId: string; message: string };
			expect(errParams.sessionId).toBe(sessionId);
			expect(errParams.message.length).toBeGreaterThan(0);
			expect(turnFinished.params).toMatchObject({ sessionId });
		} finally {
			await server.stop();
			await provider.close();
		}
	}, 15000);
});

/// Mock provider that always returns HTTP 401 — simulates an OpenAI
/// endpoint rejecting an empty/invalid API key.
function startMockProvider401(): Promise<MockServer> {
	return new Promise((resolve) => {
		const server = createServer((req, res) => {
			req.on("data", () => undefined);
			req.on("end", () => {
				res.statusCode = 401;
				res.setHeader("Content-Type", "application/json");
				res.end(JSON.stringify({ error: { message: "Invalid API key", type: "invalid_request_error" } }));
			});
		});
		server.listen(0, "127.0.0.1", () => {
			const addr = server.address() as AddressInfo;
			resolve({
				server,
				url: `http://127.0.0.1:${addr.port}`,
				close: () => new Promise((r) => server.close(() => r())),
			});
		});
	});
}
