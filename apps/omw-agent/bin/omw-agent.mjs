#!/usr/bin/env node
// Production entry point for the `omw-agent` binary.
//
// Two modes:
//
// 1. `omw-agent ask <prompt> [...flags]` — v0.1 direct-fetch streaming for
//    `omw ask`. Backed by `runCli` from `dist/src/cli.js`.
// 2. `omw-agent --serve-stdio` — Phase 1 JSON-RPC stdio server backed by
//    pi-agent-core. Used by omw-server to multiplex agent sessions.
//
// Mode is selected by argv[0]. Anything that isn't `--serve-stdio` falls
// through to the CLI dispatcher (which itself rejects unknown flags).

const argv = process.argv.slice(2);

if (argv[0] === "--serve-stdio") {
	const { runStdioServer } = await import("../dist/src/serve.js");
	const { getKeychainSecret } = await import("../dist/src/keychain.js");

	await runStdioServer({
		stdin: process.stdin,
		stdout: process.stdout,
		stderr: process.stderr,
		// Resolve key_refs straight through the helper bridge; keys never
		// transit any frame on the JSON-RPC surface.
		getApiKey: (keyRef) => getKeychainSecret(keyRef),
	});
	// runStdioServer returns when stdin closes. Exit cleanly.
	const flush = (s) => new Promise((res) => s.write("", res));
	await flush(process.stdout);
	await flush(process.stderr);
	process.exit(0);
}

const { runCli } = await import("../dist/src/cli.js");

const code = await runCli(argv, process.env, {
	stdout: process.stdout,
	stderr: process.stderr,
});
// Drain stdout/stderr before exit so the final usage JSON line on stderr
// (and any tail of streamed text on stdout) is not truncated under pipe.
const flush = (s) => new Promise((res) => s.write("", res));
await flush(process.stdout);
await flush(process.stderr);
process.exit(code);
