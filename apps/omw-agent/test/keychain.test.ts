// TS unit tests for apps/omw-agent/src/keychain.ts.
//
// These tests mock node:child_process.spawn so we can drive exit codes,
// stdout, stderr, and 'error' events deterministically. They never invoke
// the real helper binary.
//
// File-boundary note: tests in this file are owned by the Test Overseer
// under the TRD protocol. Implementation lives in src/keychain.ts and is
// authored by the Executor; nothing here may be edited from the impl side.

import { EventEmitter } from "node:events";
import { Readable } from "node:stream";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { assertNoSecretLeak, renderError } from "./_helpers.js";

// ---------- spawn mock plumbing ----------
//
// We hoist a `spawn` mock into the node:child_process module and feed it a
// queue of "scripted" child processes. Each test pushes the script it wants
// (stdout payload, stderr payload, exit code, optional 'error' event) before
// the call that triggers the spawn.
//
// IMPORTANT: the mock emits ONLY the 'close' event on completion, NOT
// 'exit'. The contract in test/README.md §1 says the impl waits for stdio
// to flush before resolving; in Node that means listening to 'close', not
// 'exit'. An impl that only listens to 'exit' would hang forever against
// this mock — which is the gate signal we want.

interface SpawnScript {
	stdout?: string;
	stderr?: string;
	exitCode?: number;
	errorEvent?: NodeJS.ErrnoException;
}

interface SpawnCall {
	command: string;
	args: readonly string[];
	options: { env?: NodeJS.ProcessEnv } | undefined;
}

const spawnQueue: SpawnScript[] = [];
const spawnCalls: SpawnCall[] = [];

function makeChildProcess(script: SpawnScript): EventEmitter {
	const child = new EventEmitter() as EventEmitter & {
		stdout: Readable;
		stderr: Readable;
	};
	child.stdout = Readable.from(script.stdout !== undefined ? [script.stdout] : []);
	child.stderr = Readable.from(script.stderr !== undefined ? [script.stderr] : []);

	// Emit either an 'error' event, or ONLY 'close' on next tick. We do NOT
	// emit 'exit' — see header comment for rationale (impl must use 'close').
	queueMicrotask(() => {
		if (script.errorEvent) {
			child.emit("error", script.errorEvent);
			return;
		}
		const code = script.exitCode ?? 0;
		child.emit("close", code, null);
	});

	return child;
}

vi.mock("node:child_process", () => {
	return {
		spawn: vi.fn((command: string, args?: readonly string[], options?: { env?: NodeJS.ProcessEnv }) => {
			spawnCalls.push({ command, args: args ?? [], options });
			const script = spawnQueue.shift() ?? { exitCode: 0 };
			return makeChildProcess(script);
		}),
	};
});

// Lazily import the module under test AFTER vi.mock is registered.
async function loadModule() {
	// Cache-busting: vitest resets module registry between tests when we
	// call vi.resetModules(); we re-import to get a fresh in-memory cache
	// so caching tests are independent.
	return await import("../src/keychain.js");
}

// ---------- shared test helpers ----------

function pushScript(s: SpawnScript) {
	spawnQueue.push(s);
}

beforeEach(() => {
	spawnQueue.length = 0;
	spawnCalls.length = 0;
	vi.resetModules();
});

afterEach(() => {
	vi.restoreAllMocks();
});

// ---------- tests ----------

