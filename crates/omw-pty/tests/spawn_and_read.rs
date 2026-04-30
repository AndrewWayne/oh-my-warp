//! B.1 — Spawn a child that prints "hello" and read it back through the PTY.
//!
//! Cross-platform: uses `cmd /c echo hello` on Windows, `/bin/sh -c 'printf hello'`
//! elsewhere. PTY echoes typically CR-cook output to `\r\n`, so we assert
//! containment of the substring "hello" rather than exact bytes.

use omw_pty::{Pty, PtyCommand};
use tokio::time::{timeout, Duration};

fn echo_hello_cmd() -> PtyCommand {
    if cfg!(windows) {
        PtyCommand::new("cmd").arg("/c").arg("echo hello")
    } else {
        PtyCommand::new("/bin/sh").arg("-c").arg("printf hello")
    }
}

#[tokio::test]
async fn spawn_prints_hello_and_exits_successfully() {
    let mut pty = Pty::spawn(echo_hello_cmd())
        .await
        .expect("spawn should succeed for a trivial echo command");

    let mut reader = pty.reader().expect("reader should be available once");

    // Drain output until EOF or until we see "hello", with an overall timeout
    // so a hang surfaces as a clear test failure rather than hanging CI.
    let collected = timeout(Duration::from_secs(5), async {
        let mut acc = Vec::<u8>::new();
        let mut buf = [0u8; 1024];
        loop {
            match reader.read(&mut buf).await {
                Ok(0) => break, // EOF
                Ok(n) => {
                    acc.extend_from_slice(&buf[..n]);
                    if acc
                        .windows(b"hello".len())
                        .any(|w| w == b"hello")
                    {
                        // Got it — but keep draining briefly so the child can
                        // finish writing and we get a clean EOF.
                        // Continue reading until EOF.
                        continue;
                    }
                }
                Err(e) => panic!("read failed: {e}"),
            }
        }
        acc
    })
    .await
    .expect("read loop did not hit EOF within 5s");

    let as_str = String::from_utf8_lossy(&collected);
    assert!(
        as_str.contains("hello"),
        "expected output to contain \"hello\", got: {as_str:?}"
    );

    let status = timeout(Duration::from_secs(5), pty.wait())
        .await
        .expect("wait did not return within 5s")
        .expect("wait returned an error");

    assert!(
        status.success(),
        "expected successful exit, got {:?}",
        status
    );
    // On platforms that report it, the code should be 0.
    if let Some(code) = status.code() {
        assert_eq!(code, 0, "expected exit code 0, got {code}");
    }
}
