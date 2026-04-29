# apps/omw-agent — test contract for the Executor

This directory was authored by the **Test Overseer** under the TRD protocol.
The Overseer owns these tests; the Executor MUST NOT modify them.

## Files the Executor must create before tests can run

- `apps/omw-agent/package.json` — declares `vitest` as a devDependency, plus
  any production deps (`@types/node`). Suggested `scripts`:
  ```jsonc
  {
    "scripts": {
      "test": "vitest run",
      "test:watch": "vitest"
    }
  }
  ```
  Set `"type": "module"` so the ESM imports in tests resolve.

- `apps/omw-agent/tsconfig.json` — strict TS config that supports vitest
  (target ES2022 or later, module NodeNext, moduleResolution NodeNext).

- `apps/omw-agent/src/keychain.ts` — the production module under test. Must
  export the public API documented in the TRD brief:
  - `KeychainHelperOptions` interface with `binaryPath?` and
    `backend?: 'memory' | 'os' | 'auto'`.
  - `KeychainHelperError` class extending `Error` with `exitCode: number`.
  - `getKeychainSecret(keyRef, opts?) → Promise<string | undefined>`.
  - `makeGetApiKey(opts?) → (provider: string) => Promise<string | undefined>`.

- `apps/omw-agent/src/cli.ts` — the agent CLI behind `omw ask`. Must export:
  ```ts
  export interface RunCliOptions {
    stdout: NodeJS.WritableStream;
    stderr: NodeJS.WritableStream;
    fetchImpl?: typeof fetch;
    getKeychainSecretImpl?: (keyRef: string) => Promise<string | undefined>;
  }
  export async function runCli(
    argv: string[],            // argv WITHOUT node and script (e.g. ["ask", "hi"])
    env: Record<string, string>,
    opts: RunCliOptions,
  ): Promise<number>;          // exit code
  ```
  The bin entry (a `#!/usr/bin/env node` script) is what the Executor wires up
  to invoke `runCli(process.argv.slice(2), process.env, { stdout: process.stdout, stderr: process.stderr })`.

  Tests inject `fetchImpl` and `getKeychainSecretImpl` so no global
  monkey-patching is required and tests stay parallel-safe.

## Behavioral contract the tests assert

1. **Async spawn only.** `getKeychainSecret` MUST use `child_process.spawn`
   (not `spawnSync`) so the agent's event loop never blocks. Tests mock
   `node:child_process` and assert `spawn` is the entry point.

2. **Exit-code mapping.**
   - Exit 0 → resolve `stdout` with **one** trailing newline trimmed (and
     exactly one — internal newlines must round-trip).
   - Exit 1 → resolve `undefined` (NotFound).
   - Exit 2 / Exit 3 → reject with `KeychainHelperError` whose `exitCode`
     matches the helper's exit code.
   - Spawn `error` event (e.g. `ENOENT`) → reject (KeychainHelperError or
     the underlying error class — tests accept either, but the message must
     be present and must not contain any secret material).

3. **Caching.** `getKeychainSecret` caches by a key that includes
   `binaryPath`, `backend`, and `keyRef`. Two calls with identical inputs
   spawn once; differing in any of those three keys spawn separately.
   `undefined` (NotFound) and successful values are both cached. Errors are
   NOT cached (so a transient failure does not poison the cache).

4. **Provider mapping (`makeGetApiKey`).**
   - `openai` → `keychain:omw/openai`
   - `anthropic` → `keychain:omw/anthropic`
   - `openai-compatible` → `keychain:omw/openai-compatible`
   - `ollama` → `undefined` (no spawn)
   - Unknown providers — out of scope for v0.1; the Executor MAY throw,
     return `undefined`, or pass through. Tests do not assert this.

5. **Secret hygiene.**
   - Secret values MUST NOT appear in `KeychainHelperError.message`,
     `error.toString()`, or `JSON.stringify(error)`.
   - Secret values MUST NOT pass through `console.log/warn/error/info/debug`.
   - The cache lives in-process memory only; nothing is written to disk.

6. **Spawn `options.env` propagation.** When `backend` is supplied, the
   spawn `env` must contain `OMW_KEYCHAIN_BACKEND=<backend>`. When
   `binaryPath` is supplied, it MUST be the spawn command (first arg);
   otherwise the binary is resolved via `process.env.OMW_KEYCHAIN_HELPER`
   then `omw-keychain-helper` on `PATH`.

## Running the tests

```bash
cd apps/omw-agent
npm install
npm test
```

Integration tests live in `keychain.integration.test.ts` and skip themselves
unless `OMW_KEYCHAIN_HELPER` points at a real, existing helper binary. The
Executor does not need to wire CI for them in v0.1; they are a smoke test
for local development against a built helper.
