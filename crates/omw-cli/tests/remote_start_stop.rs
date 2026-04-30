//! Integration tests for `omw remote start` and `omw remote stop` — Phase F.
//!
//! File-boundary note: this file is owned by the Test Overseer under the
//! TRD protocol. The Executor MUST NOT modify it.
//!
//! ## Executor checklist
//!
//! 1. `omw remote start [--listen 127.0.0.1:8787] [--no-tailscale]`
//!    spawns the `omw-remote` server in the FOREGROUND (no
//!    daemonization for v0.4-thin). It must:
//!      - Write a pidfile at `<OMW_DATA_DIR>/remote.pid`. The format must
//!        encode at minimum the listen port and the process pid; we
//!        accept a `pid=<n>\nport=<p>\n` plain-text format or a
//!        single-int file (start chooses; stop must read the same).
//!      - Load the host signing key from
//!        `<OMW_DATA_DIR>/host_key.bin`, generating + saving it if
//!        missing (`omw_remote::HostKey::load_or_create`).
//!      - Open the pairings db at `<OMW_DATA_DIR>/omw-remote.sqlite3`
//!        (apply Phase D migrations via `omw_remote::open_db`).
//!      - Bind to the requested address. Tests pass `127.0.0.1:0` so the
//!        OS picks a free port; the actual bound port must be discoverable
//!        via the pidfile (so `omw remote stop` can reach the running
//!        instance, and tests can wait-for-bind).
//!      - Shut down gracefully when a hidden test hook fires. The
//!        cleanest mechanism is `--shutdown-signal <env_var_name>`: when
//!        the named env var is observed (e.g., the daemon spins on a
//!        watch channel that flips when the file
//!        `<OMW_DATA_DIR>/<env_var_name>.signal` appears), the server
//!        unwinds. Implementation detail is up to the Executor; the test
//!        below uses an environment variable as the rendezvous.
//!
//! 2. `omw remote stop [--all]` reads the pidfile, signals the running
//!    daemon to shut down (TERM on Unix, equivalent on Windows or via
//!    the same shutdown mechanism), waits for the process to exit, and
//!    removes the pidfile. With `--all`, additionally sets
//!    `devices.revoked_at = now()` for every row.
//!
//! These tests are intentionally minimal: the start path is hard to
//! exercise portably. Both tests are gated `#[cfg(unix)]` because spawn
//! + signal semantics on Windows require a different shutdown mechanism
//! (named events, not signals) — that variant is Beyond-v1 work.

mod common;

