//! B.5 — Dropping a `Pty` without explicit kill or wait must not hang and must
//! actually terminate the child (no leak), plus the once-only contracts on
//! `reader()` / `writer()`.
//!
//! Finding 3: a "fast drop" that returns within 3s but leaves the child
//! running passes a timing-only check. We strengthen by giving the child an
//! observable side effect (heartbeat appended to a tempfile); after drop the
//! file size MUST stop growing.
//!
//! Finding 4: lib.rs documents that `reader()` and `writer()` are once-only
//! (`Some` then `None`). Pin that contract here.

use omw_pty::{Pty, PtyCommand};
use std::time::Duration as StdDuration;
use tokio::time::{timeout, Duration};

fn long_running_cmd() -> PtyCommand {
    if cfg!(windows) {
        PtyCommand::new("cmd")
            .arg("/c")
            .arg("timeout /t 60 /nobreak")
    } else {
        PtyCommand::new("/bin/sh").arg("-c").arg("sleep 60")
    }
}

fn heartbeat_cmd(path: &str) -> PtyCommand {
    if cfg!(windows) {
        // Windows can sleep sub-second, but we mirror the Unix 1s cadence so
        // both branches use the same warmup/post-drop windows downstream.
        let script = format!(
            "while ($true) {{ Add-Content -LiteralPath '{}' -Value 'h'; Start-Sleep -Seconds 1 }}",
            path.replace('\'', "''")
        );
        PtyCommand::new("powershell")
            .arg("-NoProfile")
            .arg("-Command")
            .arg(script)
    } else {
        // POSIX `sleep` only takes integer seconds (busybox/Alpine), so we
        // use `sleep 1` rather than `sleep 0.1`. Warmup and post-drop windows
        // below are widened to match this 1s cadence.
        let script = format!(
            "while true; do echo h >> '{}'; sleep 1; done",
            path.replace('\'', "'\\''")
        );
        PtyCommand::new("/bin/sh").arg("-c").arg(script)
    }
}

#[tokio::test]
async fn drop_without_kill_or_wait_does_not_hang() {
    let pty = Pty::spawn(long_running_cmd())
        .await
        .expect("spawn should succeed");

    let drop_task = tokio::task::spawn_blocking(move || {
        // Implicit drop at end of scope; this is the path under test.
        drop(pty);
    });

    timeout(Duration::from_secs(3), drop_task)
        .await
        .expect("drop without kill/wait must complete in <3s")
        .expect("drop task itself must not panic");
}

/// Finding 3 — drop must actually kill the child. We use a heartbeat file:
/// after drop, the file must stop growing. A fast-but-leaky drop (returns
/// quickly but leaves the child alive) fails this assertion.
#[tokio::test]
async fn drop_actually_terminates_child() {
    let tmp = tempfile::NamedTempFile::new().expect("create tempfile");
    let path = tmp.path().to_path_buf();
    drop(tmp); // Release the handle so Windows allows other writers.
    let path_str = path
        .to_str()
        .expect("tempfile path should be valid UTF-8")
        .to_owned();

    let pty = Pty::spawn(heartbeat_cmd(&path_str))
        .await
        .expect("spawn heartbeat child");

    // Heartbeat warmup — child sleeps 1s between writes (POSIX `sleep`
    // takes integer seconds), so 2s gives at least one write reliably.
    tokio::time::sleep(Duration::from_secs(2)).await;

    let size_before_drop = std::fs::metadata(&path)
        .expect("heartbeat file must exist before drop")
        .len();
    assert!(
        size_before_drop > 0,
        "heartbeat child should have written something before drop, but file is empty"
    );

    let drop_task = tokio::task::spawn_blocking(move || drop(pty));
    timeout(Duration::from_secs(3), drop_task)
        .await
        .expect("drop must not hang")
        .expect("drop task should not panic");

    // Brief settle window so the OS can reap the child after drop, then
    // sample the file size as our "post-drop baseline."
    tokio::time::sleep(StdDuration::from_millis(200)).await;
    let size_after_drop = std::fs::metadata(&path)
        .expect("heartbeat file must still exist after drop")
        .len();

    // Wait one full heartbeat cycle (1s) plus margin. A leaky impl that left
    // the child alive will append at least one more 'h\n' in this window.
    tokio::time::sleep(Duration::from_secs(2)).await;
    let size_final = std::fs::metadata(&path)
        .expect("heartbeat file must still exist at end of test")
        .len();

    assert_eq!(
        size_after_drop, size_final,
        "heartbeat file kept growing after drop ({} -> {}) — child was not killed when Pty was dropped",
        size_after_drop, size_final
    );

    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn back_to_back_spawn_then_drop_does_not_hang() {
    // Smoke test that the spawn+drop cycle is stable across iterations —
    // catches obvious leaks of threads or file handles in the bridge.
    for i in 0..3 {
        let pty = Pty::spawn(long_running_cmd())
            .await
            .unwrap_or_else(|e| panic!("spawn iteration {i} failed: {e}"));
        let drop_task = tokio::task::spawn_blocking(move || drop(pty));
        timeout(Duration::from_secs(3), drop_task)
            .await
            .unwrap_or_else(|_| panic!("drop iteration {i} hung"))
            .unwrap_or_else(|e| panic!("drop iteration {i} panicked: {e}"));
    }
}

/// Finding 4 — `reader()` and `writer()` are documented as once-only. Pin it.
#[tokio::test]
async fn reader_and_writer_are_once_only() {
    let mut pty = Pty::spawn(long_running_cmd())
        .await
        .expect("spawn should succeed");

    let first_reader = pty.reader();
    assert!(
        first_reader.is_some(),
        "first reader() call must return Some per lib.rs:93 contract"
    );

    let second_reader = pty.reader();
    assert!(
        second_reader.is_none(),
        "second reader() call must return None per lib.rs:93 once-only contract"
    );

    let first_writer = pty.writer();
    assert!(
        first_writer.is_some(),
        "first writer() call must return Some per lib.rs:99 contract"
    );

    let second_writer = pty.writer();
    assert!(
        second_writer.is_none(),
        "second writer() call must return None per lib.rs:99 once-only contract"
    );

    // Drop halves to release any internal references before we kill.
    drop(first_reader);
    drop(first_writer);

    pty.kill().expect("kill should succeed");
    let _ = timeout(Duration::from_secs(5), pty.wait())
        .await
        .expect("wait did not return within 5s after kill");
}
