// Vitest unit tests for `apps/omw-agent/src/cli.ts` — the `omw-agent` CLI
// entry point that backs `omw ask <prompt>` for the v0.1 MVP.
//
// File-boundary note: this file is owned by the Test Overseer under the TRD
// protocol. The Executor authors `apps/omw-agent/src/cli.ts` and may NOT
// modify this test file.
//
// =====================================================================
// Contract that `apps/omw-agent/src/cli.ts` MUST satisfy
// =====================================================================
//
//   export interface RunCliOptions {
//     stdout: NodeJS.WritableStream;
//     stderr: NodeJS.WritableStream;
//     fetchImpl?: typeof fetch;
//     getKeychainSecretImpl?: (
//       keyRef: string,
//     ) => Promise<string | undefined>;
//   }
//
//   export async function runCli(
//     argv: string[],            // argv WITHOUT node and script (e.g. ["ask", "hello"])
//     env: Record<string, string>,
//     opts: RunCliOptions,
//   ): Promise<number>;          // exit code
//
// Behavior the tests assert below:
//
// 1. `runCli` reads the config file at `env.OMW_CONFIG` (TOML, same schema as
//    omw-config: `version`, `default_provider`, `[providers.<id>]`).
// 2. The first positional after `ask` is the prompt. Flags: `--provider`,
//    `--model`, `--max-tokens`, `--temperature`.
// 3. Resolves provider id by, in order: `--provider <id>` → `default_provider`
//    in config → fail.
// 4. Resolves model by, in order: `--model <m>` → provider's `default_model`
//    in config → fail.
// 5. Resolves the API key via `getKeychainSecretImpl(provider.key_ref)`. For
//    `kind = "ollama"` providers with no `key_ref`, no key resolution happens.
// 6. Calls `fetchImpl` with provider-shaped URL/headers/body and consumes the
//    streamed response, writing each text delta to `stdout` (no extra
//    framing — pure concatenation).
// 7. After the stream finishes, writes EXACTLY ONE JSON line to `stderr` with
//    keys: `prompt_tokens`, `completion_tokens`, `total_tokens`, `provider`,
//    `model`, `duration_ms`.
// 8. Returns 0 on success; non-zero on any error. On error, `stderr` ends
//    with a human-readable diagnostic; secret material never appears on
//    either stream.
//
// Tests use `fetchImpl` and `getKeychainSecretImpl` injection (no global
// monkey-patching) to keep the surface clean and the tests parallel-safe.

import { Writable } from "node:stream";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { assertNoSecretLeak } from "./_helpers.js";

// =====================================================================
// Test plumbing
// =====================================================================

/** Buffered Writable that records every chunk written, as utf8. */
class BufferSink extends Writable {
	chunks: Buffer[] = [];
	override _write(
		chunk: Buffer | string,
		_encoding: BufferEncoding,
		cb: (err?: Error | null) => void,
	): void {
		this.chunks.push(typeof chunk === "string" ? Buffer.from(chunk) : chunk);
		cb();
	}
	get text(): string {
		return Buffer.concat(this.chunks).toString("utf8");
	}
}

/** Build a minimal `Response` from a UTF-8 string body using a streaming `body`. */
function makeStreamingResponse(
	body: string,
	init: { status?: number; headers?: Record<string, string> } = {},
): Response {
	const status = init.status ?? 200;
	const stream = new ReadableStream<Uint8Array>({
		start(controller) {
			controller.enqueue(new TextEncoder().encode(body));
			controller.close();
		},
	});
	const headers: Record<string, string> = {
		"content-type": "text/event-stream",
		...(init.headers ?? {}),
	};
	return new Response(stream, { status, headers });
}

/** Seed a TOML config file in a fresh tempdir and return its path. */
function withConfigFile(toml: string): string {
	const os = require("node:os") as typeof import("node:os");
	const fs = require("node:fs") as typeof import("node:fs");
	const path = require("node:path") as typeof import("node:path");
	const dir = fs.mkdtempSync(path.join(os.tmpdir(), "omw-agent-cli-test-"));
	const cfgPath = path.join(dir, "config.toml");
	fs.writeFileSync(cfgPath, toml, "utf8");
	tempDirsToCleanup.push(dir);
	return cfgPath;
}

