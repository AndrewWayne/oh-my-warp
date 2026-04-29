// `omw-agent` CLI behind `omw ask`. Reads OMW_CONFIG, resolves provider
// + model, calls the provider's streaming endpoint, writes text deltas to
// stdout, and writes a single-line JSON usage telemetry record to stderr.
//
// Threat-model invariants (specs/threat-model.md):
// - I-1: API keys NEVER appear on stdout/stderr. Provider error bodies
//   are NOT formatted with the request headers; we only fold in the
//   provider-supplied error text (which may not contain the key).
// - This module never reads or writes the keychain directly — secret
//   resolution is delegated to `getKeychainSecretImpl` (or, by default,
//   the `getKeychainSecret` helper in `./keychain.ts`).

import { readFileSync } from "node:fs";

import * as toml from "@iarna/toml";

import { getKeychainSecret } from "./keychain.js";

export interface RunCliOptions {
	stdout: NodeJS.WritableStream;
	stderr: NodeJS.WritableStream;
	fetchImpl?: typeof fetch;
	getKeychainSecretImpl?: (keyRef: string) => Promise<string | undefined>;
}

interface ProviderConfigCommon {
	kind: string;
	key_ref?: string;
	base_url?: string;
	default_model?: string;
}

interface ParsedConfig {
	default_provider?: string;
	providers: Record<string, ProviderConfigCommon>;
}

interface ParsedFlags {
	prompt: string;
	provider?: string;
	model?: string;
	maxTokens?: number;
	temperature?: number;
}

interface UsageTotals {
	prompt_tokens: number;
	completion_tokens: number;
}

const ANTHROPIC_DEFAULT_MAX_TOKENS = 4096;
const OLLAMA_DEFAULT_BASE_URL = "http://127.0.0.1:11434";
const OPENAI_DEFAULT_BASE_URL = "https://api.openai.com";
const ANTHROPIC_BASE_URL = "https://api.anthropic.com";
const ANTHROPIC_VERSION = "2023-06-01";

export async function runCli(
	argv: string[],
	env: Record<string, string>,
	opts: RunCliOptions,
): Promise<number> {
	const { stdout, stderr } = opts;
	const fetchImpl = opts.fetchImpl ?? fetch;
	const getKey =
		opts.getKeychainSecretImpl ??
		((keyRef: string) => getKeychainSecret(keyRef));

	let flags: ParsedFlags;
	try {
		flags = parseArgv(argv);
	} catch (e) {
		writeStderr(stderr, `error: ${(e as Error).message}\n`);
		return 2;
	}

	let cfg: ParsedConfig;
	try {
		cfg = loadConfig(env.OMW_CONFIG);
	} catch (e) {
		writeStderr(stderr, `error: ${(e as Error).message}\n`);
		return 2;
	}

	const providerId = flags.provider ?? cfg.default_provider;
	if (!providerId) {
		writeStderr(
			stderr,
			"error: no provider resolvable; pass --provider or set default_provider in config\n",
		);
		return 2;
	}
	const provider = cfg.providers[providerId];
	if (!provider) {
		writeStderr(
			stderr,
			`error: provider \`${providerId}\` not found in config\n`,
		);
		return 2;
	}
	const model = flags.model ?? provider.default_model;
	if (!model) {
		writeStderr(
			stderr,
			`error: no model specified for provider \`${providerId}\`; pass --model or set default_model\n`,
		);
		return 2;
	}

	// Resolve the API key for kinds that need it.
	let apiKey: string | undefined;
	const kind = provider.kind;
	const needsKeyResolution = kind !== "ollama" || provider.key_ref;
	if (needsKeyResolution && provider.key_ref) {
		try {
			apiKey = await getKey(provider.key_ref);
		} catch (e) {
			writeStderr(
				stderr,
				`error: keychain lookup failed: ${(e as Error).message}\n`,
			);
			return 2;
		}
		if (apiKey === undefined && kind !== "ollama") {
			writeStderr(
				stderr,
				`error: no API key found for provider \`${providerId}\`\n`,
			);
			return 2;
		}
	}

	const startedAt = Date.now();
	let usage: UsageTotals;
	try {
		usage = await dispatchAndStream(
			fetchImpl,
			provider,
			model,
			flags,
			apiKey,
			stdout,
		);
	} catch (e) {
		const msg = (e as Error).message ?? String(e);
		writeStderr(stderr, `error: ${msg}\n`);
		return 1;
	}
	const durationMs = Date.now() - startedAt;

	const telemetry = {
		prompt_tokens: usage.prompt_tokens,
		completion_tokens: usage.completion_tokens,
		total_tokens: usage.prompt_tokens + usage.completion_tokens,
		provider: providerId,
		model,
		duration_ms: durationMs,
	};
	writeStderr(stderr, `${JSON.stringify(telemetry)}\n`);
	return 0;
}

