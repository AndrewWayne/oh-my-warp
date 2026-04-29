//! Integration tests for `omw ask <prompt>` — the v0.1 MVP that spawns an
//! `omw-agent` binary with the prompt + flags and forwards env.
//!
//! File-boundary note: this file is owned by the Test Overseer under the
//! TRD protocol. The Executor MUST NOT modify this file or any other
//! `tests/*` file.
//!
//! ## Executor checklist (gate signals beyond `cli_provider.rs`)
//!
//! 1. The `Cli`/`Command` enum must grow an `Ask` variant with at minimum:
//!    - `prompt: String` (required positional)
//!    - `provider: Option<String>` (`--provider`)
//!    - `model: Option<String>` (`--model`)
//!    - `max_tokens: Option<u32>` (`--max-tokens`)
//!    - `temperature: Option<f32>` (`--temperature`)
//! 2. The handler resolves the agent binary in this order:
//!    - `OMW_AGENT_BIN` env var (used by these tests),
//!    - some sensible default for production (e.g. `omw-agent` on PATH).
//!    The default is NOT exercised here.
//! 3. The handler spawns the resolved binary with `ask` as the first
//!    argv, followed by the prompt, then any provided flags. It must
//!    propagate the relevant env vars (at least `OMW_CONFIG`,
//!    `OMW_KEYCHAIN_HELPER`, `OMW_KEYCHAIN_BACKEND`) to the child.
//! 4. The handler must stream the child's stdout/stderr into its own and
//!    exit with the child's exit code.
//!
//! ## Why a Node-script fixture
//!
//! The fake agent lives at `tests/fixtures/fake-agent.cjs` and writes a
//! single JSON line to stdout describing argv + a whitelist of inherited
//! env vars. The tests parse that JSON to assert what the SUT spawned.
//!
//! `OMW_AGENT_BIN` must be a single executable path on every platform.
//! On Windows we generate a `.cmd` wrapper at test runtime; on Unix we
//! generate a `.sh` script with the executable bit set. Both forward
//! all argv to `node <fake-agent.cjs>`.

mod common;

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::common::omw_cmd;

// =============================================================================
// Fake-agent wrapper plumbing
// =============================================================================

/// JSON shape the fake agent prints to stdout. Mirrors the structure in
/// `tests/fixtures/fake-agent.cjs`.
#[derive(Debug, Deserialize)]
struct AgentEcho {
    argv: Vec<String>,
    env: std::collections::BTreeMap<String, String>,
}

/// Path to the Node fixture, expressed relative to the workspace root via
/// `CARGO_MANIFEST_DIR`. cargo sets that env var for every integration test.
fn fake_agent_script() -> PathBuf {
    let manifest = std::env::var("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR must be set by cargo for integration tests");
    Path::new(&manifest)
        .join("tests")
        .join("fixtures")
        .join("fake-agent.cjs")
}

/// Resolve `node` on the parent's PATH. The tests REQUIRE Node — they
/// fail loudly if Node isn't available so the failure mode is unambiguous.
fn locate_node() -> String {
    // `which`/`where` both return absolute paths. We use the same shell
    // command on either platform via `command -v` / `where` would be
    // non-portable; just trust that `node` resolves through `Command`'s
    // PATH search. If the test machine doesn't have Node, the wrapper
    // will fail to execute and the assertion below will surface it.
    "node".to_string()
}

/// Build a single-executable wrapper inside `dir` that forwards all args
/// to `node <fake-agent.cjs>`. Returns the wrapper's absolute path.
///
/// On Windows: writes a `.cmd` file using `%*` for argv passthrough.
/// On Unix:    writes a `.sh` script with `+x` and `"$@"` for argv passthrough.
fn write_agent_wrapper(dir: &Path) -> PathBuf {
    let script = fake_agent_script();
    let node = locate_node();

    if cfg!(windows) {
        let wrapper = dir.join("fake-agent.cmd");
        // `@echo off` to keep stdout clean. Quote both paths defensively
        // for spaces (e.g. "C:\Program Files\nodejs\node.exe").
        let body = format!(
            "@echo off\r\n\"{}\" \"{}\" %*\r\n",
            node,
            script.display()
        );
        std::fs::write(&wrapper, body).expect("write windows wrapper");
        wrapper
    } else {
        let wrapper = dir.join("fake-agent.sh");
        let body = format!(
            "#!/bin/sh\nexec \"{}\" \"{}\" \"$@\"\n",
            node,
            script.display()
        );
        std::fs::write(&wrapper, body).expect("write unix wrapper");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perm = std::fs::metadata(&wrapper)
                .expect("stat wrapper")
                .permissions();
            perm.set_mode(0o755);
            std::fs::set_permissions(&wrapper, perm).expect("chmod wrapper");
        }
        wrapper
    }
}

