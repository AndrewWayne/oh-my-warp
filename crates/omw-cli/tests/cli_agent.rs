//! Integration tests for `omw agent` — the v0.1 line-oriented REPL that
//! reads stdin and spawns `omw-agent ask <line>` per non-empty / non-meta
//! line.
//!
//! File-boundary note: this file is owned by the Test Overseer under the
//! TRD protocol. The Executor MUST NOT modify this file or any other
//! `tests/*` file.
//!
//! ## Executor checklist (gates beyond `cli_ask.rs` / `cli_costs.rs`)
//!
//! 1. The clap `Command` enum gains an `Agent` variant with at minimum:
//!    - `cwd: Option<PathBuf>`   (`--cwd`)
//!    - `provider: Option<String>` (`--provider`)
//!    - `model: Option<String>`    (`--model`)
//!
//! 2. New module `crates/omw-cli/src/commands/agent_runner.rs` exposing a
//!    function that runs ONE turn (spawn + stream + persist) so the REPL
//!    loop can call it per-line. `commands/ask.rs` becomes a thin wrapper
//!    around it.
//!
//! 3. The REPL reads stdin line-by-line:
//!    - Empty line: print a prompt marker (e.g. `>>> `) and continue.
//!    - `/exit` or `/quit`: break the loop, exit 0.
//!    - EOF: exit 0.
//!    - Otherwise: spawn `omw-agent ask <line>` (with --provider / --model
//!      from the parent flags propagated), stream stdio, persist usage,
//!      continue regardless of whether the turn succeeded.
//!    - A failed turn (non-zero child exit) writes diagnostics to stderr
//!      but does NOT terminate the loop.
//!
//! 4. `--cwd <path>` is the working directory used when spawning the
//!    agent for each turn. The file `--cwd` itself MUST NOT be passed
//!    through to `omw-agent ask`; it's a parent-side flag only.

mod common;

use std::path::{Path, PathBuf};

use crate::common::omw_cmd;

// =============================================================================
// Fake-agent wrapper plumbing
//
// Mirrors the helpers in cli_ask.rs / cli_costs.rs. Each integration-test
// file is its own crate, so we duplicate the small wrapper-emitter rather
// than putting it in `common::mod.rs` (which is intentionally minimal).
// =============================================================================

fn fake_agent_script() -> PathBuf {
    let manifest = std::env::var("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR must be set by cargo for integration tests");
    Path::new(&manifest)
        .join("tests")
        .join("fixtures")
        .join("fake-agent.cjs")
}

/// Build a single-executable wrapper inside `dir` that forwards all args
/// to `node <fake-agent.cjs>`. Returns the wrapper's absolute path.
fn write_agent_wrapper(dir: &Path) -> PathBuf {
    let script = fake_agent_script();
    if cfg!(windows) {
        let wrapper = dir.join("fake-agent.cmd");
        let body = format!("@echo off\r\n\"node\" \"{}\" %*\r\n", script.display());
        std::fs::write(&wrapper, body).expect("write windows wrapper");
        wrapper
    } else {
        let wrapper = dir.join("fake-agent.sh");
        let body = format!("#!/bin/sh\nexec node \"{}\" \"$@\"\n", script.display());
        std::fs::write(&wrapper, body).expect("write unix wrapper");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perm = std::fs::metadata(&wrapper).expect("stat").permissions();
            perm.set_mode(0o755);
            std::fs::set_permissions(&wrapper, perm).expect("chmod");
        }
        wrapper
    }
}

/// Number of invocations recorded in the FAKE_AGENT_COUNTER_FILE. The
/// fixture appends one newline per invocation, so the count is just the
/// number of `\n` bytes (or the file's byte length, which is equivalent).
fn invocation_count(counter_file: &Path) -> usize {
    if !counter_file.exists() {
        return 0;
    }
    let bytes = std::fs::read(counter_file).expect("read counter file");
    bytes.iter().filter(|b| **b == b'\n').count()
}

