//! CLI integration tests for `omw-keychain-helper`.
//!
//! File-boundary note: this file is owned by the Test Overseer under the
//! TRD protocol. The Executor authors `src/main.rs` and `Cargo.toml`; they
//! must not modify these tests.
//!
//! ## Executor checklist (DO BEFORE THESE TESTS COMPILE OR RUN)
//!
//! 1. Add `"crates/omw-keychain-helper"` to `members` in the root
//!    `Cargo.toml`'s `[workspace]` table. Without this entry,
//!    `cargo test --workspace --all-targets` will not see this crate at all
//!    and these tests become dead weight.
//! 2. Create `crates/omw-keychain-helper/Cargo.toml` with:
//!      - `[lib]`  `path = "src/lib.rs"`
//!      - `[[bin]]` `name = "omw-keychain-helper"`, `path = "src/main.rs"`
//!      - `[dependencies]`: `omw-config = { path = "../omw-config" }`,
//!        `omw-keychain = { path = "../omw-keychain" }`
//!      - `[dev-dependencies]`: `assert_cmd = "2"`,
//!        `omw-config = { path = "../omw-config" }`,
//!        `omw-keychain = { path = "../omw-keychain" }`
//!    (If `assert_cmd` is added to `[workspace.dependencies]`, dev-deps may
//!    use `.workspace = true` instead of pinning the version here.)
//! 3. The binary in `src/main.rs` is a thin wrapper: collect
//!    `std::env::args()` (skipping argv[0]) and `std::env::vars()`, call
//!    `omw_keychain_helper::run()`, then `std::process::exit` with the code.
//!
//! See also `tests/lib.rs` (in-process tests) and `tests/README.md`.

use std::process::Command;

use assert_cmd::Command as AssertCommand;

/// Helper: build a clean Command pointing at the cargo-built binary, with
/// the parent process's environment fully cleared so each test resolves
/// `OMW_KEYCHAIN_BACKEND` from explicit values only.
fn helper() -> AssertCommand {
    let mut cmd = AssertCommand::cargo_bin("omw-keychain-helper")
        .expect("omw-keychain-helper binary should be built by cargo test");
    cmd.env_clear();
    // Inherit PATH so the dynamic linker can find anything the binary needs
    // on platforms where that matters (notably macOS).
    if let Ok(path) = std::env::var("PATH") {
        cmd.env("PATH", path);
    }
    cmd
}

/// Sweep every substring of `secret` of length >= `min_window` and assert
/// none of them appear in `rendered`. Stricter than a full-string check —
/// catches partial-prefix leaks (e.g. an impl that logs the first 8 chars
/// of a secret on a panic). Mirrors the TS-side `assertNoSecretLeak` helper.
fn assert_no_secret_leak(rendered: &str, secret: &str, min_window: usize) {
    let bytes: Vec<char> = secret.chars().collect();
    if bytes.len() < min_window {
        assert!(
            !rendered.contains(secret),
            "secret leak: full secret {:?} found in rendered output {:?}",
            secret,
            rendered,
        );
        return;
    }
    for start in 0..=(bytes.len() - min_window) {
        for end in (start + min_window)..=bytes.len() {
            let window: String = bytes[start..end].iter().collect();
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

#[test]
fn t1_get_on_never_set_returns_not_found() {
    let assert = helper()
        .env("OMW_KEYCHAIN_BACKEND", "memory")
        .args(["get", "keychain:omw/never-set"])
        .assert();
    let output = assert.get_output();
    assert_eq!(output.status.code(), Some(1), "expected exit 1, got {:?}", output);
    // No stdout on NotFound (asserted more strictly in t9).
    assert!(output.stdout.is_empty(), "stdout should be empty on NotFound, got {:?}", output.stdout);
    // Stderr must explain the failure — defense against silent failures.
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.is_empty(), "stderr should be non-empty on NotFound");
    assert!(
        stderr.to_lowercase().contains("not found"),
        "expected 'not found' (case-insensitive) in stderr on exit 1, got {:?}",
        stderr,
    );
}

#[test]
fn t2_bad_input_malformed_keyref() {
    let assert = helper()
        .env("OMW_KEYCHAIN_BACKEND", "memory")
        .args(["get", "sk-not-a-keyref"])
        .assert();
    let output = assert.get_output();
    assert_eq!(output.status.code(), Some(2), "expected exit 2, got {:?}", output);
    assert!(output.stdout.is_empty(), "stdout should be empty on bad input");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.is_empty(), "stderr should be non-empty on bad input");
    let lower = stderr.to_lowercase();
    assert!(
        lower.contains("invalid") || lower.contains("usage"),
        "expected 'invalid' or 'usage' (case-insensitive) in stderr on exit 2, got {:?}",
        stderr,
    );
}

#[test]
fn t3_bad_input_missing_argv() {
    let assert = helper()
        .env("OMW_KEYCHAIN_BACKEND", "memory")
        .args(["get"])
        .assert();
    let output = assert.get_output();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.is_empty(), "stderr should be non-empty on missing argv");
    let lower = stderr.to_lowercase();
    assert!(
        lower.contains("invalid") || lower.contains("usage"),
        "expected 'invalid' or 'usage' (case-insensitive) in stderr on exit 2, got {:?}",
        stderr,
    );
}