function writeStderr(sink: NodeJS.WritableStream, msg: string): void {
	sink.write(msg);
}

function parseArgv(argv: string[]): ParsedFlags {
	if (argv.length < 1 || argv[0] !== "ask") {
		throw new Error("expected `ask` as the first arg");
	}
	const rest = argv.slice(1);
	let prompt: string | undefined;
	const flags: ParsedFlags = { prompt: "" };
	for (let i = 0; i < rest.length; i++) {
		const a = rest[i];
		if (a === "--provider") {
			flags.provider = requireValue(rest, ++i, a);
		} else if (a === "--model") {
			flags.model = requireValue(rest, ++i, a);
		} else if (a === "--max-tokens") {
			const v = requireValue(rest, ++i, a);
			const n = Number.parseInt(v, 10);
			if (!Number.isFinite(n) || n < 0) {
				throw new Error(`invalid --max-tokens value: ${v}`);
			}
			flags.maxTokens = n;
		} else if (a === "--temperature") {
			const v = requireValue(rest, ++i, a);
			const f = Number.parseFloat(v);
			if (!Number.isFinite(f)) {
				throw new Error(`invalid --temperature value: ${v}`);
			}
			flags.temperature = f;
		} else if (a.startsWith("--")) {
			throw new Error(`unknown flag: ${a}`);
		} else if (prompt === undefined) {
			prompt = a;
		} else {
			throw new Error(`unexpected positional arg: ${a}`);
		}
	}
	if (prompt === undefined) {
		throw new Error("missing required prompt argument");
	}
	flags.prompt = prompt;
	return flags;
}

function requireValue(args: string[], i: number, name: string): string {
	const v = args[i];
	if (v === undefined) {
		throw new Error(`flag ${name} requires a value`);
	}
	return v;
}

function loadConfig(path: string | undefined): ParsedConfig {
	if (!path) {
		throw new Error("OMW_CONFIG is not set");
	}
	let raw: string;
	try {
		raw = readFileSync(path, "utf8");
	} catch (e) {
		throw new Error(
			`unable to read config at ${path}: ${(e as Error).message}`,
		);
	}
	let parsed: toml.JsonMap;
	try {
		parsed = toml.parse(raw);
	} catch (e) {
		throw new Error(
			`unable to parse config at ${path}: ${(e as Error).message}`,
		);
	}
	const providersRaw = (parsed.providers as toml.JsonMap | undefined) ?? {};
	const providers: Record<string, ProviderConfigCommon> = {};
	for (const [id, blockUnknown] of Object.entries(providersRaw)) {
		if (typeof blockUnknown !== "object" || blockUnknown === null) {
			continue;
		}
		const block = blockUnknown as toml.JsonMap;
		const kindUnknown = block.kind;
		if (typeof kindUnknown !== "string") {
			continue;
		}
		const providerCfg: ProviderConfigCommon = {
			kind: kindUnknown,
			key_ref:
				typeof block.key_ref === "string" ? block.key_ref : undefined,
			base_url:
				typeof block.base_url === "string" ? block.base_url : undefined,
			default_model:
				typeof block.default_model === "string"
					? block.default_model
					: undefined,
		};
		validateProviderShape(id, providerCfg);
		providers[id] = providerCfg;
	}
	const defaultProvider =
		typeof parsed.default_provider === "string"
			? parsed.default_provider
			: undefined;
	return {
		default_provider: defaultProvider,
		providers,
	};
}