const tempDirsToCleanup: string[] = [];

beforeEach(() => {
	tempDirsToCleanup.length = 0;
});

afterEach(() => {
	const fs = require("node:fs") as typeof import("node:fs");
	for (const d of tempDirsToCleanup) {
		try {
			fs.rmSync(d, { recursive: true, force: true });
		} catch {
			// best-effort
		}
	}
	vi.restoreAllMocks();
});

/** Lazily import the module under test to avoid stale module state. */
async function loadCli() {
	return await import("../src/cli.js");
}

interface FetchCall {
	url: string;
	init: RequestInit;
}

function makeFetchCapture(response: Response): {
	fetch: typeof fetch;
	calls: FetchCall[];
} {
	const calls: FetchCall[] = [];
	const fetchImpl: typeof fetch = async (
		input: Parameters<typeof fetch>[0],
		init?: Parameters<typeof fetch>[1],
	) => {
		calls.push({ url: typeof input === "string" ? input : (input as URL).toString(), init: init ?? {} });
		return response;
	};
	return { fetch: fetchImpl, calls };
}

function readBodyAsJson(init: RequestInit): Record<string, unknown> {
	const body = init.body;
	if (typeof body !== "string") {
		throw new Error(`expected string body in fetch init, got ${typeof body}`);
	}
	return JSON.parse(body) as Record<string, unknown>;
}

// =====================================================================
// Streaming bodies
// =====================================================================

/** OpenAI SSE stream emitting three text deltas + a usage final + [DONE]. */
const OPENAI_SSE_BODY =
	"data: {\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\n\n" +
	"data: {\"choices\":[{\"delta\":{\"content\":\" \"}}]}\n\n" +
	"data: {\"choices\":[{\"delta\":{\"content\":\"world\"}}]}\n\n" +
	"data: {\"choices\":[{\"delta\":{}}],\"usage\":{\"prompt_tokens\":7,\"completion_tokens\":3,\"total_tokens\":10}}\n\n" +
	"data: [DONE]\n\n";

/**
 * Anthropic SSE stream — content_block_delta for two chunks and a
 * message_delta carrying usage (input_tokens / output_tokens).
 */
const ANTHROPIC_SSE_BODY =
	"event: message_start\n" +
	"data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":11,\"output_tokens\":0}}}\n\n" +
	"event: content_block_delta\n" +
	"data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hello \"}}\n\n" +
	"event: content_block_delta\n" +
	"data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"world\"}}\n\n" +
	"event: message_delta\n" +
	"data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":4}}\n\n" +
	"event: message_stop\n" +
	"data: {\"type\":\"message_stop\"}\n\n";

/** Ollama NDJSON stream — two text deltas and a final done frame with usage. */
const OLLAMA_NDJSON_BODY =
	"{\"message\":{\"role\":\"assistant\",\"content\":\"hello \"},\"done\":false}\n" +
	"{\"message\":{\"role\":\"assistant\",\"content\":\"world\"},\"done\":false}\n" +
	"{\"message\":{\"role\":\"assistant\",\"content\":\"\"},\"done\":true,\"prompt_eval_count\":13,\"eval_count\":4}\n";

// =====================================================================
// Tests
// =====================================================================