#[test]
fn t4_bad_input_unknown_subcommand() {
    let assert = helper()
        .env("OMW_KEYCHAIN_BACKEND", "memory")
        .args(["unknown-cmd"])
        .assert();
    let output = assert.get_output();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.is_empty(), "stderr should be non-empty on unknown subcommand");
    let lower = stderr.to_lowercase();
    assert!(
        lower.contains("invalid") || lower.contains("usage"),
        "expected 'invalid' or 'usage' (case-insensitive) in stderr on exit 2, got {:?}",
        stderr,
    );
}

#[test]
fn t5_no_subcommand() {
    let assert = helper()
        .env("OMW_KEYCHAIN_BACKEND", "memory")
        .assert();
    let output = assert.get_output();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.is_empty(), "stderr should be non-empty when no subcommand given");
    let lower = stderr.to_lowercase();
    assert!(
        lower.contains("invalid") || lower.contains("usage"),
        "expected 'invalid' or 'usage' (case-insensitive) in stderr on exit 2, got {:?}",
        stderr,
    );
}

/// Backend-unavailable applies on Linux/Windows where OS keychain is Beyond v1.
/// Skipped on macOS because the OS backend works there.
#[cfg(not(target_os = "macos"))]
#[test]
fn t6_backend_unavailable_on_linux_windows() {
    let assert = helper()
        .env("OMW_KEYCHAIN_BACKEND", "os")
        .args(["get", "keychain:omw/x"])
        .assert();
    let output = assert.get_output();
    assert_eq!(output.status.code(), Some(3), "expected exit 3, got {:?}", output);
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.is_empty(), "stderr should be non-empty on backend unavailable");
    assert!(
        stderr.to_lowercase().contains("unavailable"),
        "expected 'unavailable' (case-insensitive) in stderr on exit 3, got {:?}",
        stderr,
    );
}

/// If a sentinel string ends up in the key_ref, the binary's stderr must NOT
/// echo it back. Names ARE metadata (not secret) but we want defense-in-depth:
/// a misuse where someone passes a real secret as the key_ref must not be
/// amplified by the helper logging it. Partial-prefix sweep at length >= 4
/// catches truncated leaks too.
#[test]
fn t7_stderr_does_not_echo_sentinel_from_keyref() {
    const SENTINEL: &str = "super-secret-payload-12345";
    let key_ref = format!("keychain:omw/{}", SENTINEL);
    let assert = helper()
        .env("OMW_KEYCHAIN_BACKEND", "memory")
        .args(["get", &key_ref])
        .assert();
    let output = assert.get_output();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_no_secret_leak(&stderr, SENTINEL, 4);
}

#[test]
fn t8_help_flag_produces_usage() {
    let assert = helper().arg("--help").assert();
    let output = assert.get_output();
    assert_eq!(output.status.code(), Some(0), "--help should exit 0");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{}{}", stdout, String::from_utf8_lossy(&output.stderr));
    assert!(
        combined.to_lowercase().contains("usage")
            || combined.to_lowercase().contains("help"),
        "expected usage info in --help output, got stdout={:?} stderr={:?}",
        stdout,
        String::from_utf8_lossy(&output.stderr),
    );
}

/// Negative shape test: NotFound MUST emit zero bytes on stdout. A bare
/// trailing newline would be a bug — callers trim exactly one newline and
/// would otherwise get the empty string back, which is indistinguishable
/// from "secret was the empty string". Keep stdout empty on the error path.
#[test]
fn t9_stdout_is_empty_on_not_found_no_trailing_newline() {
    let assert = helper()
        .env("OMW_KEYCHAIN_BACKEND", "memory")
        .args(["get", "keychain:omw/never-set"])
        .assert();
    let output = assert.get_output();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        output.stdout.len(),
        0,
        "NotFound must produce zero stdout bytes, got {:?}",
        String::from_utf8_lossy(&output.stdout)
    );
}

// Sanity: silence unused-import warnings when the helper is removed.
#[allow(dead_code)]
fn _unused() -> Option<Command> {
    None
}
