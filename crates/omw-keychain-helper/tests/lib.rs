//! In-process integration tests for `omw-keychain-helper`.
//!
//! File-boundary note: this file is owned by the Test Overseer under the
//! TRD protocol. The Executor MUST NOT modify it.
//!
//! ## Why this file exists
//!
//! The original `tests/cli.rs` suite spawns the helper as a subprocess via
//! `assert_cmd`. That covers exit codes, stderr shape, and the binary
//! entry point — but it CANNOT cover the exit-0 success path, because the
//! `memory` backend is per-process: a value `set` in the parent test
//! process is invisible to the spawned child. (And the OS backend is not
//! available in CI on Linux/Windows.)
//!
//! These tests close that gap by calling the helper's main logic AS A
//! LIBRARY FUNCTION, in-process. We seed a value via `omw_keychain::set`
//! in the same process that runs the `get` logic, so the in-memory backend
//! sees it.
//!
//! ## Contract this imposes on the Executor
//!
//! The Executor MUST refactor `crates/omw-keychain-helper/Cargo.toml` to
//! be both a `[lib]` and a `[[bin]]` crate (or use the default-library +
//! `src/main.rs` layout). The library MUST expose:
//!
//! ```rust,ignore
//! pub fn run(
//!     args: &[String],
//!     envs: &std::collections::HashMap<String, String>,
//!     stdout: &mut dyn std::io::Write,
//!     stderr: &mut dyn std::io::Write,
//! ) -> i32;
//! ```
//!
//! - `args` is argv WITHOUT argv[0] (i.e. ["get", "keychain:omw/foo"]).
//! - `envs` carries the keys the helper reads (notably
//!   `OMW_KEYCHAIN_BACKEND`). The library MAY also read the real process
//!   env if a key is absent from `envs`; the test sets the process env to
//!   `memory` so either policy works.
//! - `stdout` / `stderr` are sinks — the library MUST NOT touch the
//!   process's real stdio inside `run()` (so we can capture them in
//!   buffers).
//! - Return value is the exit code the binary would have produced.
//!
//! The binary in `src/main.rs` should be a thin wrapper that collects
//! `std::env::args()` and `std::env::vars()`, calls `run()`, and exits with
//! the returned code.
//!
//! ## Gate signal
//!
//! If the Executor opts for a binary-only crate (no `[lib]`) or names the
//! library function differently, this file FAILS TO COMPILE. That failure
//! is the gate signal — it tells the Executor the contract is missing.
//!
//! Note: this file references the crate as `omw_keychain_helper` and the
//! sibling crate `omw_keychain` (the in-memory + OS-backend abstraction).
//! Both must be reachable as library crates from the dev-dependencies.
//!
//! ## Executor checklist (DO BEFORE THIS FILE COMPILES)
//!
//! 1. Add `"crates/omw-keychain-helper"` to `members` in the root
//!    `Cargo.toml`'s `[workspace]` table.
//! 2. Create `crates/omw-keychain-helper/Cargo.toml` with:
//!      - `[lib]` `path = "src/lib.rs"`
//!      - `[[bin]]` `name = "omw-keychain-helper"`, `path = "src/main.rs"`
//!      - `[dependencies]`: `omw-config = { path = "../omw-config" }`,
//!        `omw-keychain = { path = "../omw-keychain" }`
//!      - `[dev-dependencies]`: `assert_cmd = "2"`,
//!        `omw-config = { path = "../omw-config" }`,
//!        `omw-keychain = { path = "../omw-keychain" }`
//!    (If `assert_cmd` is added to `[workspace.dependencies]`, dev-deps may
//!    use `.workspace = true` instead of pinning the version here.)
//! 3. Expose in `src/lib.rs`:
//!    `pub fn run(args: &[String], envs: &HashMap<String, String>,
//!     stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32`
//! 4. `src/main.rs` is a thin wrapper: collect `std::env::args()` (skipping
//!    argv[0]) and `std::env::vars()`, call `run()`, then `std::process::exit`.

use std::collections::HashMap;
use std::io::Cursor;

// The helper's library entrypoint. The Executor must expose this.
use omw_keychain_helper::run;
// `omw_keychain` is needed to seed values into the in-memory backend in this
// same process; `omw_config::KeyRef` is the typed key the seeding API accepts.
// Both must be path-deps in `[dev-dependencies]` per the checklist above.
use omw_config::KeyRef;

/// Build an envs map that opts into the in-memory backend. We also set
/// the process env, so a `run()` impl that reads process env directly
/// (instead of from `envs`) still sees the right backend.
fn memory_env() -> HashMap<String, String> {
    let mut envs = HashMap::new();
    envs.insert("OMW_KEYCHAIN_BACKEND".to_string(), "memory".to_string());
    // Also set on the process itself for impls that read process env.
    // SAFETY: tests in this file run sequentially within their own
    // integration-test binary; cargo runs each #[test] on a thread, but
    // we serialize via the Rust test harness's --test-threads default
    // when the test imports module-global state. For belt-and-braces,
    // we always re-set this here.
    std::env::set_var("OMW_KEYCHAIN_BACKEND", "memory");
    envs
}

