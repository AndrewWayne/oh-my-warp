//! Real-byte capture: spawn `claude` in a PTY, type `/exit` slowly, dump
//! every byte the child emits to a fixture file.
//!
//! Goal: produce ground-truth byte stream so the xterm.js test in
//! `apps/web-controller/tests/` can replay them and reproduce the
//! duplicate-render the user sees on the phone, without manual capture.
//!
//! Skipped unless OMW_CAPTURE_CLAUDE=1 is set — it spawns a real claude
//! subprocess, which only succeeds on a developer machine where claude is
//! installed AND authenticated. Cannot run on CI.
//!
//! Output: writes to env var OMW_CAPTURE_OUT (or `target/claude-capture.bin`
//! by default). Replay-friendly: just feed bytes into xterm via term.write.

use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

use omw_pty::{Pty, PtyCommand};

#[tokio::test]
async fn capture_claude_exit_hint_keystrokes() {
    if std::env::var_os("OMW_CAPTURE_CLAUDE").is_none() {
        eprintln!("OMW_CAPTURE_CLAUDE not set; skipping");
        return;
    }

    // Path to claude. On Windows it's `claude.cmd` in npm's bin dir; on
    // Unix it's `claude`. We try a few likely locations.
    let claude = locate_claude();
    eprintln!("[capture] using claude = {:?}", claude);

    // Output fixture path.
    let out_path: PathBuf = std::env::var_os("OMW_CAPTURE_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let mut p = std::env::current_dir().unwrap();
            p.push("target");
            p.push("claude-capture.bin");
            p
        });
    eprintln!("[capture] dumping to {:?}", out_path);

    // Spawn claude. PtyCommand defaults to inheriting env, which is what we
    // want — claude pulls auth from $HOME/.config/claude or similar.
    let cmd = PtyCommand::new(&claude).size(149, 39);
    let mut pty = Pty::spawn(cmd).await.expect("Pty::spawn claude");
    let mut reader = pty.reader().expect("reader");
    let mut writer = pty.writer().expect("writer");

    // Ensure parent dir exists (target/ may not be present from this
    // crate's POV when we're invoked via the workspace target dir).
    if let Some(parent) = out_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    // Open the output file.
    let mut out_file = std::fs::File::create(&out_path).expect("create out file");

    // Collector task: read bytes from PTY into a shared buffer.
    let collected: std::sync::Arc<std::sync::Mutex<Vec<u8>>> =
        std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let collector = collected.clone();
    let reader_task = tokio::spawn(async move {
        let mut buf = vec![0u8; 4096];
        loop {
            match reader.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => {
                    let mut lock = collector.lock().unwrap();
                    lock.extend_from_slice(&buf[..n]);
                }
                Err(_) => break,
            }
        }
    });

    // Wait for claude's "trust this folder" prompt to render.
    tokio::time::sleep(Duration::from_secs(3)).await;
    let after_trust = collected.lock().unwrap().len();
    eprintln!("[capture] {} bytes after initial settle", after_trust);

    // Bypass trust prompt: select option 1 (trust) + Enter.
    writer.write_all(b"1\r").await.expect("trust prompt");
    tokio::time::sleep(Duration::from_secs(2)).await;
    let after_trusted = collected.lock().unwrap().len();
    eprintln!("[capture] {} bytes after trusting folder", after_trusted);

    // Mark a separator in the file so we know where keystrokes start.
    {
        let mut lock = collected.lock().unwrap();
        lock.extend_from_slice(b"\n----- KEYSTROKES BEGIN -----\n");
    }

    // Now type `/exit` keystroke-by-keystroke with 400ms gaps so claude
    // emits a separate hint-update frame for each letter.
    for ch in "/exit".chars() {
        writer
            .write_all(&[ch as u8])
            .await
            .expect("write keystroke");
        tokio::time::sleep(Duration::from_millis(400)).await;
    }
    // Wait extra time so the final hint frame fully renders before we cut.
    tokio::time::sleep(Duration::from_millis(500)).await;
    eprintln!(
        "[capture] {} bytes after typing /exit",
        collected.lock().unwrap().len()
    );
    // Mark end-of-typing — the xterm test will replay everything UP TO
    // here, ignoring whatever Ctrl-C produces afterward.
    {
        let mut lock = collected.lock().unwrap();
        lock.extend_from_slice(b"\n----- KEYSTROKES END -----\n");
    }

    // Send Ctrl-C twice to abort the line and exit.
    writer.write_all(&[0x03]).await.expect("ctrl-c");
    tokio::time::sleep(Duration::from_millis(300)).await;
    writer.write_all(&[0x03]).await.expect("ctrl-c");
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Dump everything to the fixture file.
    let final_bytes = collected.lock().unwrap().clone();
    out_file.write_all(&final_bytes).expect("write fixture");
    eprintln!("[capture] wrote {} bytes to {:?}", final_bytes.len(), out_path);

    // Cleanup. Kill claude; await reader task to finish.
    let _ = pty.kill();
    let _ = tokio::time::timeout(Duration::from_secs(2), pty.wait()).await;
    drop(writer);
    drop(pty);
    reader_task.abort();
    let _ = reader_task.await;
}

fn locate_claude() -> PathBuf {
    // Honor explicit override.
    if let Some(p) = std::env::var_os("OMW_CLAUDE_BIN") {
        return PathBuf::from(p);
    }
    // Windows: %APPDATA%\npm\claude.cmd
    if cfg!(windows) {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            let p = PathBuf::from(appdata).join("npm").join("claude.cmd");
            if p.exists() {
                return p;
            }
        }
        return PathBuf::from("claude.cmd");
    }
    // Unix: just `claude` in PATH.
    PathBuf::from("claude")
}