// Enforce per-kind required fields per omw-config schema. Errors must surface
// before any HTTP call so a misconfigured provider exits non-zero immediately.
function validateProviderShape(
	id: string,
	cfg: ProviderConfigCommon,
): void {
	switch (cfg.kind) {
		case "openai":
			if (!cfg.key_ref) {
				throw new Error(
					`provider \`${id}\` (kind=openai) requires \`key_ref\``,
				);
			}
			break;
		case "anthropic":
			if (!cfg.key_ref) {
				throw new Error(
					`provider \`${id}\` (kind=anthropic) requires \`key_ref\``,
				);
			}
			break;
		case "openai-compatible":
			if (!cfg.key_ref) {
				throw new Error(
					`provider \`${id}\` (kind=openai-compatible) requires \`key_ref\``,
				);
			}
			if (!cfg.base_url) {
				throw new Error(
					`provider \`${id}\` (kind=openai-compatible) requires \`base_url\``,
				);
			}
			break;
		case "ollama":
			// Both `key_ref` and `base_url` are optional.
			break;
		default:
			// Unknown kinds are deferred to dispatchAndStream so the existing
			// "unsupported provider kind" error message remains the source of
			// truth.
			break;
	}
}

async function dispatchAndStream(
	fetchImpl: typeof fetch,
	provider: ProviderConfigCommon,
	model: string,
	flags: ParsedFlags,
	apiKey: string | undefined,
	stdout: NodeJS.WritableStream,
): Promise<UsageTotals> {
	switch (provider.kind) {
		case "openai":
		case "openai-compatible":
			return await streamOpenAi(
				fetchImpl,
				openAiUrl(provider),
				apiKey,
				model,
				flags,
				stdout,
			);
		case "anthropic":
			return await streamAnthropic(
				fetchImpl,
				apiKey,
				model,
				flags,
				stdout,
			);
		case "ollama":
			return await streamOllama(
				fetchImpl,
				provider,
				apiKey,
				model,
				flags,
				stdout,
			);
		default:
			throw new Error(`unsupported provider kind: ${provider.kind}`);
	}
}

function openAiUrl(provider: ProviderConfigCommon): string {
	const base = provider.base_url ?? OPENAI_DEFAULT_BASE_URL;
	const trimmed = base.replace(/\/+$/, "");
	const suffix = trimmed.endsWith("/v1") ? "/chat/completions" : "/v1/chat/completions";
	return `${trimmed}${suffix}`;
}

async function streamOpenAi(
	fetchImpl: typeof fetch,
	url: string,
	apiKey: string | undefined,
	model: string,
	flags: ParsedFlags,
	stdout: NodeJS.WritableStream,
): Promise<UsageTotals> {
	const body: Record<string, unknown> = {
		model,
		messages: [{ role: "user", content: flags.prompt }],
		stream: true,
		stream_options: { include_usage: true },
	};
	if (flags.maxTokens !== undefined) body.max_tokens = flags.maxTokens;
	if (flags.temperature !== undefined) body.temperature = flags.temperature;

	const headers: Record<string, string> = {
		"Content-Type": "application/json",
	};
	if (apiKey) headers["Authorization"] = `Bearer ${apiKey}`;

	const resp = await fetchOrThrow(fetchImpl, url, {
		method: "POST",
		headers,
		body: JSON.stringify(body),
	});
	await ensureOk(resp);

	const usage: UsageTotals = { prompt_tokens: 0, completion_tokens: 0 };
	for await (const evt of iterateSseEvents(resp)) {
		// OpenAI SSE: each event has `data: {...}` (or `data: [DONE]`).
		// Events without explicit `event:` are the default delta events.
		const dataLines = evt.data;
		if (dataLines.length === 0) continue;
		const payload = dataLines.join("\n");
		if (payload === "[DONE]") break;
		let parsed: Record<string, unknown>;
		try {
			parsed = JSON.parse(payload) as Record<string, unknown>;
		} catch {
			continue;
		}
		const choices = parsed.choices as
			| Array<{ delta?: { content?: string } }>
			| undefined;
		if (choices && choices.length > 0) {
			const delta = choices[0].delta?.content;
			if (typeof delta === "string" && delta.length > 0) {
				stdout.write(delta);
			}
		}
		const u = parsed.usage as
			| { prompt_tokens?: number; completion_tokens?: number }
			| undefined;
		if (u) {
			if (typeof u.prompt_tokens === "number") {
				usage.prompt_tokens = u.prompt_tokens;
			}
			if (typeof u.completion_tokens === "number") {
				usage.completion_tokens = u.completion_tokens;
			}
		}
	}
	return usage;
}

