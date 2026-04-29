# `omw-keychain-helper` test plan (Overseer-owned)

This directory is **Test Overseer territory** under the TRD protocol. The
Executor implements `src/lib.rs` and `src/main.rs` to make these tests pass;
they MUST NOT modify the test files themselves.

## Files

- `cli.rs` — out-of-process CLI tests. Spawns the built binary via
  `assert_cmd`. Covers exit codes, stderr shape, `--help`, and the
  backend-unavailable path (Linux/Windows only).
- `lib.rs` — in-process tests for the success path. Calls
  `omw_keychain_helper::run()` directly so the in-memory keychain backend is
  shared between the seeding call and the helper's read.

## Executor checklist (REQUIRED before these tests compile or run)

1. **Workspace registration.** Add `"crates/omw-keychain-helper"` to the
   `members` list in the **root** `Cargo.toml`'s `[workspace]` table.
   Without this, `cargo test --workspace --all-targets` will not pick up the
   crate.

2. **Create `crates/omw-keychain-helper/Cargo.toml`** with both a library
   and a binary target:

   ```toml
   [package]
   name = "omw-keychain-helper"
   version = "0.0.0"
   edition.workspace = true
   rust-version.workspace = true
   license.workspace = true
   repository.workspace = true
   publish = false

   [lib]
   path = "src/lib.rs"

   [[bin]]
   name = "omw-keychain-helper"
   path = "src/main.rs"

   [dependencies]
   omw-config   = { path = "../omw-config" }
   omw-keychain = { path = "../omw-keychain" }

   [dev-dependencies]
   assert_cmd   = "2"
   omw-config   = { path = "../omw-config" }
   omw-keychain = { path = "../omw-keychain" }
   ```

   If `assert_cmd` is promoted to `[workspace.dependencies]` in the root
   `Cargo.toml`, the dev-dep here can switch to `assert_cmd.workspace = true`.

3. **Library contract** — `src/lib.rs` MUST expose:

   ```rust
   pub fn run(
       args: &[String],
       envs: &std::collections::HashMap<String, String>,
       stdout: &mut dyn std::io::Write,
       stderr: &mut dyn std::io::Write,
   ) -> i32;
   ```

   - `args` is argv WITHOUT argv[0] (e.g. `["get", "keychain:omw/foo"]`).
   - `envs` carries the keys the helper reads (notably
     `OMW_KEYCHAIN_BACKEND`). The library MAY also fall back to
     `std::env::var` if a key is absent from `envs` — `lib.rs` sets the
     process env to the same value to keep both policies happy.
   - `stdout` / `stderr` are sinks. The library MUST NOT write to the real
     process stdio inside `run()` so the in-process tests can capture both
     streams into buffers.
   - The return value is the exit code the binary would have produced.

4. **Binary wrapper** — `src/main.rs` is a thin shim: collect
   `std::env::args().skip(1)` and `std::env::vars()`, call
   `omw_keychain_helper::run`, then `std::process::exit` with the returned
   code. No business logic in `main.rs`.

## Gate signal

If the Executor produces a binary-only crate (no `[lib]`), forgets the
workspace `members` entry, or names the entrypoint anything other than
`run`, **the test harness fails to compile**. That compile failure is the
gate — it indicates the contract above has not been met.
