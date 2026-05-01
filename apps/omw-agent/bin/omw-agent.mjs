#!/usr/bin/env node
// Production entry point for the `omw-agent` binary that backs `omw ask`.
//
// Loads the compiled `runCli` from `dist/` and invokes it with the real
// process argv/env/stdio. Exits with the code returned by `runCli`.

import { runCli } from "../dist/src/cli.js";

const code = await runCli(process.argv.slice(2), process.env, {
	stdout: process.stdout,
	stderr: process.stderr,
});
// Drain stdout/stderr before exit so the final usage JSON line on stderr
// (and any tail of streamed text on stdout) is not truncated under pipe.
const flush = (s) => new Promise((res) => s.write("", res));
await flush(process.stdout);
await flush(process.stderr);
process.exit(code);