describe("runCli — OpenAI streaming", () => {
	it("1. openai_streaming_returns_concatenated_text", async () => {
		const cfg = withConfigFile(`version = 1
default_provider = "openai-prod"

[providers.openai-prod]
kind = "openai"
key_ref = "keychain:omw/openai-prod"
default_model = "gpt-4o"
`);

		const { fetch: fetchImpl, calls } = makeFetchCapture(
			makeStreamingResponse(OPENAI_SSE_BODY),
		);
		const stdout = new BufferSink();
		const stderr = new BufferSink();

		const { runCli } = await loadCli();
		const code = await runCli(
			["ask", "hi"],
			{ OMW_CONFIG: cfg },
			{
				stdout,
				stderr,
				fetchImpl,
				getKeychainSecretImpl: async () => "sk-test",
			},
		);

		expect(code).toBe(0);
		expect(stdout.text).toBe("hello world");
		expect(calls.length).toBe(1);

		// Request body shape — the user prompt must be in messages and
		// streaming must be requested.
		const body = readBodyAsJson(calls[0].init);
		expect(body.stream).toBe(true);
		const messages = body.messages as Array<{ role: string; content: string }>;
		expect(Array.isArray(messages)).toBe(true);
		expect(
			messages.some((m) => m.role === "user" && m.content.includes("hi")),
		).toBe(true);

		// Authorization: Bearer <something> — we don't compare the value,
		// just that the Bearer-prefixed header is present.
		const headers = normalizeHeaders(calls[0].init.headers);
		expect(headers["authorization"]).toMatch(/^Bearer\s+\S+/);

		// Stderr must be EXACTLY ONE non-empty JSON line on success.
		const stderrLines = stderr.text.split("\n").filter((l) => l.trim());
		expect(stderrLines.length).toBe(1);
		const parsed = JSON.parse(stderrLines[0]) as Record<string, unknown>;
		expect(parsed.prompt_tokens).toBe(7);
		expect(parsed.completion_tokens).toBe(3);
		expect(parsed.total_tokens).toBe(10);
		expect(parsed.provider).toBe("openai-prod");
		expect(parsed.model).toBe("gpt-4o");
		expect(typeof parsed.duration_ms).toBe("number");
	});

	it("2. openai_compatible_uses_custom_base_url", async () => {
		const cfg = withConfigFile(`version = 1
default_provider = "azure"

[providers.azure]
kind = "openai-compatible"
key_ref = "keychain:omw/azure"
base_url = "https://my-azure.example/v1"
default_model = "gpt-4o"
`);

		const { fetch: fetchImpl, calls } = makeFetchCapture(
			makeStreamingResponse(OPENAI_SSE_BODY),
		);
		const stdout = new BufferSink();
		const stderr = new BufferSink();

		const { runCli } = await loadCli();
		const code = await runCli(
			["ask", "hi"],
			{ OMW_CONFIG: cfg },
			{
				stdout,
				stderr,
				fetchImpl,
				getKeychainSecretImpl: async () => "sk-azure",
			},
		);

		expect(code).toBe(0);
		expect(calls.length).toBe(1);
		// The URL must use the custom base + chat completions path. We accept
		// either `<base>/chat/completions` or `<base>/v1/chat/completions`
		// depending on whether the impl strips a trailing /v1 — but the tested
		// invariant is that the host is the custom one and `/chat/completions`
		// appears.
		expect(calls[0].url).toContain("my-azure.example");
		expect(calls[0].url).toContain("/chat/completions");
		expect(calls[0].url).not.toContain("api.openai.com");

		// openai-compatible providers must also use the OpenAI request
		// shape: Bearer auth, streaming requested, prompt in messages.
		const body = readBodyAsJson(calls[0].init);
		expect(body.stream).toBe(true);
		const messages = body.messages as Array<{ role: string; content: string }>;
		expect(
			messages.some((m) => m.role === "user" && m.content.includes("hi")),
		).toBe(true);
		const headers = normalizeHeaders(calls[0].init.headers);
		expect(headers["authorization"]).toMatch(/^Bearer\s+\S+/);
	});

	it("12. model_flag_overrides_default_model", async () => {
		const cfg = withConfigFile(`version = 1
default_provider = "openai-prod"

[providers.openai-prod]
kind = "openai"
key_ref = "keychain:omw/openai-prod"
default_model = "gpt-3.5"
`);

		const { fetch: fetchImpl, calls } = makeFetchCapture(
			makeStreamingResponse(OPENAI_SSE_BODY),
		);
		const stdout = new BufferSink();
		const stderr = new BufferSink();

		const { runCli } = await loadCli();
		const code = await runCli(
			["ask", "hi", "--model", "gpt-4o"],
			{ OMW_CONFIG: cfg },
			{
				stdout,
				stderr,
				fetchImpl,
				getKeychainSecretImpl: async () => "sk-test",
			},
		);

		expect(code).toBe(0);
		const body = readBodyAsJson(calls[0].init);
		expect(body.model).toBe("gpt-4o");
	});

	it("13. max_tokens_and_temperature_propagate_to_request", async () => {
		const cfg = withConfigFile(`version = 1
default_provider = "openai-prod"

[providers.openai-prod]
kind = "openai"
key_ref = "keychain:omw/openai-prod"
default_model = "gpt-4o"
`);

		const { fetch: fetchImpl, calls } = makeFetchCapture(
			makeStreamingResponse(OPENAI_SSE_BODY),
		);
		const stdout = new BufferSink();
		const stderr = new BufferSink();

		const { runCli } = await loadCli();
		const code = await runCli(
			[
				"ask",
				"hi",
				"--max-tokens",
				"100",
				"--temperature",
				"0.5",
			],
			{ OMW_CONFIG: cfg },
			{
				stdout,
				stderr,
				fetchImpl,
				getKeychainSecretImpl: async () => "sk-test",
			},
		);

		expect(code).toBe(0);
		const body = readBodyAsJson(calls[0].init);
		expect(body.max_tokens).toBe(100);
		expect(body.temperature).toBe(0.5);
	});
});

