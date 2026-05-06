// Phase 1 cancellation test for `runStdioServer`.
//
// Mock OpenAI-compatible provider streams chunks slowly. The test sends
// session/cancel mid-stream and asserts a turn/finished notification with
// cancelled: true arrives within 1 second.
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

// Streaming mock that emits a chunk every `delayBetweenMs` so the agent
// loop has time to receive at least one delta before we cancel. Honours
// AbortSignal-driven socket close: when the client tears down, we stop.
function startSlowProvider(opts: {
	chunks: string[];
	delayBetweenMs: number;
}): Promise<MockServer> {
	return new Promise((resolve) => {
		const server = createServer(async (req, res) => {
			if (req.url !== "/chat/completions" || req.method !== "POST") {
				res.statusCode = 404;
				res.end();
				return;
			}
			req.on("data", () => undefined);
			await new Promise<void>((r) => req.on("end", r));
			res.statusCode = 200;
			res.setHeader("Content-Type", "text/event-stream");
			res.setHeader("Cache-Control", "no-cache");
			res.setHeader("Connection", "keep-alive");
			let aborted = false;
			req.socket.on("close", () => {
				aborted = true;
			});
			res.on("error", () => {
				aborted = true;
			});
			try {
				for (const chunk of opts.chunks) {
					if (aborted) break;
					try {
						res.write(`data: ${chunk}\n\n`);
					} catch {
						break;
					}
					await delay(opts.delayBetweenMs);
				}
				if (!aborted) {
					res.write("data: [DONE]\n\n");
					res.end();
				}
			} catch {
				// Cancel race: ignore writes-after-close.
			}
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

function send(stdin: PassThrough, req: JsonRpcFrame): void {
	stdin.write(`${JSON.stringify(req)}\n`);
}

describe("runStdioServer cancellation", () => {
	let mock: MockServer | null = null;

	beforeEach(() => {
		mock = null;
	});

	afterEach(async () => {
		if (mock) await mock.close();
	});

	it("session/cancel terminates an in-flight prompt within 1s and reports cancelled: true", async () => {
		// 30 chunks at 100ms each = 3 s of stream; cancel after the first
		// delta arrives and assert turn/finished within 1 s.
		const chunks = Array.from({ length: 30 }, (_, i) => deltaChunk(`tok${i}`));
		chunks.push(deltaChunk("", "stop"));
		mock = await startSlowProvider({ chunks, delayBetweenMs: 100 });

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
				// Hand to first matching waiter; otherwise queue. Never both.
				const idx = waiters.findIndex((w) => w.match(frame));
				if (idx >= 0) {
					waiters.splice(idx, 1)[0].resolve(frame);
				} else {
					frames.push(frame);
				}
			}
		});

		const next = (match: (f: JsonRpcFrame) => boolean): Promise<JsonRpcFrame> => {
			const idx = frames.findIndex(match);
			if (idx >= 0) return Promise.resolve(frames.splice(idx, 1)[0]);
			return new Promise((resolve) => waiters.push({ match, resolve }));
		};

		const serverDone = runStdioServer({
			stdin,
			stdout,
			stderr,
			getApiKey: async () => "sk-test",
		});

		try {
			send(stdin, {
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
			const created = await next((f) => f.id === 1);
			const sessionId = (created.result as { sessionId: string }).sessionId;

			send(stdin, {
				jsonrpc: "2.0",
				id: 2,
				method: "session/prompt",
				params: { sessionId, prompt: "long prompt" },
			});
			await next((f) => f.id === 2);

			// Wait for at least one delta so the stream is genuinely in flight.
			await next((f) => f.method === "assistant/delta");

			const cancelStart = Date.now();
			send(stdin, {
				jsonrpc: "2.0",
				id: 3,
				method: "session/cancel",
				params: { sessionId },
			});

			const finished = await Promise.race([
				next((f) => f.method === "turn/finished"),
				delay(1000).then(() => null),
			]);
			expect(finished).not.toBeNull();
			expect(finished?.params).toMatchObject({ sessionId, cancelled: true });
			expect(Date.now() - cancelStart).toBeLessThan(1000);
		} finally {
			stdin.end();
			await serverDone;
		}
	}, 10000);
});