async function streamAnthropic(
	fetchImpl: typeof fetch,
	apiKey: string | undefined,
	model: string,
	flags: ParsedFlags,
	stdout: NodeJS.WritableStream,
): Promise<UsageTotals> {
	const url = `${ANTHROPIC_BASE_URL}/v1/messages`;
	const body: Record<string, unknown> = {
		model,
		max_tokens: flags.maxTokens ?? ANTHROPIC_DEFAULT_MAX_TOKENS,
		messages: [{ role: "user", content: flags.prompt }],
		stream: true,
	};
	if (flags.temperature !== undefined) body.temperature = flags.temperature;

	const headers: Record<string, string> = {
		"Content-Type": "application/json",
		"anthropic-version": ANTHROPIC_VERSION,
	};
	if (apiKey) headers["x-api-key"] = apiKey;

	const resp = await fetchOrThrow(fetchImpl, url, {
		method: "POST",
		headers,
		body: JSON.stringify(body),
	});
	await ensureOk(resp);

	const usage: UsageTotals = { prompt_tokens: 0, completion_tokens: 0 };
	for await (const evt of iterateSseEvents(resp)) {
		if (evt.data.length === 0) continue;
		const payload = evt.data.join("\n");
		let parsed: Record<string, unknown>;
		try {
			parsed = JSON.parse(payload) as Record<string, unknown>;
		} catch {
			continue;
		}
		const type = parsed.type;
		if (type === "content_block_delta") {
			const delta = parsed.delta as { text?: string } | undefined;
			if (delta && typeof delta.text === "string") {
				stdout.write(delta.text);
			}
		} else if (type === "message_start") {
			const msg = parsed.message as
				| { usage?: { input_tokens?: number; output_tokens?: number } }
				| undefined;
			const u = msg?.usage;
			if (u && typeof u.input_tokens === "number") {
				usage.prompt_tokens = u.input_tokens;
			}
			if (u && typeof u.output_tokens === "number") {
				usage.completion_tokens = u.output_tokens;
			}
		} else if (type === "message_delta") {
			const u = parsed.usage as
				| { input_tokens?: number; output_tokens?: number }
				| undefined;
			if (u && typeof u.output_tokens === "number") {
				usage.completion_tokens = u.output_tokens;
			}
			if (u && typeof u.input_tokens === "number") {
				usage.prompt_tokens = u.input_tokens;
			}
		}
	}
	return usage;
}

async function streamOllama(
	fetchImpl: typeof fetch,
	provider: ProviderConfigCommon,
	apiKey: string | undefined,
	model: string,
	flags: ParsedFlags,
	stdout: NodeJS.WritableStream,
): Promise<UsageTotals> {
	const base = (provider.base_url ?? OLLAMA_DEFAULT_BASE_URL).replace(
		/\/+$/,
		"",
	);
	const url = `${base}/api/chat`;

	const options: Record<string, unknown> = {};
	if (flags.maxTokens !== undefined) options.num_predict = flags.maxTokens;
	if (flags.temperature !== undefined) options.temperature = flags.temperature;

	const body: Record<string, unknown> = {
		model,
		messages: [{ role: "user", content: flags.prompt }],
		stream: true,
	};
	if (Object.keys(options).length > 0) body.options = options;

	const headers: Record<string, string> = {
		"Content-Type": "application/json",
	};
	if (apiKey) headers["Authorization"] = `Bearer ${apiKey}`;

	const resp = await fetchOrThrow(fetchImpl, url, {
		method: "POST",
		headers,
		body: JSON.stringify(body),
	});
	await ensureOk(resp);

	const usage: UsageTotals = { prompt_tokens: 0, completion_tokens: 0 };
	for await (const line of iterateNdjsonLines(resp)) {
		if (!line) continue;
		let parsed: Record<string, unknown>;
		try {
			parsed = JSON.parse(line) as Record<string, unknown>;
		} catch {
			continue;
		}
		const message = parsed.message as { content?: string } | undefined;
		if (message && typeof message.content === "string") {
			stdout.write(message.content);
		}
		if (parsed.done === true) {
			if (typeof parsed.prompt_eval_count === "number") {
				usage.prompt_tokens = parsed.prompt_eval_count;
			}
			if (typeof parsed.eval_count === "number") {
				usage.completion_tokens = parsed.eval_count;
			}
		}
	}
	return usage;
}