/// Read the JSONL argv file the fixture wrote (one JSON array per line)
/// into a Vec<Vec<String>>. Returns an empty Vec if the file doesn't
/// exist (i.e. the fixture was never invoked).
fn read_argv_log(argv_file: &Path) -> Vec<Vec<String>> {
    if !argv_file.exists() {
        return Vec::new();
    }
    let s = std::fs::read_to_string(argv_file).expect("read argv file");
    s.lines()
        .filter(|l| !l.is_empty())
        .map(|l| {
            serde_json::from_str::<Vec<String>>(l)
                .unwrap_or_else(|e| panic!("argv line {l:?} not JSON array: {e}"))
        })
        .collect()
}

/// Lower-case + canonicalize a path for comparison. On Windows, `tempdir`
/// may hand us `C:\Users\andre\...` while the child's `process.cwd()`
/// may render with different case. We can't rely on `canonicalize`
/// existing on every path we want to compare (e.g. the canonical form
/// might prepend `\\?\`); just lower-case the string form on both sides.
fn normalize_path(p: &Path) -> String {
    p.to_string_lossy().to_lowercase().replace('\\', "/")
}

// =============================================================================
// Tests
// =============================================================================

/// 1. Empty stdin (immediate EOF, no bytes written) → exit 0.
#[test]
fn agent_exits_on_eof() {
    let dir = tempfile::tempdir().expect("tempdir");
    let wrapper = write_agent_wrapper(dir.path());
    let counter = dir.path().join("counter.txt");

    let mut cmd = omw_cmd(dir.path());
    cmd.env("OMW_AGENT_BIN", &wrapper);
    cmd.env("FAKE_AGENT_COUNTER_FILE", &counter);

    let assert = cmd.args(["agent"]).write_stdin("").assert();
    let output = assert.get_output();
    assert_eq!(
        output.status.code(),
        Some(0),
        "agent must exit 0 on EOF; stderr={:?} stdout={:?}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        invocation_count(&counter),
        0,
        "agent must NOT spawn the child on empty stdin; counter file should be empty"
    );
}

/// 2. `/exit` ends the loop with exit 0 and never spawns the child.
#[test]
fn agent_exits_on_slash_exit() {
    let dir = tempfile::tempdir().expect("tempdir");
    let wrapper = write_agent_wrapper(dir.path());
    let counter = dir.path().join("counter.txt");

    let mut cmd = omw_cmd(dir.path());
    cmd.env("OMW_AGENT_BIN", &wrapper);
    cmd.env("FAKE_AGENT_COUNTER_FILE", &counter);

    let assert = cmd.args(["agent"]).write_stdin("/exit\n").assert();
    let output = assert.get_output();
    assert_eq!(
        output.status.code(),
        Some(0),
        "agent must exit 0 on /exit; stderr={:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        invocation_count(&counter),
        0,
        "/exit is a meta-command and MUST NOT spawn the child"
    );
}

/// 3. `/quit` is a synonym for `/exit`.
#[test]
fn agent_exits_on_slash_quit() {
    let dir = tempfile::tempdir().expect("tempdir");
    let wrapper = write_agent_wrapper(dir.path());
    let counter = dir.path().join("counter.txt");

    let mut cmd = omw_cmd(dir.path());
    cmd.env("OMW_AGENT_BIN", &wrapper);
    cmd.env("FAKE_AGENT_COUNTER_FILE", &counter);

    let assert = cmd.args(["agent"]).write_stdin("/quit\n").assert();
    let output = assert.get_output();
    assert_eq!(
        output.status.code(),
        Some(0),
        "agent must exit 0 on /quit; stderr={:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        invocation_count(&counter),
        0,
        "/quit is a meta-command and MUST NOT spawn the child"
    );
}

/// 4. Empty lines are skipped (no child spawn) and EOF after them exits 0.
#[test]
fn agent_skips_empty_lines() {
    let dir = tempfile::tempdir().expect("tempdir");
    let wrapper = write_agent_wrapper(dir.path());
    let counter = dir.path().join("counter.txt");

    let mut cmd = omw_cmd(dir.path());
    cmd.env("OMW_AGENT_BIN", &wrapper);
    cmd.env("FAKE_AGENT_COUNTER_FILE", &counter);

    let assert = cmd.args(["agent"]).write_stdin("\n\n\n").assert();
    let output = assert.get_output();
    assert_eq!(
        output.status.code(),
        Some(0),
        "agent must exit 0 after a run of empty lines + EOF; stderr={:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        invocation_count(&counter),
        0,
        "empty lines must NOT spawn the child; counter={}",
        invocation_count(&counter)
    );
}