describe("runCli — Anthropic streaming", () => {
	it("3. anthropic_streaming_concatenates_deltas", async () => {
		const cfg = withConfigFile(`version = 1
default_provider = "anthro"

[providers.anthro]
kind = "anthropic"
key_ref = "keychain:omw/anthro"
default_model = "claude-sonnet-4-6"
`);

		const { fetch: fetchImpl, calls } = makeFetchCapture(
			makeStreamingResponse(ANTHROPIC_SSE_BODY),
		);
		const stdout = new BufferSink();
		const stderr = new BufferSink();

		const { runCli } = await loadCli();
		const code = await runCli(
			["ask", "hi"],
			{ OMW_CONFIG: cfg },
			{
				stdout,
				stderr,
				fetchImpl,
				getKeychainSecretImpl: async () => "sk-ant-x",
			},
		);

		expect(code).toBe(0);
		expect(stdout.text).toBe("hello world");

		// URL must be Anthropic's messages endpoint.
		expect(calls.length).toBe(1);
		expect(calls[0].url).toBe("https://api.anthropic.com/v1/messages");

		// Request body shape — streaming requested, prompt in messages.
		const body = readBodyAsJson(calls[0].init);
		expect(body.stream).toBe(true);
		const messages = body.messages as Array<{ role: string; content: unknown }>;
		expect(Array.isArray(messages)).toBe(true);
		// Anthropic accepts either string content or array-of-blocks; we
		// just need the user prompt to be reachable somewhere in there.
		const userMsg = messages.find((m) => m.role === "user");
		expect(userMsg).toBeTruthy();
		expect(JSON.stringify(userMsg)).toContain("hi");

		// Anthropic auth headers: x-api-key non-empty, anthropic-version present.
		const headers = normalizeHeaders(calls[0].init.headers);
		expect(typeof headers["x-api-key"]).toBe("string");
		expect(headers["x-api-key"].length).toBeGreaterThan(0);
		expect(headers["anthropic-version"]).toBeTruthy();

		// Stderr must be EXACTLY ONE non-empty JSON line on success.
		const stderrLines = stderr.text.split("\n").filter((l) => l.trim());
		expect(stderrLines.length).toBe(1);
		const parsed = JSON.parse(stderrLines[0]) as Record<string, unknown>;
		expect(parsed.prompt_tokens).toBe(11);
		expect(parsed.completion_tokens).toBe(4);
		expect(parsed.total_tokens).toBe(15);
		expect(parsed.provider).toBe("anthro");
		expect(parsed.model).toBe("claude-sonnet-4-6");
	});

	it("14. provider_kind_anthropic_uses_x_api_key_header_not_authorization", async () => {
		const cfg = withConfigFile(`version = 1
default_provider = "anthro"

[providers.anthro]
kind = "anthropic"
key_ref = "keychain:omw/anthro"
default_model = "claude-sonnet-4-6"
`);

		const { fetch: fetchImpl, calls } = makeFetchCapture(
			makeStreamingResponse(ANTHROPIC_SSE_BODY),
		);
		const stdout = new BufferSink();
		const stderr = new BufferSink();

		const { runCli } = await loadCli();
		const code = await runCli(
			["ask", "hi"],
			{ OMW_CONFIG: cfg },
			{
				stdout,
				stderr,
				fetchImpl,
				getKeychainSecretImpl: async () => "sk-ant-x",
			},
		);

		expect(code).toBe(0);
		const headers = normalizeHeaders(calls[0].init.headers);
		expect(headers["x-api-key"]).toBe("sk-ant-x");
		expect(headers["anthropic-version"]).toBeTruthy();
		expect(headers["authorization"]).toBeUndefined();
	});
});

