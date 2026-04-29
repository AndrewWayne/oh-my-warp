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
process.exit(code);