describe("getKeychainSecret — spawn integration via mock", () => {
	it("1. resolves to trimmed secret on exit 0", async () => {
		pushScript({ stdout: "sk-test-secret\n", exitCode: 0 });
		const { getKeychainSecret } = await loadModule();
		const out = await getKeychainSecret("keychain:omw/openai");
		expect(out).toBe("sk-test-secret");
	});

	it("2. preserves internal newlines in multi-line secret", async () => {
		pushScript({ stdout: "line1\nline2\nline3\n", exitCode: 0 });
		const { getKeychainSecret } = await loadModule();
		const out = await getKeychainSecret("keychain:omw/multiline");
		expect(out).toBe("line1\nline2\nline3");
	});

	it("3. round-trips a Unicode secret", async () => {
		pushScript({ stdout: "sk-中文-测试\n", exitCode: 0 });
		const { getKeychainSecret } = await loadModule();
		const out = await getKeychainSecret("keychain:omw/unicode");
		expect(out).toBe("sk-中文-测试");
	});

	it("4. resolves to undefined when helper exits 1 (NotFound)", async () => {
		pushScript({ stdout: "", stderr: "not found", exitCode: 1 });
		const { getKeychainSecret } = await loadModule();
		const out = await getKeychainSecret("keychain:omw/never-set");
		expect(out).toBeUndefined();
	});

	it("5. throws KeychainHelperError with exitCode 2 on bad input", async () => {
		pushScript({ stdout: "", stderr: "invalid key_ref", exitCode: 2 });
		const { getKeychainSecret, KeychainHelperError } = await loadModule();
		try {
			await getKeychainSecret("not-a-keyref");
			throw new Error("expected throw");
		} catch (err) {
			expect(err).toBeInstanceOf(KeychainHelperError);
			expect((err as { exitCode: number }).exitCode).toBe(2);
		}
	});

	it("6. throws KeychainHelperError with exitCode 3 on backend unavailable", async () => {
		pushScript({ stdout: "", stderr: "OS keychain backend unavailable", exitCode: 3 });
		const { getKeychainSecret, KeychainHelperError } = await loadModule();
		await getKeychainSecret("keychain:omw/x", { backend: "os" }).then(
			() => {
				throw new Error("expected throw");
			},
			(err) => {
				expect(err).toBeInstanceOf(KeychainHelperError);
				expect(err.exitCode).toBe(3);
			},
		);
	});

	it("7. throws on spawn ENOENT", async () => {
		const enoent: NodeJS.ErrnoException = Object.assign(new Error("spawn ENOENT"), {
			code: "ENOENT",
		});
		pushScript({ errorEvent: enoent });
		const { getKeychainSecret } = await loadModule();
		await expect(
			getKeychainSecret("keychain:omw/openai", { binaryPath: "/no/such/binary" }),
		).rejects.toBeInstanceOf(Error);
	});

	it("17. trims EXACTLY one trailing newline (\"secret\\n\\n\" → \"secret\\n\")", async () => {
		// Regression guard: an impl using `trimEnd()` would strip BOTH trailing
		// newlines and return "secret", losing data. The contract in
		// test/README.md §2 says "exactly one" — use `replace(/\n$/, '')` or
		// equivalent.
		pushScript({ stdout: "secret\n\n", exitCode: 0 });
		const { getKeychainSecret } = await loadModule();
		const out = await getKeychainSecret("keychain:omw/double-newline");
		expect(out).toBe("secret\n");
	});

	it("18. argv shape is exactly ['get', keyRef]", async () => {
		// Tighter than tests 9/10 — we assert the LITERAL subcommand 'get' at
		// position 0 and the keyRef at position 1. An impl that omits 'get'
		// (e.g. just passes the keyRef) would still satisfy a `.toContain`
		// check, but the helper's CLI requires the subcommand.
		pushScript({ stdout: "anything\n", exitCode: 0 });
		const { getKeychainSecret } = await loadModule();
		await getKeychainSecret("keychain:omw/argv-shape");
		expect(spawnCalls.length).toBe(1);
		const args = spawnCalls[0].args;
		expect(args[0]).toBe("get");
		expect(args[1]).toBe("keychain:omw/argv-shape");
	});
});

