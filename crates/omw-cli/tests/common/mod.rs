//! Shared test helpers for `omw-cli` integration tests.
//!
//! File-boundary note: this module is owned by the Test Overseer under the
//! TRD protocol. The Executor MUST NOT modify it.
//!
//! ## Two ways tests talk to the CLI
//!
//! 1. **Subprocess** (`omw_cmd`): spawns the cargo-built `omw` binary via
//!    `assert_cmd`. This covers exit codes, stderr/stdout shape, and the
//!    binary's argv plumbing.
//!
//! 2. **In-process** (`lib_mode_run`): calls `omw_cli::run` as a library
//!    function. This is REQUIRED to test the success path of any code that
//!    touches the keychain — the `memory` backend is per-process, so a
//!    `set` in the parent test process is invisible to a child subprocess.
//!
//! ## Contract this imposes on the Executor
//!
//! `crates/omw-cli/Cargo.toml` MUST be both `[lib]` and `[[bin]]`. The
//! library MUST expose:
//!
//! ```rust,ignore
//! pub fn run(
//!     args: &[String],
//!     stdout: &mut dyn std::io::Write,
//!     stderr: &mut dyn std::io::Write,
//! ) -> i32;
//! ```
//!
//! - `args` is argv WITHOUT argv[0] (i.e. `["provider", "list"]`).
//! - Exit code is the i32 a binary wrapper would `exit()` with.
//! - The library MUST NOT touch the process's real stdio — write only to
//!   the provided sinks (so tests can capture into buffers).
//!
//! The binary `src/main.rs` is a thin wrapper that collects
//! `std::env::args()` (skipping argv[0]), calls `run()`, and exits.
//!
//! ## Gate signal
//!
//! If the Executor ships a binary-only crate (no `[lib]`) or uses a
//! different signature, this module FAILS TO COMPILE. That's the gate.

#![allow(dead_code)]

use std::io::Cursor;
use std::path::Path;
use std::process::Command;
use std::sync::{Mutex, MutexGuard, OnceLock};

use assert_cmd::Command as AssertCommand;

/// Process-wide serialization lock for tests that mutate the global process
/// environment (`OMW_CONFIG`, `OMW_KEYCHAIN_BACKEND`, ...) and then call
/// `lib_mode_run`. Cargo runs integration tests in parallel by default; two
/// such tests racing on `set_var` can interleave so test A's `lib_mode_run`
/// sees test B's `OMW_CONFIG`.
///
/// Subprocess tests (`omw_cmd`) do NOT need this — each subprocess has its
/// own env block.
///
/// Usage at the top of every in-process test:
/// ```rust,ignore
/// let _g = common::env_lock();
/// ```
///
/// We intentionally swallow `PoisonError` (`.unwrap_or_else(|e| e.into_inner())`)
/// so a panic in test A doesn't poison the lock and cascade-fail every other
/// in-process test. Each test sets its own `OMW_CONFIG` before reading, so a
/// crashed sibling's leftover env var is overwritten on entry.
pub fn env_lock() -> MutexGuard<'static, ()> {
    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    ENV_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// Build a clean `omw` subprocess command pointed at a temp config dir.
///
/// Clears the parent env so OMW_CONFIG / XDG_CONFIG_HOME / OMW_KEYCHAIN_BACKEND
/// resolve to the values we set explicitly. PATH is preserved so the dynamic
/// linker still works on platforms that need it.
pub fn omw_cmd(temp_dir: &Path) -> AssertCommand {
    let mut cmd =
        AssertCommand::cargo_bin("omw").expect("omw binary should be built by cargo test");
    cmd.env_clear();
    cmd.env("OMW_CONFIG", temp_dir.join("config.toml"));
    cmd.env("OMW_KEYCHAIN_BACKEND", "memory");
    // HOME / USERPROFILE: home_dir() in omw-config falls back to these if
    // XDG_CONFIG_HOME is unset, but since we always set OMW_CONFIG explicitly
    // in tests, these are belt-and-braces.
    cmd.env("HOME", temp_dir);
    cmd.env("USERPROFILE", temp_dir);
    cmd.env("XDG_CONFIG_HOME", temp_dir.join("config"));
    if let Some(path) = std::env::var_os("PATH") {
        cmd.env("PATH", path);
    }
    cmd
}

/// Write `content` to `<temp_dir>/config.toml`. Creates the parent directory
/// if it doesn't already exist (tempdir creation is the caller's job).
pub fn seed_config(temp_dir: &Path, content: &str) {
    let path = temp_dir.join("config.toml");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create_dir_all on tempdir parent");
    }
    std::fs::write(&path, content).expect("seed_config write");
}

/// Read back `<temp_dir>/config.toml`. Panics if missing — call only after
/// the SUT was supposed to create or modify the file.
pub fn read_config(temp_dir: &Path) -> String {
    let path = temp_dir.join("config.toml");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read_config({:?}) failed: {}", path, e))
}

/// Run the CLI in-process via the library entrypoint. Returns
/// `(exit_code, stdout_bytes, stderr_bytes)`.
///
/// Set `OMW_KEYCHAIN_BACKEND=memory` and `OMW_CONFIG=<path>` on the live
/// process env BEFORE calling this (it's process-global state).
pub fn lib_mode_run(args: &[&str]) -> (i32, Vec<u8>, Vec<u8>) {
    let owned: Vec<String> = args.iter().map(|s| (*s).to_string()).collect();
    let mut stdout = Cursor::new(Vec::<u8>::new());
    let mut stderr = Cursor::new(Vec::<u8>::new());
    let code = omw_cli::run(&owned, &mut stdout, &mut stderr);
    (code, stdout.into_inner(), stderr.into_inner())
}

/// Sweep every substring of `secret` of length `>= min_window` and assert
/// none appear in `rendered`. Catches partial-prefix leaks (e.g. an impl
/// that logs the first 8 chars of a secret on a panic). Mirrors the
/// `assertNoSecretLeak` helper from `omw-keychain-helper`.
pub fn assert_no_secret_leak(rendered: &str, secret: &str, min_window: usize) {
    let chars: Vec<char> = secret.chars().collect();
    if chars.len() < min_window {
        assert!(
            !rendered.contains(secret),
            "secret leak: full secret {:?} found in rendered output {:?}",
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
                "secret leak: window of length {} ({:?}) from secret {:?} found in {:?}",
                window.chars().count(),
                window,
                secret,
                rendered,
            );
        }
    }
}

// Sanity: silence unused-import warnings if a test file doesn't pull
// `Command` directly. Each integration-test file is its own crate, so this
// per-file `mod common;` import pattern means `dead_code` lint may fire on
// helpers a particular file doesn't use.
#[allow(dead_code)]
fn _unused() -> Option<Command> {
    None
}
