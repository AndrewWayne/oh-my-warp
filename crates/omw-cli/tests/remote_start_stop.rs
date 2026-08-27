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
//! These tests use the cross-platform shutdown-sentinel file recorded in
//! the pidfile, so the same contract runs on Unix and Windows.

mod common;

mod start_stop {
    use std::ops::{Deref, DerefMut};
    use std::path::PathBuf;
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};

    use crate::common::omw_cmd;

    // Failure-path cleanup; the tests still assert graceful public shutdown.
    struct ChildGuard(Child);

    impl ChildGuard {
        fn new(child: Child) -> Self {
            Self(child)
        }
    }

    impl Deref for ChildGuard {
        type Target = Child;

        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }

    impl DerefMut for ChildGuard {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.0
        }
    }

    impl Drop for ChildGuard {
        fn drop(&mut self) {
            if !matches!(self.0.try_wait(), Ok(Some(_))) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
    }

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

    fn spawn_remote(data_dir: &std::path::Path, signal_var: &str) -> std::io::Result<Child> {
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
        // WinSock provider initialization depends on SystemRoot. Preserve
        // only this required system value after env_clear; provider/config
        // variables remain isolated by the explicit test environment.
        if let Some(system_root) = std::env::var_os("SYSTEMROOT") {
            cmd.env("SYSTEMROOT", system_root);
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

        // The `--shutdown-signal <name>` hook records a portable sentinel
        // file at `<OMW_DATA_DIR>/<name>.signal` in the pidfile.
        let signal_name = "omw_test_stop";

        let mut child =
            ChildGuard::new(spawn_remote(&data_dir, signal_name).expect("spawn omw remote start"));

        // Wait up to 5 seconds for the pidfile to appear. If it doesn't,
        // dump child output for debugging.
        let pidfile = data_dir.join("remote.pid");
        let pid_body = match wait_for_pidfile(&pidfile, Duration::from_secs(5)) {
            Ok(s) => s,
            Err(e) => {
                let _ = child.kill();
                let status = child.wait().ok();
                panic!(
                    "pidfile {:?} did not appear: {}; child status: {:?}",
                    pidfile, e, status
                );
            }
        };
        assert!(
            !pid_body.trim().is_empty(),
            "pidfile must be non-empty; got {:?}",
            pid_body
        );

        let expected_port = pid_body
            .lines()
            .find_map(|line| line.strip_prefix("port="))
            .and_then(|port| port.parse::<u16>().ok())
            .expect("pidfile must contain a numeric port");
        let mut status_cmd = omw_cmd(dir.path());
        status_cmd.env("OMW_DATA_DIR", &data_dir);
        let status_assert = status_cmd.args(["remote", "status"]).assert();
        let status_output = status_assert.get_output();
        assert_eq!(
            status_output.status.code(),
            Some(0),
            "remote status must exit 0; stderr={:?}",
            String::from_utf8_lossy(&status_output.stderr)
        );
        let status_text = String::from_utf8_lossy(&status_output.stdout);
        assert!(
            status_text.contains("running")
                && status_text.contains(&format!("127.0.0.1:{expected_port}")),
            "live status must include running state and exact bound port; got {:?}",
            status_text
        );

        // Exercise the public stop command. It reads the pidfile and writes
        // the sentinel file used by the foreground process.
        let mut stop_cmd = omw_cmd(dir.path());
        stop_cmd.env("OMW_DATA_DIR", &data_dir);
        let stop_assert = stop_cmd.args(["remote", "stop"]).assert();
        let stop_output = stop_assert.get_output();
        assert_eq!(
            stop_output.status.code(),
            Some(0),
            "remote stop must exit 0; stderr={:?}",
            String::from_utf8_lossy(&stop_output.stderr)
        );

        // Wait up to 5 seconds for the child to exit on its own.
        let start = Instant::now();
        loop {
            match child.try_wait().expect("try_wait child") {
                Some(status) => {
                    assert!(
                        status.success(),
                        "child must exit cleanly; got {:?}",
                        status
                    );
                    assert!(
                        !pidfile.exists(),
                        "pidfile must be removed after graceful shutdown"
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
        let mut child = ChildGuard::new(spawn_remote(&data_dir, signal_name).expect("spawn"));
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
        let child_status = loop {
            if let Some(status) = child.try_wait().expect("try_wait") {
                break status;
            }
            if start.elapsed() >= Duration::from_secs(5) {
                let _ = child.kill();
                let _ = child.wait();
                panic!("child did not exit within 5s after `remote stop --all`");
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        assert!(
            child_status.success(),
            "child must exit cleanly; got {:?}",
            child_status
        );
        assert!(
            !data_dir.join("remote.pid").exists(),
            "pidfile must be removed after graceful shutdown"
        );

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