describe("getKeychainSecret — caching", () => {
	it("8. second call with same keyRef does NOT re-spawn", async () => {
		pushScript({ stdout: "sk-cached\n", exitCode: 0 });
		const { getKeychainSecret } = await loadModule();
		const first = await getKeychainSecret("keychain:omw/openai");
		const second = await getKeychainSecret("keychain:omw/openai");
		expect(first).toBe("sk-cached");
		expect(second).toBe("sk-cached");
		expect(spawnCalls.length).toBe(1);
	});

	it("9. different keyRefs spawn separately", async () => {
		pushScript({ stdout: "sk-openai\n", exitCode: 0 });
		pushScript({ stdout: "sk-anthropic\n", exitCode: 0 });
		const { getKeychainSecret } = await loadModule();
		const a = await getKeychainSecret("keychain:omw/openai");
		const b = await getKeychainSecret("keychain:omw/anthropic");
		expect(a).toBe("sk-openai");
		expect(b).toBe("sk-anthropic");
		expect(spawnCalls.length).toBe(2);
		// Argv must differ on the keyRef AND each must include the 'get' subcommand.
		expect(spawnCalls[0].args[0]).toBe("get");
		expect(spawnCalls[0].args[1]).toBe("keychain:omw/openai");
		expect(spawnCalls[1].args[0]).toBe("get");
		expect(spawnCalls[1].args[1]).toBe("keychain:omw/anthropic");
	});

	it("16. cache key includes backend — same keyRef + different backend re-spawns", async () => {
		// Per the brief: cache key should be `${binaryPath}|${backend}|${keyRef}`.
		// The `memory` and `os` backends could yield different secrets, so they
		// MUST NOT share a cache slot.
		pushScript({ stdout: "secret-from-memory\n", exitCode: 0 });
		pushScript({ stdout: "secret-from-os\n", exitCode: 0 });
		const { getKeychainSecret } = await loadModule();
		const m = await getKeychainSecret("keychain:omw/openai", { backend: "memory" });
		const o = await getKeychainSecret("keychain:omw/openai", { backend: "os" });
		expect(m).toBe("secret-from-memory");
		expect(o).toBe("secret-from-os");
		expect(spawnCalls.length).toBe(2);
	});

	it("19. cache key includes binaryPath — same keyRef + different binaryPath re-spawns", async () => {
		// Two different binaries could be two different installs of the helper
		// (e.g. system vs. project-local) and could read different keychains.
		// They MUST NOT share a cache slot.
		pushScript({ stdout: "secret-from-binA\n", exitCode: 0 });
		pushScript({ stdout: "secret-from-binB\n", exitCode: 0 });
		const { getKeychainSecret } = await loadModule();
		const a = await getKeychainSecret("keychain:omw/openai", { binaryPath: "/path/A" });
		const b = await getKeychainSecret("keychain:omw/openai", { binaryPath: "/path/B" });
		expect(a).toBe("secret-from-binA");
		expect(b).toBe("secret-from-binB");
		expect(spawnCalls.length).toBe(2);
		expect(spawnCalls[0].command).toBe("/path/A");
		expect(spawnCalls[1].command).toBe("/path/B");
	});

	it("20. NotFound (undefined) is cached — second call to same ref does NOT re-spawn", async () => {
		// `undefined` is a legitimate value — it means "this provider has no
		// secret stored". Caching it avoids a fork/exec storm if the agent
		// asks repeatedly for an unset provider.
		pushScript({ stdout: "", stderr: "not found", exitCode: 1 });
		const { getKeychainSecret } = await loadModule();
		const first = await getKeychainSecret("keychain:omw/never-set");
		const second = await getKeychainSecret("keychain:omw/never-set");
		expect(first).toBeUndefined();
		expect(second).toBeUndefined();
		expect(spawnCalls.length).toBe(1);
	});

	it("21. Errors are NOT cached — second call after exit 2 spawns again", async () => {
		// Rationale: exit 2/3 indicate transient or fixable failures (bad
		// argv, backend unavailable). Caching them would poison the cache for
		// the lifetime of the process; better to retry. NotFound (exit 1) IS
		// cached because it's a stable answer, but failures are not.
		pushScript({ stdout: "", stderr: "invalid key_ref", exitCode: 2 });
		pushScript({ stdout: "", stderr: "invalid key_ref", exitCode: 2 });
		const { getKeychainSecret, KeychainHelperError } = await loadModule();
		await expect(getKeychainSecret("keychain:omw/transient")).rejects.toBeInstanceOf(
			KeychainHelperError,
		);
		await expect(getKeychainSecret("keychain:omw/transient")).rejects.toBeInstanceOf(
			KeychainHelperError,
		);
		expect(spawnCalls.length).toBe(2);
	});
});