/// 5. Each non-empty / non-meta line spawns one independent turn.
#[test]
fn agent_runs_multiple_turns() {
    let dir = tempfile::tempdir().expect("tempdir");
    let wrapper = write_agent_wrapper(dir.path());
    let counter = dir.path().join("counter.txt");

    let mut cmd = omw_cmd(dir.path());
    cmd.env("OMW_AGENT_BIN", &wrapper);
    cmd.env("FAKE_AGENT_COUNTER_FILE", &counter);

    let assert = cmd
        .args(["agent"])
        .write_stdin("hello\nworld\n/exit\n")
        .assert();
    let output = assert.get_output();
    assert_eq!(
        output.status.code(),
        Some(0),
        "agent must exit 0; stderr={:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let count = invocation_count(&counter);
    assert_eq!(
        count, 2,
        "two non-empty / non-meta lines must spawn the child exactly twice; got {count}"
    );
}

/// 6. Each successful turn writes a usage_records row.
#[test]
fn agent_writes_usage_per_turn() {
    let dir = tempfile::tempdir().expect("tempdir");
    let wrapper = write_agent_wrapper(dir.path());
    let counter = dir.path().join("counter.txt");
    let data_dir = dir.path().join("data");
    std::fs::create_dir_all(&data_dir).expect("mkdir data");

    // The agent's usage-capture path needs to resolve provider_kind from
    // the config. Seed config with a `test` provider matching the
    // fake-agent's default usage line (provider="test").
    crate::common::seed_config(
        dir.path(),
        r#"version = 1

[providers.test]
kind = "ollama"
"#,
    );

    let mut cmd = omw_cmd(dir.path());
    cmd.env("OMW_AGENT_BIN", &wrapper);
    cmd.env("FAKE_AGENT_COUNTER_FILE", &counter);
    cmd.env("OMW_DATA_DIR", &data_dir);

    let assert = cmd
        .args(["agent"])
        .write_stdin("hello\nworld\n/exit\n")
        .assert();
    let output = assert.get_output();
    assert_eq!(
        output.status.code(),
        Some(0),
        "agent must exit 0; stderr={:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        invocation_count(&counter),
        2,
        "two turns must produce two child invocations"
    );

    // Inspect the db that the in-process record_usage path wrote to.
    let db = data_dir.join("omw.sqlite3");
    assert!(
        db.exists(),
        "agent must have created the SQLite db at {:?}",
        db
    );
    let conn = rusqlite::Connection::open(&db).expect("open db");
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM usage_records", [], |r| r.get(0))
        .expect("count usage_records");
    assert_eq!(
        count, 2,
        "each turn must persist exactly one usage_records row; got {count}"
    );

    // Both rows should have provider_id = 'test' (the fake-agent default).
    let test_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM usage_records WHERE provider_id = 'test'",
            [],
            |r| r.get(0),
        )
        .expect("count test rows");
    assert_eq!(
        test_count, 2,
        "all usage rows should have provider_id='test'; got {test_count}"
    );
}