#[test]
fn t10_get_existing_key_succeeds_with_exit_0_and_trailing_newline() {
    // Seed a value into the in-memory backend in THIS process. The helper's
    // `run()` then reads from the same in-process backend.
    //
    // The Executor's `omw-keychain` crate must expose a `set(key_ref,
    // value)` function that writes to the in-memory backend. If that name
    // differs, the Executor will need to adjust this test's seeding call —
    // but the seeding helper is part of the Overseer-owned contract and
    // SHOULD remain on `omw_keychain::set`.
    let envs = memory_env();
    let kr: KeyRef = "keychain:omw/lib-test-1"
        .parse()
        .expect("KeyRef should parse");
    omw_keychain::set(&kr, "the-secret-value")
        .expect("seeding the in-memory backend should succeed");

    let mut stdout = Cursor::new(Vec::<u8>::new());
    let mut stderr = Cursor::new(Vec::<u8>::new());

    let args: Vec<String> = vec!["get".into(), "keychain:omw/lib-test-1".into()];
    let code = run(&args, &envs, &mut stdout, &mut stderr);

    assert_eq!(code, 0, "expected exit 0, stderr={:?}", String::from_utf8_lossy(stderr.get_ref()));
    let stdout_bytes = stdout.into_inner();
    let stdout_str = String::from_utf8(stdout_bytes).expect("stdout should be valid UTF-8");
    assert_eq!(
        stdout_str, "the-secret-value\n",
        "stdout should be value + exactly one trailing newline",
    );
}

#[test]
fn t11_get_existing_key_with_unicode_value() {
    // Round-trip a Unicode secret. Catches impls that mis-handle byte
    // boundaries when adding the trailing newline.
    let envs = memory_env();
    let kr: KeyRef = "keychain:omw/lib-test-unicode"
        .parse()
        .expect("KeyRef should parse");
    omw_keychain::set(&kr, "sk-中文-测试").expect("seeding should succeed");

    let mut stdout = Cursor::new(Vec::<u8>::new());
    let mut stderr = Cursor::new(Vec::<u8>::new());

    let args: Vec<String> = vec!["get".into(), "keychain:omw/lib-test-unicode".into()];
    let code = run(&args, &envs, &mut stdout, &mut stderr);

    assert_eq!(code, 0);
    let stdout_str = String::from_utf8(stdout.into_inner()).expect("UTF-8");
    assert_eq!(stdout_str, "sk-中文-测试\n");
}

#[test]
fn t12_get_missing_key_returns_exit_1_with_empty_stdout() {
    // Confirms the in-process path agrees with the subprocess path
    // (cli.rs t1) on NotFound. Catches divergence between `run()` and
    // the binary wrapper.
    let envs = memory_env();
    let mut stdout = Cursor::new(Vec::<u8>::new());
    let mut stderr = Cursor::new(Vec::<u8>::new());

    let args: Vec<String> = vec!["get".into(), "keychain:omw/lib-test-missing".into()];
    let code = run(&args, &envs, &mut stdout, &mut stderr);

    assert_eq!(code, 1);
    assert!(stdout.into_inner().is_empty(), "stdout must be empty on NotFound");
    let stderr_str = String::from_utf8(stderr.into_inner()).expect("UTF-8");
    assert!(
        stderr_str.to_lowercase().contains("not found"),
        "expected 'not found' in stderr, got {:?}",
        stderr_str,
    );
}

/// Sweep every substring of `secret` of length >= `min_window`; assert
/// none appear in `rendered`. Mirrors the TS `assertNoSecretLeak`.
fn assert_no_secret_leak(rendered: &str, secret: &str, min_window: usize) {
    let chars: Vec<char> = secret.chars().collect();
    if chars.len() < min_window {
        assert!(
            !rendered.contains(secret),
            "leak: full secret {:?} in {:?}",
            secret,
            rendered,
        );
        return;
    }
    for start in 0..=(chars.len() - min_window) {
        for end in (start + min_window)..=chars.len() {
            let window: String = chars[start..end].iter().collect();
            assert!(
                !rendered.contains(&window),
                "leak: window {:?} (len {}) from secret {:?} found in {:?}",
                window,
                window.chars().count(),
                secret,
                rendered,
            );
        }
    }
}

#[test]
fn t13_secret_value_never_appears_in_stderr_on_success() {
    // The success path emits the secret on stdout (by design). It MUST NOT
    // also dribble into stderr (e.g. via a debug log). Partial-prefix sweep
    // at length >= 4 catches truncated leaks.
    let envs = memory_env();
    let secret = "ultra-secret-stderr-check-payload";
    let kr: KeyRef = "keychain:omw/lib-test-stderr-hygiene"
        .parse()
        .expect("KeyRef should parse");
    omw_keychain::set(&kr, secret).expect("seeding should succeed");

    let mut stdout = Cursor::new(Vec::<u8>::new());
    let mut stderr = Cursor::new(Vec::<u8>::new());

    let args: Vec<String> = vec!["get".into(), "keychain:omw/lib-test-stderr-hygiene".into()];
    let code = run(&args, &envs, &mut stdout, &mut stderr);

    assert_eq!(code, 0);
    let stderr_str = String::from_utf8(stderr.into_inner()).expect("UTF-8");
    assert_no_secret_leak(&stderr_str, secret, 4);
}