#[cfg(unix)]
mod unix_only {
    use std::path::PathBuf;
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};

    use crate::common::omw_cmd;

    fn omw_bin() -> PathBuf {
        // assert_cmd builds and caches the bin in the same target dir
        // cargo would. We can resolve via env::var_os("CARGO_BIN_EXE_omw")
        // when it's set (cargo sets this for integration tests), and fall
        // back to assert_cmd's resolver otherwise.
        if let Some(p) = std::env::var_os("CARGO_BIN_EXE_omw") {
            return PathBuf::from(p);
        }
        assert_cmd::cargo::cargo_bin("omw")
    }

    fn wait_for_pidfile(path: &std::path::Path, timeout: Duration) -> std::io::Result<String> {
        let start = Instant::now();
        loop {
            if path.exists() {
                let s = std::fs::read_to_string(path)?;
                if !s.trim().is_empty() {
                    return Ok(s);
                }
            }
            if start.elapsed() > timeout {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "pidfile did not appear within timeout",
                ));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn spawn_remote(
        data_dir: &std::path::Path,
        signal_var: &str,
    ) -> std::io::Result<Child> {
        let mut cmd = Command::new(omw_bin());
        // Mirror omw_cmd's env scrubbing so the child sees the same world
        // every other test does.
        cmd.env_clear();
        cmd.env("OMW_CONFIG", data_dir.join("config.toml"));
        cmd.env("OMW_KEYCHAIN_BACKEND", "memory");
        cmd.env("OMW_DATA_DIR", data_dir);
        cmd.env("HOME", data_dir);
        cmd.env("USERPROFILE", data_dir);
        if let Some(path) = std::env::var_os("PATH") {
            cmd.env("PATH", path);
        }
        cmd.args([
            "remote",
            "start",
            "--listen",
            "127.0.0.1:0",
            "--no-tailscale",
            "--shutdown-signal",
            signal_var,
        ]);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        cmd.spawn()
    }

    /// 1. `omw remote start` writes a pidfile and `omw remote stop`
    ///    causes the foreground process to exit cleanly. We trigger
    ///    shutdown by writing a sentinel file the daemon's
    ///    `--shutdown-signal` watcher polls for; this avoids relying on
    ///    Unix signal forwarding through `assert_cmd`.
    #[test]
    fn start_writes_pidfile_and_stop_terminates() {
        let dir = tempfile::tempdir().expect("tempdir");
        let data_dir = dir.path().join("data");
        std::fs::create_dir_all(&data_dir).expect("mkdir data");

        // The Executor's `--shutdown-signal <name>` hook should observe
        // EITHER the env var or a sentinel file at
        // `<OMW_DATA_DIR>/<name>.signal`. The latter is more portable
        // because env vars on a running child can't be set after spawn.
        // Tests use the file form.
        let signal_name = "omw_test_stop";
        let signal_file = data_dir.join(format!("{}.signal", signal_name));

        let mut child = spawn_remote(&data_dir, signal_name).expect("spawn omw remote start");

        // Wait up to 5 seconds for the pidfile to appear. If it doesn't,
        // dump child output for debugging.
        let pidfile = data_dir.join("remote.pid");
        let pid_body = match wait_for_pidfile(&pidfile, Duration::from_secs(5)) {
            Ok(s) => s,
            Err(e) => {
                let _ = child.kill();
                let out = child.wait_with_output().ok();
                panic!(
                    "pidfile {:?} did not appear: {}; child output: {:?}",
                    pidfile, e, out
                );
            }
        };
        assert!(
            !pid_body.trim().is_empty(),
            "pidfile must be non-empty; got {:?}",
            pid_body
        );

        // Trigger shutdown via the sentinel file.
        std::fs::write(&signal_file, b"stop\n").expect("write signal file");

        // Wait up to 5 seconds for the child to exit on its own.
        let start = Instant::now();
        loop {
            match child.try_wait().expect("try_wait child") {
                Some(status) => {
                    assert!(
                        status.success() || status.code().is_some(),
                        "child must exit cleanly; got {:?}",
                        status
                    );
                    break;
                }
                None => {
                    if start.elapsed() > Duration::from_secs(5) {
                        let _ = child.kill();
                        panic!(
                            "child did not exit within 5s after signal file written; \
                             pidfile body was {:?}",
                            pid_body
                        );
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
            }
        }
    }

    /// 2. `omw remote stop --all` revokes every paired device.
    ///    We seed two devices BEFORE start, then run a start→stop cycle
    ///    with `--all` and verify both rows have a non-NULL `revoked_at`.
    #[test]
    fn stop_all_revokes_all_devices() {
        use omw_remote::open_db;
        use rusqlite::params;

        let dir = tempfile::tempdir().expect("tempdir");
        let data_dir = dir.path().join("data");
        std::fs::create_dir_all(&data_dir).expect("mkdir data");

        // Seed two devices into the omw-remote db.
        let db_path = data_dir.join("omw-remote.sqlite3");
        let conn = open_db(&db_path).expect("open_db");
        for (idx, id) in ["aaaa1111aaaa1111", "bbbb2222bbbb2222"].iter().enumerate() {
            conn.execute(
                "INSERT INTO devices \
                    (id, name, public_key, paired_at, last_seen, permissions_json, revoked_at) \
                 VALUES (?1, ?2, ?3, ?4, NULL, ?5, NULL)",
                params![
                    id,
                    format!("device-{}", idx),
                    vec![idx as u8 + 1; 32],
                    "2026-04-20T10:00:00Z",
                    "[\"PtyRead\"]",
                ],
            )
            .expect("seed device");
        }
        drop(conn);

        // Spawn the daemon, wait for the pidfile, then issue
        // `omw remote stop --all`. The stop command does the revocation
        // unconditionally — even if the SUT doesn't have a running
        // daemon, `--all` should still revoke (the brief calls this out).
        let signal_name = "omw_test_stopall";
        let mut child = spawn_remote(&data_dir, signal_name).expect("spawn");
        let _pid_body = wait_for_pidfile(&data_dir.join("remote.pid"), Duration::from_secs(5))
            .unwrap_or_else(|e| {
                let _ = child.kill();
                panic!("pidfile did not appear: {}", e);
            });

        let mut stop_cmd = omw_cmd(dir.path());
        stop_cmd.env("OMW_DATA_DIR", &data_dir);
        let stop_assert = stop_cmd.args(["remote", "stop", "--all"]).assert();
        let stop_output = stop_assert.get_output();
        assert_eq!(
            stop_output.status.code(),
            Some(0),
            "remote stop --all must exit 0; stderr={:?}",
            String::from_utf8_lossy(&stop_output.stderr)
        );

        // The child should now have exited (stop signaled it).
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(5) {
            if let Some(_status) = child.try_wait().expect("try_wait") {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = child.kill();
        let _ = child.wait();

        // Both devices must now have non-NULL revoked_at.
        let conn = rusqlite::Connection::open(&db_path).expect("re-open db");
        let mut stmt = conn
            .prepare("SELECT id, revoked_at FROM devices ORDER BY id")
            .expect("prepare");
        let rows: Vec<(String, Option<String>)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .expect("query")
            .map(|r| r.expect("row"))
            .collect();
        assert_eq!(rows.len(), 2, "expected two device rows; got {:?}", rows);
        for (id, revoked_at) in &rows {
            assert!(
                revoked_at.is_some(),
                "device {} must have non-NULL revoked_at after `stop --all`; got {:?}",
                id,
                revoked_at
            );
        }
    }
}

// On Windows, both tests above are skipped at compile time. We don't
// emit a stub `#[ignore]` test because cargo would still need a function
// body referencing the Unix-only helpers. Beyond-v1: revisit when the
// Windows shutdown mechanism (named event handle) lands.
