import { createServer, type IncomingHttpHeaders, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { afterEach, describe, expect, it, vi } from "vitest";
import { Session, type ProviderKind } from "../src/session.js";

const platformDescriptor = Object.getOwnPropertyDescriptor(process, "platform")!;
let server: Server | undefined;

afterEach(async () => {
	Object.defineProperty(process, "platform", platformDescriptor);
	vi.unstubAllEnvs();
	if (server) await new Promise<void>((resolve) => server!.close(() => resolve()));
	server = undefined;
});

async function prompt(platform: string, kind: ProviderKind, keyRef?: string) {
	Object.defineProperty(process, "platform", { ...platformDescriptor, value: platform });
	vi.stubEnv("OLLAMA_API_KEY", "");
	vi.stubEnv("OPENAI_API_KEY", "");
	const requests: IncomingHttpHeaders[] = [];
	server = createServer((req, res) => {
		requests.push(req.headers);
		req.resume();
		req.on("end", () => {
			res.writeHead(200, { "Content-Type": "text/event-stream" });
			for (const [content, finish_reason] of [["Local reply", null], ["", "stop"]]) {
				res.write(`data: ${JSON.stringify({
					id: "qa", object: "chat.completion.chunk", created: 1, model: "qa-model",
					choices: [{ index: 0, delta: { content }, finish_reason }],
				})}\n\n`);
			}
			res.end("data: [DONE]\n\n");
		});
	});
	await new Promise<void>((resolve) => server!.listen(0, "127.0.0.1", resolve));
	const port = (server.address() as AddressInfo).port;
	const getApiKey = vi.fn(async () => "qa-key");
	const session = new Session({
		sessionId: "qa", model: "qa-model",
		providerConfig: { kind, key_ref: keyRef, base_url: `http://127.0.0.1:${port}/v1` },
	}, {
		getApiKey, notifyApprovalRequest: () => {},
		rpcBridge: {
			notify: () => {}, registerCommandSubscriber: () => {},
			unregisterCommandSubscriber: () => {},
		},
	});
	await session.prompt("hello", () => {});
	return { requests, getApiKey, assistant: session.transcript().find((m) => m.role === "assistant") };
}

describe("Mac keyless Ollama and Windows boundary", () => {
	it("streams a Mac keyless Ollama reply without Authorization or keychain lookup", async () => {
		const result = await prompt("darwin", "ollama");
		expect(result.requests).toHaveLength(1);
		expect(result.requests[0].authorization).toBeUndefined();
		expect(result.getApiKey).not.toHaveBeenCalled();
		expect(result.assistant).toMatchObject({ stopReason: "stop", content: [{ type: "text", text: "Local reply" }] });
	});

	it.each(["darwin", "win32"])("retains keyed Ollama authentication on %s", async (platform) => {
		const result = await prompt(platform, "ollama", "keychain:omw/qa");
		expect(result.requests[0].authorization).toBe("Bearer qa-key");
		expect(result.getApiKey).toHaveBeenCalledWith("keychain:omw/qa");
		expect(result.assistant).toMatchObject({ stopReason: "stop" });
	});

	it.each(["win32", "linux"])("preserves existing keyless Ollama behavior on %s", async (platform) => {
		const result = await prompt(platform, "ollama");
		expect(result.requests).toHaveLength(0);
		expect(result.assistant).toMatchObject({ stopReason: "error" });
	});

	it("does not bypass required authentication for other Mac providers", async () => {
		const result = await prompt("darwin", "openai-compatible");
		expect(result.requests).toHaveLength(0);
		expect(result.assistant).toMatchObject({ stopReason: "error" });
	});
});