async function fetchOrThrow(
	fetchImpl: typeof fetch,
	url: string,
	init: RequestInit,
): Promise<Response> {
	try {
		return await fetchImpl(url, init);
	} catch (e) {
		throw new Error(`network error: ${(e as Error).message}`);
	}
}

async function ensureOk(resp: Response): Promise<void> {
	if (resp.ok) return;
	let body = "";
	try {
		body = await resp.text();
	} catch {
		// best-effort
	}
	const excerpt = scrubSecrets(
		body.length > 200 ? `${body.slice(0, 200)}…` : body,
	);
	throw new Error(
		`provider returned HTTP ${resp.status}${excerpt ? `: ${excerpt}` : ""}`,
	);
}

// I-1: scrub Bearer tokens and authorization/x-api-key header values that
// some misbehaving proxies echo back in error bodies, so they never reach
// stderr.
//
// Ordering: Bearer first so dotted JWTs (e.g. `Bearer eyJ.foo.bar`) are
// fully consumed before the header rules see the value. The JSON-form rule
// matches `"authorization":"<value>"` shapes (some providers echo the
// request body as JSON in errors). The plain-form rule matches
// `authorization: <value>` / `=` separator. All three rules together cover
// both request-header echoes and JSON body echoes.
function scrubSecrets(s: string): string {
	return s
		.replace(/Bearer\s+[^\s,";]+/gi, "Bearer <redacted>")
		.replace(
			/("(?:authorization|x-api-key)"\s*:\s*")[^"]*(")/gi,
			"$1<redacted>$2",
		)
		.replace(
			/(\b(?:authorization|x-api-key)\s*[:=]\s*)[^\s,;]+/gi,
			"$1<redacted>",
		);
}

interface SseEvent {
	event?: string;
	data: string[];
}

/**
 * Iterate Server-Sent Events from a `Response.body`. Each yielded event has
 * `data` as an array of lines (the SSE spec joins them with `\n`).
 */
async function* iterateSseEvents(resp: Response): AsyncIterable<SseEvent> {
	let pending: SseEvent = { data: [] };
	for await (const line of iterateLines(resp)) {
		if (line === "") {
			if (pending.event !== undefined || pending.data.length > 0) {
				yield pending;
			}
			pending = { data: [] };
			continue;
		}
		if (line.startsWith(":")) {
			// comment line — skip
			continue;
		}
		const colon = line.indexOf(":");
		const field = colon === -1 ? line : line.slice(0, colon);
		let value = colon === -1 ? "" : line.slice(colon + 1);
		if (value.startsWith(" ")) value = value.slice(1);
		if (field === "data") {
			pending.data.push(value);
		} else if (field === "event") {
			pending.event = value;
		}
		// Other fields (id, retry) are unused for our use case.
	}
	if (pending.event !== undefined || pending.data.length > 0) {
		yield pending;
	}
}

async function* iterateNdjsonLines(resp: Response): AsyncIterable<string> {
	for await (const line of iterateLines(resp)) {
		yield line;
	}
}

async function* iterateLines(resp: Response): AsyncIterable<string> {
	if (!resp.body) return;
	const decoder = new TextDecoder("utf-8");
	let buf = "";
	const reader = resp.body.getReader();
	try {
		for (;;) {
			const { value, done } = await reader.read();
			if (value) {
				buf += decoder.decode(value, { stream: true });
				let idx: number;
				while ((idx = buf.indexOf("\n")) >= 0) {
					let line = buf.slice(0, idx);
					if (line.endsWith("\r")) line = line.slice(0, -1);
					buf = buf.slice(idx + 1);
					yield line;
				}
			}
			if (done) break;
		}
		buf += decoder.decode();
		if (buf.length > 0) {
			if (buf.endsWith("\r")) buf = buf.slice(0, -1);
			yield buf;
		}
	} finally {
		reader.releaseLock();
	}
}
