//! B.2 — Write input to the PTY and confirm it round-trips through to the
//! child's stdout.
//!
//! This must distinguish a *real* round-trip (writes reach the child AND the
//! child's stdout bridges back through the PTY master) from PTY line-discipline
//! echo (where the kernel-side echo can surface the marker bytes even if writes
//! never reached the child or stdout bridging is broken).
//!
//! Strategy: have the child *transform* the input. If the child prepends
//! "ACK:" to each line, the only way "ACK:" can appear on the master read
//! side is if (a) the write reached the child and (b) the child's stdout
//! bridges through. Mere PTY echo cannot synthesize "ACK:".
//!
//! - Unix:    `sh -c "stty -echo; while IFS= read -r line; do printf 'ACK:%s\n' \"$line\"; done"`
//!            — `stty -echo` disables PTY echo, removing the alternate path
//!            entirely. Body is POSIX so `sh` works on busybox/Alpine too.
//! - Windows: same idea via PowerShell `Read-Host`. ConPTY does not give us a
//!            clean `stty -echo` analogue, so we rely on the "ACK:" prefix
//!            being unforgeable by PTY echo to disambiguate.

use omw_pty::{Pty, PtyCommand};
use tokio::time::{timeout, Duration};

fn ack_loop_cmd() -> PtyCommand {
    if cfg!(windows) {
        // Read lines and prepend "ACK:". Loop exits on EOF (Read-Host
        // returns $null on closed input).
        let script = "while ($true) { $line = Read-Host; if ($null -eq $line) { break }; Write-Host ('ACK:' + $line) }";
        PtyCommand::new("powershell")
            .arg("-NoProfile")
            .arg("-Command")
            .arg(script)
    } else {
        // `stty -echo` disables line-discipline echo so we cannot accidentally
        // see the marker bytes via the kernel's echo path. The only way to
        // see "ACK:<marker>" on the master is via the child's printf.
        // Use `sh` (not `bash`) — the script body is POSIX, and `sh` widens
        // compatibility to busybox/Alpine systems that lack bash.
        PtyCommand::new("sh").arg("-c").arg(
            "stty -echo; while IFS= read -r line; do printf 'ACK:%s\\n' \"$line\"; done",
        )
    }
}

#[tokio::test]
async fn write_round_trips_through_child_stdout() {
    let mut pty = Pty::spawn(ack_loop_cmd())
        .await
        .expect("spawn should succeed");

    let mut reader = pty.reader().expect("reader should be available once");
    let mut writer = pty.writer().expect("writer should be available once");

    // Marker unlikely to appear in any shell prompt or banner.
    let marker = "omw-pty-test";
    let expected = format!("ACK:{marker}");

    // Give the child a moment to apply `stty -echo` before we send the line,
    // so we don't race the echo state.
    tokio::time::sleep(Duration::from_millis(200)).await;

    let line = format!("{marker}\r\n");
    writer
        .write_all(line.as_bytes())
        .await
        .expect("write_all to PTY master must succeed");

    // Read until we see "ACK:<marker>", with a hard timeout.
    let saw_ack = timeout(Duration::from_secs(5), async {
        let mut acc = Vec::<u8>::new();
        let mut buf = [0u8; 1024];
        loop {
            match reader.read(&mut buf).await {
                Ok(0) => return false, // EOF before we saw it
                Ok(n) => {
                    acc.extend_from_slice(&buf[..n]);
                    if acc
                        .windows(expected.len())
                        .any(|w| w == expected.as_bytes())
                    {
                        return true;
                    }
                }
                Err(_) => return false,
            }
        }
    })
    .await
    .expect("did not see ACK:<marker> within 5s");

    assert!(
        saw_ack,
        "expected child to transform input into {expected:?} on stdout. \
         Seeing this fail means writes did not reach the child, or the child's \
         stdout did not bridge back through the PTY master."
    );

    // Drop the writer so the child sees EOF on Unix; on Windows we kill below.
    drop(writer);

    pty.kill().expect("kill should succeed (or be a no-op if exited)");

    let _status = timeout(Duration::from_secs(5), pty.wait())
        .await
        .expect("wait did not return within 5s after kill")
        .expect("wait returned an error");
}