describe("runCli — Ollama streaming", () => {
	it("4. ollama_streaming_ndjson", async () => {
		const cfg = withConfigFile(`version = 1
default_provider = "local"

[providers.local]
kind = "ollama"
default_model = "llama3.1:8b"
`);

		const { fetch: fetchImpl, calls } = makeFetchCapture(
			new Response(
				new ReadableStream<Uint8Array>({
					start(controller) {
						controller.enqueue(new TextEncoder().encode(OLLAMA_NDJSON_BODY));
						controller.close();
					},
				}),
				{ status: 200, headers: { "content-type": "application/x-ndjson" } },
			),
		);
		const stdout = new BufferSink();
		const stderr = new BufferSink();

		const { runCli } = await loadCli();
		const code = await runCli(
			["ask", "hi"],
			{ OMW_CONFIG: cfg },
			{
				stdout,
				stderr,
				fetchImpl,
				// Should not be called — ollama with no key_ref.
				getKeychainSecretImpl: async () => {
					throw new Error("getKeychainSecret should not be called for ollama-no-key");
				},
			},
		);

		expect(code).toBe(0);
		expect(stdout.text).toBe("hello world");
		expect(calls.length).toBe(1);
		// Default ollama base_url:
		expect(calls[0].url).toBe("http://127.0.0.1:11434/api/chat");

		// Stderr must be EXACTLY ONE non-empty JSON line on success.
		const stderrLines = stderr.text.split("\n").filter((l) => l.trim());
		expect(stderrLines.length).toBe(1);
		const parsed = JSON.parse(stderrLines[0]) as Record<string, unknown>;
		expect(parsed.prompt_tokens).toBe(13);
		expect(parsed.completion_tokens).toBe(4);
		expect(parsed.total_tokens).toBe(17);
		expect(parsed.provider).toBe("local");
		expect(parsed.model).toBe("llama3.1:8b");
	});

	it("15. ollama_with_no_key_ref_omits_authorization", async () => {
		const cfg = withConfigFile(`version = 1
default_provider = "local"

[providers.local]
kind = "ollama"
default_model = "llama3.1:8b"
`);

		const { fetch: fetchImpl, calls } = makeFetchCapture(
			new Response(
				new ReadableStream<Uint8Array>({
					start(controller) {
						controller.enqueue(new TextEncoder().encode(OLLAMA_NDJSON_BODY));
						controller.close();
					},
				}),
				{ status: 200, headers: { "content-type": "application/x-ndjson" } },
			),
		);
		const stdout = new BufferSink();
		const stderr = new BufferSink();

		const { runCli } = await loadCli();
		const code = await runCli(
			["ask", "hi"],
			{ OMW_CONFIG: cfg },
			{
				stdout,
				stderr,
				fetchImpl,
				getKeychainSecretImpl: async () => undefined,
			},
		);

		expect(code).toBe(0);
		const headers = normalizeHeaders(calls[0].init.headers);
		expect(headers["authorization"]).toBeUndefined();
		expect(headers["x-api-key"]).toBeUndefined();
	});
});