/// Find the value that immediately follows `name` in `argv`. Returns
/// `None` if the flag is absent or has no following token. Used by tests
/// to assert flag/value adjacency — a buggy SUT that swaps two values
/// (e.g. `--provider gpt-4o --model openai-prod`) would fail this check
/// even though both values are present somewhere in argv.
///
/// Also accepts the `--name=value` shape: in that case the value is the
/// substring after `=` in the matching token.
fn flag_value<'a>(argv: &'a [String], name: &str) -> Option<&'a str> {
    let eq_prefix = format!("{name}=");
    for (i, a) in argv.iter().enumerate() {
        if a == name {
            return argv.get(i + 1).map(|s| s.as_str());
        }
        if let Some(rest) = a.strip_prefix(&eq_prefix) {
            return Some(rest);
        }
    }
    None
}

/// Decode the JSON line the fake agent printed. Panics with a useful
/// diagnostic if the SUT's stdout doesn't contain the expected payload —
/// e.g. because the SUT swallowed the child's stdout instead of streaming
/// it through.
fn parse_agent_echo(stdout: &[u8]) -> AgentEcho {
    let s = std::str::from_utf8(stdout)
        .unwrap_or_else(|e| panic!("fake-agent stdout not utf-8: {e}\n{:?}", stdout));
    let line = s.lines().next().unwrap_or_else(|| {
        panic!(
            "fake-agent stdout is empty — SUT must STREAM child stdout to parent stdout. Got {:?}",
            s
        )
    });
    serde_json::from_str(line).unwrap_or_else(|e| {
        panic!(
            "fake-agent stdout is not valid JSON: {e}\nstdout was: {:?}",
            s
        )
    })
}

// =============================================================================
// Tests
// =============================================================================

#[test]
fn ask_requires_prompt_arg() {
    // Without a positional prompt, clap should reject the invocation. The
    // exit code must be non-zero and stderr should explain the missing
    // argument. We don't bind to clap's exact wording — we accept any of
    // the canonical phrases.
    let dir = tempfile::tempdir().expect("tempdir");
    let assert = omw_cmd(dir.path()).args(["ask"]).assert();
    let output = assert.get_output();
    assert_ne!(
        output.status.code(),
        Some(0),
        "ask without prompt must fail; stdout={:?}",
        String::from_utf8_lossy(&output.stdout),
    );
    let stderr = String::from_utf8_lossy(&output.stderr).to_lowercase();
    let mentions_prompt = stderr.contains("prompt")
        || stderr.contains("required")
        || stderr.contains("missing")
        || stderr.contains("argument");
    assert!(
        mentions_prompt,
        "stderr should explain the missing prompt arg, got {:?}",
        stderr
    );
}

#[test]
fn ask_passes_prompt_to_omw_agent_bin() {
    // Set OMW_AGENT_BIN to a wrapper that runs the fake-agent Node script.
    // Run `omw ask "hello world"` and assert the fake agent saw both
    // `ask` and `hello world` in its argv.
    let dir = tempfile::tempdir().expect("tempdir");
    let wrapper = write_agent_wrapper(dir.path());

    let mut cmd = omw_cmd(dir.path());
    cmd.env("OMW_AGENT_BIN", &wrapper);
    let assert = cmd.args(["ask", "hello world"]).assert();
    let output = assert.get_output();
    assert_eq!(
        output.status.code(),
        Some(0),
        "ask should exit 0 with a successful fake agent; stderr={:?}",
        String::from_utf8_lossy(&output.stderr)
    );

    let echo = parse_agent_echo(&output.stdout);
    let ask_idx = echo
        .argv
        .iter()
        .position(|a| a == "ask")
        .unwrap_or_else(|| panic!("child argv must contain 'ask'; got {:?}", echo.argv));
    let prompt_idx = echo
        .argv
        .iter()
        .position(|a| a == "hello world")
        .unwrap_or_else(|| {
            panic!(
                "child argv must contain the literal prompt 'hello world'; got {:?}",
                echo.argv
            )
        });
    assert!(
        ask_idx < prompt_idx,
        "'ask' must come before the prompt; got argv={:?}",
        echo.argv
    );
    assert_eq!(
        prompt_idx,
        ask_idx + 1,
        "the prompt must be the next positional after 'ask'; got argv={:?}",
        echo.argv
    );
}

