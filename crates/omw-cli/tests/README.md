# omw-cli tests — TRD overseer-owned

These tests are owned by the **Test Overseer** under the project's
Test-Reinforced Development protocol. The Executor agent (who writes
`crates/omw-cli/src/{lib.rs,main.rs}` and the `[dependencies]` section of
`Cargo.toml`) MUST NOT modify any file under `tests/`.

## Executor checklist

Before these tests will compile, let alone pass, the Executor must:

1. Make `crates/omw-cli/Cargo.toml` a `[lib]` + `[[bin]]` crate.
   - `[lib]` `path = "src/lib.rs"`
   - `[[bin]]` `name = "omw"`, `path = "src/main.rs"`
2. Expose the entry point from `src/lib.rs`:

   ```rust
   pub fn run(
       args: &[String],
       stdout: &mut dyn std::io::Write,
       stderr: &mut dyn std::io::Write,
   ) -> i32;
   ```

   - `args` is argv WITHOUT argv[0] (e.g. `["provider", "list"]`).
   - Return value is the exit code the binary wrapper would `exit()` with.
   - The library MUST NOT touch the process's real stdio inside `run()`.
3. `src/main.rs` is a thin wrapper: collect `std::env::args()` (skipping
   argv[0]), call `omw_cli::run`, then `std::process::exit` with the code.
4. Add `[dependencies]`: `clap`, `toml_edit` (NOT `toml` — comment
   preservation is a tested invariant), `anyhow`,
   `omw-config = { path = "../omw-config" }`,
   `omw-keychain = { path = "../omw-keychain" }`. You may pin `clap`,
   `toml_edit`, `anyhow` in the root `[workspace.dependencies]`.
5. The interactive `add` path (no `--key`, no `--from-stdin`, no
   `--non-interactive`) needs a key prompt — but every test in this suite
   passes `--non-interactive` and `--key`, so a `todo!()` or a clean error
   is fine for v0.1.

## Why both subprocess and in-process tests

`tests/common/mod.rs` provides two ways to drive the CLI:

- **`omw_cmd(temp_dir)` — subprocess**: spawns the cargo-built `omw`
  binary via `assert_cmd`. Covers exit codes, argv plumbing, end-to-end
  shape.
- **`lib_mode_run(args)` — in-process**: calls `omw_cli::run` as a library
  function with captured stdout/stderr.

The split is forced by a real constraint:
**`OMW_KEYCHAIN_BACKEND=memory` is per-process.** A subprocess
`omw provider add foo --key sk-x` writes to its own per-process memory
store, which is gone the moment that subprocess exits. So a follow-up
subprocess `omw provider list` sees a fresh, empty store — making it
impossible to verify "key was actually stored" or "list shows the
`stored` status" via subprocess alone.

In-process tests close that gap by running both calls in the same
process (the test binary), where the memory backend persists.

## Gate signal

If the Executor ships a binary-only crate (no `[lib]`), or names the
library function differently, `tests/common/mod.rs` and the test files
that import `omw_cli::run` will fail to compile. That compile failure is
the gate signal — it says "the contract is missing, fix the crate
layout."

## Test inventory

`cli_provider.rs`:

1. `provider_list_on_empty_config`
2. `provider_list_with_one_missing_key`
3. `provider_list_with_default_marked`
4. `provider_list_with_multiple_providers_stable_order`
5. `provider_add_openai_non_interactive` (in-process; verifies keychain)
6. `provider_add_openai_compatible_without_base_url_fails`
7. `provider_add_ollama_no_key`
8. `provider_add_invalid_id`
9. `provider_add_existing_fails_without_force`
10. `provider_add_creates_missing_config_with_version_1`
11. `provider_add_preserves_comments` (gates `toml_edit`)
12. `provider_add_make_default`
13. `provider_remove_existing_with_yes` (in-process)
14. `provider_remove_nonexistent_fails`
15. `provider_remove_clears_default_provider`
16. `provider_add_does_not_echo_secret_to_stdout_or_stderr`

`cli_config.rs`:

17. `config_path_honors_omw_config_env`
18. `config_path_default_xdg_home`
19. `config_show_empty_config`
20. `config_show_with_providers_no_secret_leak` (in-process)

`cli_ask.rs`:

21. `ask_requires_prompt_arg`
22. `ask_passes_prompt_to_omw_agent_bin`
23. `ask_passes_provider_and_model_flags`
24. `ask_passes_through_environment`

24 tests total. ≥ 18 satisfied.

### `omw ask` contract for the Executor

`cli_ask.rs` exercises the SPAWN SURFACE of `omw ask <prompt>`. The Rust
half (this crate) is responsible for:

- accepting `<prompt>` (required positional) and the optional flags
  `--provider <id>`, `--model <m>`, `--max-tokens <n>`, `--temperature <f>`,
- locating an `omw-agent` executable via the `OMW_AGENT_BIN` env var
  (with some sensible default for production — `omw-agent` on PATH — that
  tests do NOT exercise),
- spawning that executable with `ask` as the first arg followed by the
  prompt and any forwarded flags, and forwarding the parent's
  `OMW_CONFIG`, `OMW_KEYCHAIN_HELPER`, and `OMW_KEYCHAIN_BACKEND` env
  vars to the child,
- streaming the child's stdout to the parent's stdout, the child's
  stderr to the parent's stderr, and exiting with the child's exit
  code (or non-zero on spawn failure).

The actual provider streaming, keychain resolution, and usage telemetry
all live in the TS half (`apps/omw-agent/src/cli.ts`) and are tested by
`apps/omw-agent/test/cli.test.ts`.

The fake agent at `tests/fixtures/fake-agent.cjs` is a plain Node script
that JSON-prints its argv + a whitelist of inherited env vars to stdout
and exits 0 — the Rust tests use it to verify the spawn payload. A small
`.cmd` (Windows) or `.sh` (Unix) wrapper is generated at test runtime so
`OMW_AGENT_BIN` can be a single executable path regardless of the host
shell.