describe("runCli — config + key resolution", () => {
	it("5. resolves_provider_key_via_helper_for_openai", async () => {
		const cfg = withConfigFile(`version = 1
default_provider = "openai-prod"

[providers.openai-prod]
kind = "openai"
key_ref = "keychain:omw/openai-prod"
default_model = "gpt-4o"
`);

		const { fetch: fetchImpl } = makeFetchCapture(
			makeStreamingResponse(OPENAI_SSE_BODY),
		);
		const stdout = new BufferSink();
		const stderr = new BufferSink();

		const helperCalls: string[] = [];
		const getKeychainSecretImpl = async (keyRef: string) => {
			helperCalls.push(keyRef);
			return "sk-from-helper";
		};

		const { runCli } = await loadCli();
		const code = await runCli(
			["ask", "hi"],
			{ OMW_CONFIG: cfg },
			{
				stdout,
				stderr,
				fetchImpl,
				getKeychainSecretImpl,
			},
		);

		expect(code).toBe(0);
		expect(helperCalls).toEqual(["keychain:omw/openai-prod"]);
	});

	it("6. honors_default_provider_from_config_when_provider_flag_omitted", async () => {
		const cfg = withConfigFile(`version = 1
default_provider = "openai-prod"

[providers.openai-prod]
kind = "openai-compatible"
key_ref = "keychain:omw/openai-prod"
base_url = "https://default-provider.example/v1"
default_model = "gpt-4o"

[providers.other]
kind = "openai-compatible"
key_ref = "keychain:omw/other"
base_url = "https://other.example/v1"
default_model = "gpt-3.5"
`);

		const { fetch: fetchImpl, calls } = makeFetchCapture(
			makeStreamingResponse(OPENAI_SSE_BODY),
		);
		const stdout = new BufferSink();
		const stderr = new BufferSink();

		const { runCli } = await loadCli();
		const code = await runCli(
			["ask", "hi"], // no --provider flag
			{ OMW_CONFIG: cfg },
			{
				stdout,
				stderr,
				fetchImpl,
				getKeychainSecretImpl: async () => "sk-x",
			},
		);

		expect(code).toBe(0);
		expect(calls.length).toBe(1);
		expect(calls[0].url).toContain("default-provider.example");
		expect(calls[0].url).not.toContain("other.example");
	});

	it("7. errors_when_default_model_missing", async () => {
		const cfg = withConfigFile(`version = 1
default_provider = "openai-prod"

[providers.openai-prod]
kind = "openai"
key_ref = "keychain:omw/openai-prod"
`); // no default_model

		const { fetch: fetchImpl, calls } = makeFetchCapture(
			makeStreamingResponse(OPENAI_SSE_BODY),
		);
		const stdout = new BufferSink();
		const stderr = new BufferSink();

		const { runCli } = await loadCli();
		const code = await runCli(
			["ask", "hi"], // no --model
			{ OMW_CONFIG: cfg },
			{
				stdout,
				stderr,
				fetchImpl,
				getKeychainSecretImpl: async () => "sk-x",
			},
		);

		expect(code).not.toBe(0);
		// Should not have made an HTTP request.
		expect(calls.length).toBe(0);
		// Diagnostic mentions model.
		expect(stderr.text.toLowerCase()).toContain("model");
	});

	it("8. errors_when_provider_not_in_config", async () => {
		const cfg = withConfigFile(`version = 1
default_provider = "openai-prod"

[providers.openai-prod]
kind = "openai"
key_ref = "keychain:omw/openai-prod"
default_model = "gpt-4o"
`);

		const { fetch: fetchImpl, calls } = makeFetchCapture(
			makeStreamingResponse(OPENAI_SSE_BODY),
		);
		const stdout = new BufferSink();
		const stderr = new BufferSink();

		const { runCli } = await loadCli();
		const code = await runCli(
			["ask", "hi", "--provider", "ghost"],
			{ OMW_CONFIG: cfg },
			{
				stdout,
				stderr,
				fetchImpl,
				getKeychainSecretImpl: async () => "sk-x",
			},
		);

		expect(code).not.toBe(0);
		expect(calls.length).toBe(0);
		const lower = stderr.text.toLowerCase();
		// Either "provider" + "ghost" or "not found"/"unknown".
		expect(lower).toContain("ghost");
	});

	it("16. provider_flag_overrides_default_provider", async () => {
		// Config declares `default_provider = "alpha"` but ALSO defines
		// `[providers.beta]`. When the user passes `--provider beta`, the
		// flag must win — beta's URL gets called, not alpha's.
		const cfg = withConfigFile(`version = 1
default_provider = "alpha"

[providers.alpha]
kind = "openai-compatible"
key_ref = "keychain:omw/alpha"
base_url = "https://alpha.example/v1"
default_model = "gpt-4o"

[providers.beta]
kind = "openai-compatible"
key_ref = "keychain:omw/beta"
base_url = "https://beta.example/v1"
default_model = "gpt-4o"
`);

		const { fetch: fetchImpl, calls } = makeFetchCapture(
			makeStreamingResponse(OPENAI_SSE_BODY),
		);
		const stdout = new BufferSink();
		const stderr = new BufferSink();

		const { runCli } = await loadCli();
		const code = await runCli(
			["ask", "hi", "--provider", "beta"],
			{ OMW_CONFIG: cfg },
			{
				stdout,
				stderr,
				fetchImpl,
				getKeychainSecretImpl: async () => "sk-x",
			},
		);

		expect(code).toBe(0);
		expect(calls.length).toBe(1);
		expect(calls[0].url).toContain("beta.example");
		expect(calls[0].url).not.toContain("alpha.example");
	});

	it("17. errors_when_no_provider_resolvable", async () => {
		// Config has NO `default_provider` and the user did not pass
		// `--provider`. The CLI must exit non-zero with a diagnostic
		// explaining that no provider could be resolved, and MUST NOT
		// call fetch.
		const cfg = withConfigFile(`version = 1

[providers.alpha]
kind = "openai-compatible"
key_ref = "keychain:omw/alpha"
base_url = "https://alpha.example/v1"
default_model = "gpt-4o"
`);

		const { fetch: fetchImpl, calls } = makeFetchCapture(
			makeStreamingResponse(OPENAI_SSE_BODY),
		);
		const stdout = new BufferSink();
		const stderr = new BufferSink();

		const { runCli } = await loadCli();
		const code = await runCli(
			["ask", "hi"], // no --provider, no default_provider
			{ OMW_CONFIG: cfg },
			{
				stdout,
				stderr,
				fetchImpl,
				getKeychainSecretImpl: async () => "sk-x",
			},
		);

		expect(code).not.toBe(0);
		expect(calls.length).toBe(0);
		// Diagnostic must explain the resolution failure.
		expect(stderr.text.toLowerCase()).toContain("provider");
	});
});