#[test]
fn ask_passes_provider_and_model_flags() {
    // `omw ask "hi" --provider foo --model gpt-4o --max-tokens 100
    //  --temperature 0.5` — the fake agent must see all flag values.
    let dir = tempfile::tempdir().expect("tempdir");
    let wrapper = write_agent_wrapper(dir.path());

    let mut cmd = omw_cmd(dir.path());
    cmd.env("OMW_AGENT_BIN", &wrapper);
    let assert = cmd
        .args([
            "ask",
            "hi",
            "--provider",
            "foo",
            "--model",
            "gpt-4o",
            "--max-tokens",
            "100",
            "--temperature",
            "0.5",
        ])
        .assert();
    let output = assert.get_output();
    assert_eq!(
        output.status.code(),
        Some(0),
        "ask with flags should exit 0; stderr={:?}",
        String::from_utf8_lossy(&output.stderr)
    );

    let echo = parse_agent_echo(&output.stdout);
    // The literal prompt must still reach the child as a positional.
    assert!(
        echo.argv.iter().any(|a| a == "hi"),
        "expected prompt \"hi\" in forwarded argv {:?}",
        echo.argv
    );
    // Each flag must be paired with the CORRECT value. We accept either
    // `--name value` (adjacent tokens) or `--name=value` (single token).
    // Asserting only the presence of names + values would let a buggy
    // SUT swap two values (e.g. `--provider gpt-4o --model foo`) pass.
    let pairs: &[(&str, &str)] = &[
        ("--provider", "foo"),
        ("--model", "gpt-4o"),
        ("--max-tokens", "100"),
        ("--temperature", "0.5"),
    ];
    for (name, expected) in pairs {
        let actual = flag_value(&echo.argv, name).unwrap_or_else(|| {
            panic!(
                "child argv must contain {name} flag with a value; got {:?}",
                echo.argv
            )
        });
        assert_eq!(
            actual, *expected,
            "expected {name} to be paired with {expected:?}, got {actual:?}; argv={:?}",
            echo.argv
        );
    }
}

#[test]
fn ask_propagates_child_exit_code_and_stderr() {
    // The fake agent is told via env to write a known stderr line and
    // exit 42. The SUT must (a) stream the child's stderr to the
    // parent's stderr and (b) exit with the child's exit code.
    let dir = tempfile::tempdir().expect("tempdir");
    let wrapper = write_agent_wrapper(dir.path());

    let mut cmd = omw_cmd(dir.path());
    cmd.env("OMW_AGENT_BIN", &wrapper);
    cmd.env("FAKE_AGENT_MODE", "fail");
    let assert = cmd.args(["ask", "x"]).assert();
    let output = assert.get_output();

    assert_eq!(
        output.status.code(),
        Some(42),
        "parent exit code must equal child exit code (42); stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("fake stderr line"),
        "child stderr must be streamed to parent stderr; got stderr={:?}",
        stderr
    );
}

#[test]
fn ask_passes_through_environment() {
    // OMW_CONFIG must reach the child. We set a synthetic value and
    // assert the fake agent saw it. We also include OMW_KEYCHAIN_HELPER
    // and OMW_KEYCHAIN_BACKEND in the parent env block so an Executor
    // that propagates those gets credit.
    let dir = tempfile::tempdir().expect("tempdir");
    let wrapper = write_agent_wrapper(dir.path());
    let synthetic_config = dir.path().join("synthetic-config.toml");

    let mut cmd = omw_cmd(dir.path());
    cmd.env("OMW_AGENT_BIN", &wrapper);
    cmd.env("OMW_CONFIG", &synthetic_config);
    cmd.env("OMW_KEYCHAIN_HELPER", "/fake/path/to/helper");
    cmd.env("OMW_KEYCHAIN_BACKEND", "memory");
    cmd.env("OMW_AGENT_PROBE", "should-be-inherited");

    let assert = cmd.args(["ask", "ping"]).assert();
    let output = assert.get_output();
    assert_eq!(
        output.status.code(),
        Some(0),
        "ask should exit 0; stderr={:?}",
        String::from_utf8_lossy(&output.stderr)
    );

    let echo = parse_agent_echo(&output.stdout);
    assert_eq!(
        echo.env.get("OMW_CONFIG").map(String::as_str),
        Some(synthetic_config.to_string_lossy().as_ref()),
        "OMW_CONFIG must be forwarded verbatim to the agent; got env={:?}",
        echo.env
    );
    assert_eq!(
        echo.env.get("OMW_KEYCHAIN_HELPER").map(String::as_str),
        Some("/fake/path/to/helper"),
        "OMW_KEYCHAIN_HELPER must be forwarded; got env={:?}",
        echo.env
    );
    assert_eq!(
        echo.env.get("OMW_KEYCHAIN_BACKEND").map(String::as_str),
        Some("memory"),
        "OMW_KEYCHAIN_BACKEND must be forwarded; got env={:?}",
        echo.env
    );
}
