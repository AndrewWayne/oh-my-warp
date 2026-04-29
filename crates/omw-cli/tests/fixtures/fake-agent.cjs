#!/usr/bin/env node
// Fake `omw-agent` binary used by `crates/omw-cli/tests/cli_ask.rs`.
//
// File-boundary note: this fixture is owned by the Test Overseer under the
// TRD protocol. The Executor MUST NOT modify it.
//
// Behavior: emit a single JSON object on stdout describing exactly what we
// were spawned with so the integration tests can assert the omw-cli `ask`
// subcommand forwards prompt, flags, and env correctly.
//
// The shape is:
//   {
//     "argv": [ ...process.argv.slice(2) ],   // args the Rust SUT passed
//     "env": {                                 // env vars the SUT propagated
//       "OMW_CONFIG": "...",
//       "OMW_KEYCHAIN_HELPER": "...",
//       "OMW_KEYCHAIN_BACKEND": "..."
//     }
//   }
//
// We deliberately limit the env keys we echo so the test assertions don't
// depend on the rest of the parent env block (PATH, HOME, etc.).
//
// Modes:
//   - default: write JSON payload to stdout. Also write a usage-record
//              JSON line as the LAST line of stderr (the cost-telemetry
//              tests require this; the agent contract is that the final
//              stderr line is parseable usage JSON). Exit 0.
//   - fail:    write a fixed line to stderr, exit 42. Used by the Rust
//              gate that asserts the SUT propagates child stderr to parent
//              stderr AND propagates the child's nonzero exit code.
//              Triggered by either argv `--mode=fail` (the wrapper would
//              have to forward it) or env var `FAKE_AGENT_MODE=fail`. We
//              prefer the env trigger because the Rust SUT controls the
//              parent env directly without needing to inject argv.
//
// Usage line shape (default mode, on stderr):
//   {"prompt_tokens":N,"completion_tokens":N,"total_tokens":N,
//    "provider":"...","model":"...","duration_ms":N}
// The provider and model strings can be overridden via FAKE_AGENT_PROVIDER
// and FAKE_AGENT_MODEL env vars; tests that don't care let them default.

"use strict";

const argv = process.argv.slice(2);
const failMode =
	process.env.FAKE_AGENT_MODE === "fail" ||
	argv.includes("--mode=fail");

if (failMode) {
	process.stderr.write("fake stderr line\n");
	process.exit(42);
}

const echoEnvKeys = [
	"OMW_CONFIG",
	"OMW_KEYCHAIN_HELPER",
	"OMW_KEYCHAIN_BACKEND",
	"OMW_AGENT_PROBE",
];

const echoedEnv = {};
for (const k of echoEnvKeys) {
	if (process.env[k] !== undefined) {
		echoedEnv[k] = process.env[k];
	}
}

const payload = {
	argv,
	env: echoedEnv,
};

process.stdout.write(JSON.stringify(payload) + "\n");

// Emit a usage-record JSON object as the LAST line on stderr. The omw-cli
// `ask` handler captures the final stderr line and parses it as the
// telemetry payload. Older callers that didn't read stderr keep working
// (they only inspected stdout). Tests can override provider/model via env.
const usage = {
	prompt_tokens: 10,
	completion_tokens: 20,
	total_tokens: 30,
	provider: process.env.FAKE_AGENT_PROVIDER || "test",
	model: process.env.FAKE_AGENT_MODEL || "test-model",
	duration_ms: 100,
};
process.stderr.write(JSON.stringify(usage) + "\n");

process.exit(0);