describe("runCli — usage telemetry", () => {
	it("9. token_usage_emitted_to_stderr_as_json", async () => {
		const cfg = withConfigFile(`version = 1
default_provider = "openai-prod"

[providers.openai-prod]
kind = "openai"
key_ref = "keychain:omw/openai-prod"
default_model = "gpt-4o"
`);

		const { fetch: fetchImpl } = makeFetchCapture(
			makeStreamingResponse(OPENAI_SSE_BODY),
		);
		const stdout = new BufferSink();
		const stderr = new BufferSink();

		const { runCli } = await loadCli();
		const code = await runCli(
			["ask", "hi"],
			{ OMW_CONFIG: cfg },
			{
				stdout,
				stderr,
				fetchImpl,
				getKeychainSecretImpl: async () => "sk-x",
			},
		);

		expect(code).toBe(0);
		// Stderr must be EXACTLY ONE non-empty JSON line on success — no
		// preamble, no warnings, no banner — so downstream callers can
		// `JSON.parse(stderr.trim())` directly.
		const stderrLines = stderr.text.split("\n").filter((l) => l.trim());
		expect(stderrLines.length).toBe(1);
		const parsed = JSON.parse(stderrLines[0]) as Record<string, unknown>;
		const expectedKeys = [
			"prompt_tokens",
			"completion_tokens",
			"total_tokens",
			"provider",
			"model",
			"duration_ms",
		];
		for (const k of expectedKeys) {
			expect(parsed).toHaveProperty(k);
		}
		expect(typeof parsed.prompt_tokens).toBe("number");
		expect(typeof parsed.completion_tokens).toBe("number");
		expect(typeof parsed.total_tokens).toBe("number");
		expect(typeof parsed.provider).toBe("string");
		expect(typeof parsed.model).toBe("string");
		expect(typeof parsed.duration_ms).toBe("number");
	});
});