describe("makeGetApiKey — provider mapping", () => {
	it("10a. openai maps to keychain:omw/openai", async () => {
		pushScript({ stdout: "sk-openai\n", exitCode: 0 });
		const { makeGetApiKey } = await loadModule();
		const getApiKey = makeGetApiKey();
		const out = await getApiKey("openai");
		expect(out).toBe("sk-openai");
		expect(spawnCalls.length).toBe(1);
		expect(spawnCalls[0].args[0]).toBe("get");
		expect(spawnCalls[0].args[1]).toBe("keychain:omw/openai");
	});

	it("10b. anthropic maps to keychain:omw/anthropic", async () => {
		pushScript({ stdout: "sk-ant-x\n", exitCode: 0 });
		const { makeGetApiKey } = await loadModule();
		const getApiKey = makeGetApiKey();
		const out = await getApiKey("anthropic");
		expect(out).toBe("sk-ant-x");
		expect(spawnCalls[0].args[0]).toBe("get");
		expect(spawnCalls[0].args[1]).toBe("keychain:omw/anthropic");
	});

	it("10c. openai-compatible maps to keychain:omw/openai-compatible", async () => {
		pushScript({ stdout: "sk-compat\n", exitCode: 0 });
		const { makeGetApiKey } = await loadModule();
		const getApiKey = makeGetApiKey();
		const out = await getApiKey("openai-compatible");
		expect(out).toBe("sk-compat");
		expect(spawnCalls[0].args[0]).toBe("get");
		expect(spawnCalls[0].args[1]).toBe("keychain:omw/openai-compatible");
	});

	it("11. ollama returns undefined without spawning", async () => {
		const { makeGetApiKey } = await loadModule();
		const getApiKey = makeGetApiKey();
		const out = await getApiKey("ollama");
		expect(out).toBeUndefined();
		expect(spawnCalls.length).toBe(0);
	});

	it("22. makeGetApiKey propagates binaryPath AND backend to spawn", async () => {
		// Factory-level options must reach the underlying spawn. A bug where
		// the factory swallows opts (e.g. `makeGetApiKey(_opts) { return
		// (p) => getKeychainSecret(map[p]) }` — note the missing opts
		// forwarding) would silently use defaults.
		pushScript({ stdout: "sk-custom\n", exitCode: 0 });
		const { makeGetApiKey } = await loadModule();
		const getApiKey = makeGetApiKey({ binaryPath: "/custom", backend: "memory" });
		const out = await getApiKey("openai");
		expect(out).toBe("sk-custom");
		expect(spawnCalls.length).toBe(1);
		expect(spawnCalls[0].command).toBe("/custom");
		const env = spawnCalls[0].options?.env ?? {};
		expect(env.OMW_KEYCHAIN_BACKEND).toBe("memory");
	});
});

describe("getKeychainSecret — option propagation", () => {
	it("12. backend option is propagated as OMW_KEYCHAIN_BACKEND env to spawn", async () => {
		pushScript({ stdout: "anything\n", exitCode: 0 });
		const { getKeychainSecret } = await loadModule();
		await getKeychainSecret("keychain:omw/openai", { backend: "memory" });
		expect(spawnCalls.length).toBe(1);
		const env = spawnCalls[0].options?.env ?? {};
		expect(env.OMW_KEYCHAIN_BACKEND).toBe("memory");
	});

	it("13. binaryPath option is used as the spawn command", async () => {
		pushScript({ stdout: "anything\n", exitCode: 0 });
		const { getKeychainSecret } = await loadModule();
		await getKeychainSecret("keychain:omw/openai", {
			binaryPath: "/custom/path/omw-keychain-helper",
		});
		expect(spawnCalls.length).toBe(1);
		expect(spawnCalls[0].command).toBe("/custom/path/omw-keychain-helper");
	});

	it("23. backend: 'auto' is propagated as OMW_KEYCHAIN_BACKEND=auto", async () => {
		// `auto` is the helper's default selection mode (memory in CI, OS on
		// macOS). An impl that filters out 'auto' as "no override" would
		// silently strip it from the spawn env — the helper would then
		// resolve to its own default, which is fine, but the caller asked
		// for explicit 'auto' and must get it.
		pushScript({ stdout: "anything\n", exitCode: 0 });
		const { getKeychainSecret } = await loadModule();
		await getKeychainSecret("keychain:omw/openai", { backend: "auto" });
		expect(spawnCalls.length).toBe(1);
		const env = spawnCalls[0].options?.env ?? {};
		expect(env.OMW_KEYCHAIN_BACKEND).toBe("auto");
	});
});

