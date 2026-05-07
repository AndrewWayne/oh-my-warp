// Keychain bridge for the TypeScript agent.
//
// This module is the v0.1 transport between the agent's `getApiKey` hooks
// and the omw-keychain CRUD layer. It spawns the `omw-keychain-helper`
// Rust binary (one fork/exec per cache-miss), reads the secret from its
// stdout, and returns it.
//
// Threat-model invariants (specs/threat-model.md):
// - I-1: Secrets never appear in error messages, error JSON, or any
//   `console.*` call. The helper writes secrets only to stdout; `stderr`
//   is treated as untrusted (a misbehaving helper could leak there) and
//   is therefore NEVER folded into `KeychainHelperError.message`.
// - In-process cache only; nothing is written to disk by this layer.

import { spawn } from "node:child_process";

export type KeychainBackend = "memory" | "os" | "auto";

export interface KeychainHelperOptions {
	binaryPath?: string;
	backend?: KeychainBackend;
}

export class KeychainHelperError extends Error {
	readonly exitCode: number;
	constructor(exitCode: number) {
		// Generic message — DO NOT include stderr content. The helper
		// writes secrets to stdout, but a buggy/malicious helper could
		// leak them to stderr; we redact defensively.
		super(`keychain helper exited with code ${exitCode}`);
		this.name = "KeychainHelperError";
		this.exitCode = exitCode;
	}
}

const DEFAULT_BINARY = "omw-keychain-helper";

function resolveBinary(opts: KeychainHelperOptions | undefined): string {
	if (opts?.binaryPath) return opts.binaryPath;
	const fromEnv = process.env.OMW_KEYCHAIN_HELPER;
	if (fromEnv && fromEnv.length > 0) return fromEnv;
	return DEFAULT_BINARY;
}

// Cache key includes binaryPath, backend, and keyRef so:
// - different installs of the helper (system vs project-local) do not
//   alias each other
// - memory/os backends do not share entries (the secret value can differ)
// - distinct keyRefs are independent (obvious)
//
// We cache the *Promise*, not the resolved value, so concurrent callers
// for the same triple coalesce onto a single spawn. On rejection we
// remove the entry so a retry triggers a fresh spawn (errors are NOT
// cached — they may be transient). NotFound (resolve `undefined`) IS
// cached because it's a stable answer.
const cache = new Map<string, Promise<string | undefined>>();

function cacheKey(binary: string, backend: KeychainBackend | undefined, keyRef: string): string {
	return `${binary}|${backend ?? ""}|${keyRef}`;
}

export async function getKeychainSecret(
	keyRef: string,
	opts?: KeychainHelperOptions,
): Promise<string | undefined> {
	const binary = resolveBinary(opts);
	const key = cacheKey(binary, opts?.backend, keyRef);
	const hit = cache.get(key);
	if (hit) return hit;

	const promise = spawnHelper(binary, opts?.backend, keyRef);
	cache.set(key, promise);
	// On rejection, evict so a retry spawns again. We do NOT chain this
	// onto the cached promise itself (callers see the original rejection).
	promise.catch(() => {
		cache.delete(key);
	});
	return promise;
}

