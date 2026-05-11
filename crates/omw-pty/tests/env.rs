use std::time::Duration;

use omw_pty::{Pty, PtyCommand};
use tokio::time::timeout;

#[tokio::test]
async fn env_remove_strips_inherited_env() {
    let key = "OMW_PTY_ENV_REMOVE_TEST";
    std::env::set_var(key, "leaked");

    let cmd = if cfg!(windows) {
        PtyCommand::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "if ($env:OMW_PTY_ENV_REMOVE_TEST) { Write-Output $env:OMW_PTY_ENV_REMOVE_TEST } else { Write-Output unset }",
            ])
            .env_remove(key)
    } else {
        PtyCommand::new("/bin/sh")
            .args(["-c", "printf '<%s>' \"${OMW_PTY_ENV_REMOVE_TEST-unset}\""])
            .env_remove(key)
    };

    let mut pty = Pty::spawn(cmd).await.expect("spawn env probe");
    let mut reader = pty.reader().expect("reader once");
    let mut out = Vec::new();

    timeout(Duration::from_secs(5), async {
        let mut buf = [0u8; 256];
        loop {
            let n = reader.read(&mut buf).await.expect("read env probe");
            if n == 0 {
                break;
            }
            out.extend_from_slice(&buf[..n]);
        }
    })
    .await
    .expect("env probe should exit");

    std::env::remove_var(key);
    let text = String::from_utf8_lossy(&out);
    assert!(
        text.contains("unset"),
        "removed env key must not reach child; output was {text:?}"
    );
}