describe("secret hygiene", () => {
	it("14. KeychainHelperError never echoes a sentinel that appeared in stderr", async () => {
		// We use a sentinel that resembles a secret to verify the impl doesn't
		// fold stderr verbatim into error.message. Helper SHOULD never put a
		// secret in stderr, but the TS layer must defensively redact too.
		// Partial-prefix sweep: no substring of length >= 4 from the sentinel
		// may appear anywhere in the rendered error.
		const SECRET_SENTINEL = "super-secret-payload-12345";
		pushScript({
			stdout: "",
			// Simulate a buggy helper that leaks the sentinel into stderr.
			stderr: `internal failure: ${SECRET_SENTINEL}`,
			exitCode: 2,
		});
		const { getKeychainSecret } = await loadModule();
		try {
			await getKeychainSecret("keychain:omw/sensitive");
			throw new Error("expected throw");
		} catch (err) {
			const rendered = renderError(err);
			assertNoSecretLeak(rendered, SECRET_SENTINEL, 4);
		}
	});

	it("15. successful getKeychainSecret never calls console.* with the secret", async () => {
		const SECRET = "sk-leaky-but-not-leaked-XYZ";
		pushScript({ stdout: SECRET + "\n", exitCode: 0 });

		const consoleSpies = [
			vi.spyOn(console, "log").mockImplementation(() => {}),
			vi.spyOn(console, "warn").mockImplementation(() => {}),
			vi.spyOn(console, "error").mockImplementation(() => {}),
			vi.spyOn(console, "info").mockImplementation(() => {}),
			vi.spyOn(console, "debug").mockImplementation(() => {}),
		];

		const { getKeychainSecret } = await loadModule();
		const out = await getKeychainSecret("keychain:omw/openai");
		expect(out).toBe(SECRET); // sanity — secret really did flow through

		for (const spy of consoleSpies) {
			for (const call of spy.mock.calls) {
				const stringified = call.map((arg) => {
					try {
						return typeof arg === "string" ? arg : JSON.stringify(arg);
					} catch {
						return String(arg);
					}
				}).join(" ");
				assertNoSecretLeak(stringified, SECRET, 4);
			}
		}
	});

	it("24. error path never calls console.* with a sentinel from stderr", async () => {
		// Mirror of test 15 but on the failure path. A buggy impl that does
		// `console.error(stderr)` to "help debugging" would leak whatever
		// the helper wrote (and a misbehaving helper might write a secret).
		// Partial-prefix sweep at length >= 4.
		const SENTINEL = "super-secret-stderr-payload-67890";
		pushScript({
			stdout: "",
			stderr: `something went wrong: ${SENTINEL}`,
			exitCode: 2,
		});

		const consoleSpies = [
			vi.spyOn(console, "log").mockImplementation(() => {}),
			vi.spyOn(console, "warn").mockImplementation(() => {}),
			vi.spyOn(console, "error").mockImplementation(() => {}),
			vi.spyOn(console, "info").mockImplementation(() => {}),
			vi.spyOn(console, "debug").mockImplementation(() => {}),
		];

		const { getKeychainSecret, KeychainHelperError } = await loadModule();
		await expect(getKeychainSecret("keychain:omw/sensitive")).rejects.toBeInstanceOf(
			KeychainHelperError,
		);

		for (const spy of consoleSpies) {
			for (const call of spy.mock.calls) {
				const stringified = call.map((arg) => {
					try {
						return typeof arg === "string" ? arg : JSON.stringify(arg);
					} catch {
						return String(arg);
					}
				}).join(" ");
				assertNoSecretLeak(stringified, SENTINEL, 4);
			}
		}
	});
});
