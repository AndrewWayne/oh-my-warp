//! B.4 — `kill()` followed by `wait()` returns within a bounded time and
//! reports an abnormal exit.

use omw_pty::{Pty, PtyCommand};
#[cfg(windows)]
use portable_pty::{native_pty_system, CommandBuilder, PtySize as PortablePtySize};
#[cfg(windows)]
use std::time::Instant;
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

#[cfg(windows)]
#[tokio::test]
async fn portable_pty_killer_reports_success_after_terminating_child() {
    let pair = native_pty_system()
        .openpty(PortablePtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("open Windows PTY");
    let mut command = CommandBuilder::new("powershell.exe");
    command.args([
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        "Start-Sleep -Seconds 60",
    ]);
    let mut child = pair
        .slave
        .spawn_command(command)
        .expect("spawn long-running child");
    assert!(
        child.try_wait().expect("query child before kill").is_none(),
        "child exited before the kill contract could be tested"
    );

    let mut killer = child.clone_killer();
    let kill_result = killer.kill();

    // TerminateProcess is asynchronous. Reap before asserting the return
    // contract so a failing regression never leaves the child running.
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.try_wait().expect("query child after kill") {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            panic!("child was still running 5s after kill");
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    };

    kill_result.expect("successful TerminateProcess must return Ok");
    assert!(!status.success(), "killed child unexpectedly succeeded");
}

#[tokio::test]
async fn kill_then_wait_completes_quickly_with_abnormal_exit() {
    let mut pty = Pty::spawn(long_running_cmd())
        .await
        .expect("spawn should succeed");

    pty.kill().expect("kill should succeed for a live child");

    let status = timeout(Duration::from_secs(5), pty.wait())
        .await
        .expect("wait must return within 5s after kill")
        .expect("wait returned an error");

    // A killed child must NOT report success.
    assert!(
        !status.success(),
        "expected non-success after kill, got status {:?}",
        status
    );

    // If a code is reported, it must not be 0. On Unix portable-pty surfaces
    // signal-killed processes as `code = None`; that is also acceptable.
    if let Some(code) = status.code() {
        assert_ne!(code, 0, "killed child should not report exit code 0");
    }
}

#[tokio::test]
async fn kill_is_idempotent_after_exit() {
    let mut pty = Pty::spawn(long_running_cmd())
        .await
        .expect("spawn should succeed");

    pty.kill().expect("first kill should succeed");
    let _ = timeout(Duration::from_secs(5), pty.wait())
        .await
        .expect("wait must return within 5s after first kill")
        .expect("wait returned an error");

    // Per lib.rs:111 docs: kill is idempotent if the child has already
    // exited. Pin the contract: the second kill MUST return Ok, not Err.
    pty.kill().expect(
        "second kill on an already-reaped child must return Ok per lib.rs:111 idempotency contract",
    );
}