/// 7. `--provider foo` propagates into every per-turn child invocation.
#[test]
fn agent_passes_provider_flag_through_to_each_turn() {
    let dir = tempfile::tempdir().expect("tempdir");
    let wrapper = write_agent_wrapper(dir.path());
    let counter = dir.path().join("counter.txt");
    let argv_file = dir.path().join("argv.jsonl");

    let mut cmd = omw_cmd(dir.path());
    cmd.env("OMW_AGENT_BIN", &wrapper);
    cmd.env("FAKE_AGENT_COUNTER_FILE", &counter);
    cmd.env("FAKE_AGENT_ARGV_FILE", &argv_file);

    let assert = cmd
        .args(["agent", "--provider", "foo"])
        .write_stdin("hi\n/exit\n")
        .assert();
    let output = assert.get_output();
    assert_eq!(
        output.status.code(),
        Some(0),
        "agent must exit 0; stderr={:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        invocation_count(&counter),
        1,
        "exactly one non-meta line must produce one invocation"
    );

    let invocations = read_argv_log(&argv_file);
    assert_eq!(
        invocations.len(),
        1,
        "argv log should record one invocation; got {invocations:?}"
    );
    for (i, argv) in invocations.iter().enumerate() {
        assert!(
            argv.iter().any(|a| a == "foo"),
            "turn #{i} child argv must include the provider value 'foo'; got {argv:?}"
        );
        // Belt-and-braces: the `--provider` flag itself or a `--provider=foo`
        // single-token form must be present alongside the value.
        let has_flag = argv.iter().any(|a| a == "--provider" || a == "--provider=foo");
        assert!(
            has_flag,
            "turn #{i} child argv must include the --provider flag; got {argv:?}"
        );
    }
}

/// 8. `--cwd <path>` is the spawn cwd of each turn's child process.
#[test]
fn agent_propagates_cwd_to_child() {
    let dir = tempfile::tempdir().expect("tempdir");
    let wrapper = write_agent_wrapper(dir.path());
    let cwd_file = dir.path().join("cwd.txt");

    // The directory we want the child to run inside. Must exist before
    // we spawn — Command::current_dir on a non-existent path is an error
    // on every platform.
    let target_cwd = dir.path().join("workdir");
    std::fs::create_dir_all(&target_cwd).expect("mkdir workdir");

    let mut cmd = omw_cmd(dir.path());
    cmd.env("OMW_AGENT_BIN", &wrapper);
    cmd.env("FAKE_AGENT_CWD_FILE", &cwd_file);

    let assert = cmd
        .args(["agent", "--cwd"])
        .arg(&target_cwd)
        .write_stdin("hi\n/exit\n")
        .assert();
    let output = assert.get_output();
    assert_eq!(
        output.status.code(),
        Some(0),
        "agent must exit 0; stderr={:?}",
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        cwd_file.exists(),
        "fake-agent must have written cwd-side-channel file at {:?}",
        cwd_file
    );
    let observed = std::fs::read_to_string(&cwd_file).expect("read cwd file");
    let observed_norm = normalize_path(Path::new(observed.trim()));
    let want_norm = normalize_path(&target_cwd);
    assert_eq!(
        observed_norm, want_norm,
        "child cwd must equal --cwd target; observed={:?} want={:?}",
        observed, target_cwd
    );
}

/// 9. A failed turn (non-zero child exit) does NOT terminate the REPL —
///    subsequent lines are still processed.
#[test]
fn agent_continues_after_failed_turn() {
    let dir = tempfile::tempdir().expect("tempdir");
    let wrapper = write_agent_wrapper(dir.path());
    let counter = dir.path().join("counter.txt");

    let mut cmd = omw_cmd(dir.path());
    cmd.env("OMW_AGENT_BIN", &wrapper);
    cmd.env("FAKE_AGENT_COUNTER_FILE", &counter);
    // First invocation fails (exit 42); subsequent succeed. The counter
    // file is the source of truth for "first invocation" — the fixture
    // checks the file's pre-append size, so this is robust against
    // re-orderings.
    cmd.env("FAKE_AGENT_FAIL_FIRST", "1");

    let assert = cmd
        .args(["agent"])
        .write_stdin("first\nsecond\n/exit\n")
        .assert();
    let output = assert.get_output();
    assert_eq!(
        output.status.code(),
        Some(0),
        "REPL must exit 0 even when a turn fails; stderr={:?} stdout={:?}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let count = invocation_count(&counter);
    assert_eq!(
        count, 2,
        "REPL must run BOTH turns even though the first failed; got {count} invocations"
    );

    // The first turn's stderr line ("fake stderr line") must have made
    // it through to the parent's stderr, so the user knows the turn
    // failed (vs. silent swallow).
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("fake stderr line"),
        "failed turn's child stderr must reach the user; got stderr={:?}",
        stderr
    );
}
