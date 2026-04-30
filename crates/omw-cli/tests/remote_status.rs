//! Integration tests for `omw remote status` — Phase F.
//!
//! File-boundary note: this file is owned by the Test Overseer under the
//! TRD protocol. The Executor MUST NOT modify it.
//!
//! ## Executor checklist
//!
//! 1. New CLI subcommand: `omw remote status`. Reads
//!    `<OMW_DATA_DIR>/remote.pid` to find the running daemon's PID.
//! 2. If the pidfile is missing OR the process named in it is not alive,
//!    print `not running` (exact substring required, lowercase) and exit 0.
//!    Treat a stale pidfile as "not running" — do NOT exit non-zero.
//! 3. (Live-process variant) When the daemon IS running, the message must
//!    contain the substring `running` and the listen address. The
//!    happy-path live test is gated behind `#[cfg(unix)]` and `#[ignore]`
//!    on Windows because process-liveness checks via the pidfile are
//!    flakier on Windows in CI; the negative test below is sufficient.

mod common;

use crate::common::omw_cmd;

/// 1. With no pidfile present, `omw remote status` must print
///    `not running` and exit 0. (No daemon was ever started; the absence
///    of a pidfile is the primary signal.)
#[test]
fn status_when_not_running() {
    let dir = tempfile::tempdir().expect("tempdir");
    let data_dir = dir.path().join("data");
    std::fs::create_dir_all(&data_dir).expect("mkdir data");
    // Explicitly assert there's no pidfile to begin with.
    assert!(
        !data_dir.join("remote.pid").exists(),
        "test setup invariant: remote.pid must not pre-exist"
    );

    let mut cmd = omw_cmd(dir.path());
    cmd.env("OMW_DATA_DIR", &data_dir);
    let assert = cmd.args(["remote", "status"]).assert();
    let output = assert.get_output();
    assert_eq!(
        output.status.code(),
        Some(0),
        "remote status with no pidfile must exit 0; stderr={:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
    .to_lowercase();
    assert!(
        combined.contains("not running"),
        "output must contain 'not running'; got combined output:\n{}",
        combined
    );
}

/// 2. A stale pidfile (PID belongs to no live process) must also be
///    treated as "not running", with the same exit-0 + 'not running'
///    contract as the missing-pidfile case.
///
///    We use PID 1 on Unix and PID 0 on Windows as sentinel "definitely-
///    not-this-binary" values. PID 1 on Unix is init/launchd — it exists
///    but is NOT an `omw` daemon; the executor must do at least a
///    process-name (or port-liveness) check, not just `kill -0`. PID 0 on
///    Windows is reserved.
#[test]
fn status_with_stale_pidfile_says_not_running() {
    let dir = tempfile::tempdir().expect("tempdir");
    let data_dir = dir.path().join("data");
    std::fs::create_dir_all(&data_dir).expect("mkdir data");

    // Write a pidfile with both a PID and a port. Format is unspecified
    // by the task brief; the Executor is free to use either a single
    // line (PID) or a multi-line key=value form. To keep this test
    // robust to either choice, we write a 2-line "pid=<n>\nport=<p>\n"
    // body that a reasonable parser will accept (and a single-int parser
    // will read PID off the first line).
    let pid_body = if cfg!(windows) {
        "pid=0\nport=8787\n"
    } else {
        "pid=1\nport=8787\n"
    };
    std::fs::write(data_dir.join("remote.pid"), pid_body).expect("write stale pidfile");

    let mut cmd = omw_cmd(dir.path());
    cmd.env("OMW_DATA_DIR", &data_dir);
    let assert = cmd.args(["remote", "status"]).assert();
    let output = assert.get_output();
    assert_eq!(
        output.status.code(),
        Some(0),
        "remote status with stale pidfile must exit 0; stderr={:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
    .to_lowercase();
    assert!(
        combined.contains("not running"),
        "stale-pidfile output must contain 'not running'; got combined output:\n{}",
        combined
    );
}