async function spawnHelper(
	binary: string,
	backend: KeychainBackend | undefined,
	keyRef: string,
): Promise<string | undefined> {
	const env: NodeJS.ProcessEnv = { ...process.env };
	if (backend !== undefined) {
		env.OMW_KEYCHAIN_BACKEND = backend;
	}

	const child = spawn(binary, ["get", keyRef], { env });

	// CRITICAL: attach error listeners synchronously, BEFORE any other
	// code path can yield to the event loop. Node 25's child_process
	// emits 'error' for spawn failures (ENOENT, EACCES, …) via
	// `process.nextTick`. If the event fires before any listener is
	// attached, Node treats it as an uncaughtException and kills the
	// process. Even though the Promise constructor below also adds an
	// 'error' listener, getting here via `readAll`/`drain` first
	// involves microtask scheduling that — empirically — can lose the
	// race. Attaching no-op listeners up front guarantees the EE has
	// at least one subscriber regardless of when the error arrives.
	// The "real" close/error handling still flows through closePromise.
	const captured: { err: NodeJS.ErrnoException | null } = { err: null };
	child.on("error", (err: NodeJS.ErrnoException) => {
		captured.err = err;
	});
	child.stdout?.on("error", () => {
		/* absorbed by the readAll for-await */
	});
	child.stderr?.on("error", () => {
		/* absorbed by the drain for-await */
	});

	// Read stdout eagerly via async iteration. We must drain the stream
	// regardless of exit code, otherwise the test mock's `Readable.from`
	// never flushes synchronously and we end up reading nothing. Consume
	// stdout/stderr concurrently with waiting for 'close'/'error'.
	//
	// Note: we DO NOT keep stderr content — folding it into errors would
	// risk leaking secrets if a buggy/malicious helper wrote them there
	// (I-1 defense in depth). We still drain it to unblock the child's
	// pipe.
	const stdoutPromise = readAll(child.stdout);
	const stderrDrain = drain(child.stderr);

	const closePromise = new Promise<
		| { kind: "close"; code: number }
		| { kind: "signal" }
		| { kind: "error"; err: NodeJS.ErrnoException }
	>((resolveOuter) => {
		// If the early no-op listener already captured an error, settle
		// immediately — `'close'` may not fire when spawn fails before
		// the child even starts.
		if (captured.err) {
			resolveOuter({ kind: "error", err: captured.err });
			return;
		}
		child.on("close", (code: number | null) => {
			if (captured.err) {
				resolveOuter({ kind: "error", err: captured.err });
				return;
			}
			if (code === null) {
				// Process killed by signal — partial stdout is not a valid
				// secret. Reject with a sentinel exit code; do NOT include
				// the signal name (I-1 defense in depth).
				resolveOuter({ kind: "signal" });
				return;
			}
			resolveOuter({ kind: "close", code });
		});
		child.on("error", (err: NodeJS.ErrnoException) => {
			resolveOuter({ kind: "error", err });
		});
	});

	const result = await closePromise;
	// Make sure we've consumed both streams before deciding (or at least
	// we no longer hold them open). stderr drain may legitimately error
	// after a spawn failure; swallow it.
	const stdoutBuf = await stdoutPromise.catch(() => "");
	await stderrDrain.catch(() => undefined);

	if (result.kind === "error") {
		throw result.err;
	}
	if (result.kind === "signal") {
		throw new KeychainHelperError(-1);
	}

	const exitCode = result.code;
	if (exitCode === 0) {
		// Trim EXACTLY one trailing newline, never more — internal newlines
		// must round-trip.
		return stdoutBuf.endsWith("\n") ? stdoutBuf.slice(0, -1) : stdoutBuf;
	}
	if (exitCode === 1) {
		return undefined; // NotFound
	}
	throw new KeychainHelperError(exitCode);
}

async function readAll(stream: NodeJS.ReadableStream | null): Promise<string> {
	if (!stream) return "";
	const chunks: Buffer[] = [];
	for await (const chunk of stream) {
		chunks.push(typeof chunk === "string" ? Buffer.from(chunk) : (chunk as Buffer));
	}
	return Buffer.concat(chunks).toString("utf8");
}

async function drain(stream: NodeJS.ReadableStream | null): Promise<void> {
	if (!stream) return;
	for await (const _chunk of stream) {
		// discard
	}
}

// ---------- provider mapping ----------

const PROVIDER_KEY_REFS: Record<string, string> = {
	openai: "keychain:omw/openai",
	anthropic: "keychain:omw/anthropic",
	"openai-compatible": "keychain:omw/openai-compatible",
};

// v0.1 cli.ts resolves API keys via the `key_ref` field from the loaded
// config (`getKeychainSecret(keyRef)`), so this factory is unused in
// production today. It exists for the v0.2 pi-agent contract: when the
// agent kernel is invoked without a config (e.g. inside the embedded
// terminal), it will receive a provider name and need to look up the key
// from a hard-coded mapping — exactly what this factory exposes.
export function makeGetApiKey(
	opts?: KeychainHelperOptions,
): (provider: string) => Promise<string | undefined> {
	return async (provider: string): Promise<string | undefined> => {
		if (provider === "ollama") {
			// Ollama does not need an API key; never spawn.
			return undefined;
		}
		const keyRef = PROVIDER_KEY_REFS[provider];
		if (!keyRef) {
			// Unknown provider: out of scope for v0.1. Pass through as
			// undefined rather than throwing; tests do not assert this path.
			return undefined;
		}
		return getKeychainSecret(keyRef, opts);
	};
}