describe("runCli — secret hygiene & errors", () => {
	it("10. secret_value_never_appears_in_stdout_or_stderr", async () => {
		// High-entropy sentinel: random consonant/digit clusters with no
		// English substrings of length >= 4. Avoids dictionary-word
		// substrings (e.g. "error", "path", "secret") that legitimately
		// appear in diagnostics and would cause `assertNoSecretLeak` at
		// minWindow=4 to false-fail.
		const SENTINEL = "Hf3qZx7vKp9wQjL2Bx4TnRm6YcVd8WgPnTr5KsBh3Lk7CzQv9Xj";
		const cfg = withConfigFile(`version = 1
default_provider = "openai-prod"

[providers.openai-prod]
kind = "openai"
key_ref = "keychain:omw/openai-prod"
default_model = "gpt-4o"
`);

		// Make the response a normal success — even on the happy path the
		// sentinel must never leak.
		const { fetch: fetchImpl } = makeFetchCapture(
			makeStreamingResponse(OPENAI_SSE_BODY),
		);
		const stdout = new BufferSink();
		const stderr = new BufferSink();

		const { runCli } = await loadCli();
		const code = await runCli(
			["ask", "hi"],
			{ OMW_CONFIG: cfg },
			{
				stdout,
				stderr,
				fetchImpl,
				getKeychainSecretImpl: async () => SENTINEL,
			},
		);

		expect(code).toBe(0);
		assertNoSecretLeak(stdout.text, SENTINEL, 4);
		assertNoSecretLeak(stderr.text, SENTINEL, 4);
	});

	it("11. error_response_4xx_returns_nonzero_exit_with_provider_error_to_stderr", async () => {
		// High-entropy sentinel: see test 10 for rationale. The error path
		// in particular is at risk of false-failing if the sentinel
		// contains substrings like "error", "path", "fail", or "401" that
		// the diagnostic itself prints.
		const SENTINEL = "Mr4FbHt6QnYz8DcRp2VwLmKxJg9StBn5HpQv7ZdLfWnXjCkRb";
		const cfg = withConfigFile(`version = 1
default_provider = "openai-prod"

[providers.openai-prod]
kind = "openai"
key_ref = "keychain:omw/openai-prod"
default_model = "gpt-4o"
`);

		const errorResp = new Response(
			JSON.stringify({ error: { message: "unauthorized" } }),
			{
				status: 401,
				headers: { "content-type": "application/json" },
			},
		);

		const { fetch: fetchImpl } = makeFetchCapture(errorResp);
		const stdout = new BufferSink();
		const stderr = new BufferSink();

		const { runCli } = await loadCli();
		const code = await runCli(
			["ask", "hi"],
			{ OMW_CONFIG: cfg },
			{
				stdout,
				stderr,
				fetchImpl,
				getKeychainSecretImpl: async () => SENTINEL,
			},
		);

		expect(code).not.toBe(0);
		const lower = stderr.text.toLowerCase();
		// Must mention either the status code or the provider's wording.
		const hasDiag = lower.includes("401") || lower.includes("unauthorized");
		expect(hasDiag).toBe(true);

		// Hard guarantee: the sentinel API key never leaks via the error
		// path either. An impl that prints the request headers verbatim on
		// failure would fail this.
		assertNoSecretLeak(stdout.text, SENTINEL, 4);
		assertNoSecretLeak(stderr.text, SENTINEL, 4);
	});
});

// =====================================================================
// Helper: normalize fetch HeadersInit to a lowercase-keyed plain object so
// tests can assert presence/absence regardless of whether the Executor used
// `new Headers()` or a plain object.
// =====================================================================
function normalizeHeaders(h: HeadersInit | undefined): Record<string, string> {
	if (!h) return {};
	const out: Record<string, string> = {};
	if (h instanceof Headers) {
		h.forEach((v, k) => {
			out[k.toLowerCase()] = v;
		});
		return out;
	}
	if (Array.isArray(h)) {
		for (const [k, v] of h) {
			out[k.toLowerCase()] = v;
		}
		return out;
	}
	for (const [k, v] of Object.entries(h)) {
		out[k.toLowerCase()] = v as string;
	}
	return out;
}

// Vitest needs at least one describe/it that REGISTERS — the above blocks do.
