//! `omw ask <prompt>` — spawn `omw-agent ask <prompt> [flags]` and forward
//! stdio + exit code.
//!
//! The Rust half here is a thin shell: it locates the agent binary via
//! `OMW_AGENT_BIN` (default `omw-agent` on PATH), spawns it with the
//! prompt + forwarded flags, propagates the relevant env vars, streams
//! the child's stdout/stderr back to the caller's sinks, and returns
//! the child's exit code as the result.
//!
//! All provider HTTP, keychain resolution, and usage telemetry live in
//! the TS half (`apps/omw-agent/src/cli.ts`).

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;

use anyhow::{anyhow, Context};
use clap::Args;

#[derive(Args, Debug)]
pub struct AskArgs {
    /// Prompt to send to the model.
    pub prompt: String,
    /// Provider id (overrides `default_provider`).
    #[arg(long)]
    pub provider: Option<String>,
    /// Model name (overrides the provider's `default_model`).
    #[arg(long)]
    pub model: Option<String>,
    /// Maximum tokens to generate.
    #[arg(long)]
    pub max_tokens: Option<u32>,
    /// Sampling temperature.
    #[arg(long)]
    pub temperature: Option<f32>,
}

/// `omw ask` exit code on internal spawn/IO failures (distinct from any
/// exit code the agent may legitimately return).
const SPAWN_FAIL_EXIT: i32 = 127;

/// Run the handler. Returns the exit code that the binary wrapper would
/// `exit()` with.
pub(crate) fn run(args: AskArgs, stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    match run_inner(args, stdout, stderr) {
        Ok(code) => code,
        Err(e) => {
            let _ = writeln!(stderr, "error: {e:#}");
            SPAWN_FAIL_EXIT
        }
    }
}

fn run_inner(args: AskArgs, stdout: &mut dyn Write, stderr: &mut dyn Write) -> anyhow::Result<i32> {
    let bin = std::env::var("OMW_AGENT_BIN").unwrap_or_else(|_| "omw-agent".to_string());

    let mut child_argv: Vec<String> = vec!["ask".to_string(), args.prompt.clone()];
    if let Some(p) = &args.provider {
        child_argv.push("--provider".to_string());
        child_argv.push(p.clone());
    }
    if let Some(m) = &args.model {
        child_argv.push("--model".to_string());
        child_argv.push(m.clone());
    }
    if let Some(n) = args.max_tokens {
        child_argv.push("--max-tokens".to_string());
        child_argv.push(n.to_string());
    }
    if let Some(t) = args.temperature {
        child_argv.push("--temperature".to_string());
        child_argv.push(format_temperature(t));
    }

    let mut cmd = Command::new(&bin);
    cmd.args(&child_argv);
    // stdin closed; stdout/stderr piped so we can forward to caller's sinks.
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    // On Windows, the agent (Node.js) cannot initialize without
    // `SystemRoot` (CSPRNG looks up the OS RNG via DLLs in
    // `%SystemRoot%\System32`). The parent may have been launched with
    // a stripped env block (e.g. cargo-test harnesses calling
    // `env_clear`); in that case we inject a sane default so the agent
    // can still start. Tracked upstream as rust-lang/rust#114737.
    #[cfg(windows)]
    {
        if std::env::var_os("SystemRoot").is_none() {
            cmd.env("SystemRoot", r"C:\Windows");
        }
    }

    // Forward the env vars the contract requires. We rely on inheritance
    // for the rest (PATH, etc.) — Command does not clear the env block by
    // default, so OMW_CONFIG / OMW_KEYCHAIN_HELPER / OMW_KEYCHAIN_BACKEND
    // already pass through. Re-setting them explicitly would be a no-op
    // when inherited; we still touch the relevant keys to make the
    // forwarding contract explicit.
    for key in ["OMW_CONFIG", "OMW_KEYCHAIN_HELPER", "OMW_KEYCHAIN_BACKEND"] {
        if let Ok(v) = std::env::var(key) {
            cmd.env(key, v);
        }
    }

    let mut child = cmd
        .spawn()
        .with_context(|| format!("spawning agent binary `{bin}`"))?;

    // Stream stdout and stderr concurrently. We spawn one thread per
    // stream that pipes raw bytes back to a channel; the main thread
    // drains the channels onto the caller's `Write` sinks.
    //
    // `&mut dyn Write` is not Send, so we cannot move the sinks into the
    // worker threads. Instead, the workers read into Vec<u8> chunks and
    // we write them on the main thread.
    let (stdout_tx, stdout_rx) = mpsc::channel::<Vec<u8>>();
    let (stderr_tx, stderr_rx) = mpsc::channel::<Vec<u8>>();

    let mut child_stdout = child.stdout.take();
    let mut child_stderr = child.stderr.take();

    let stdout_thread = thread::spawn(move || -> std::io::Result<()> {
        if let Some(s) = child_stdout.as_mut() {
            pump(s, &stdout_tx)?;
        }
        Ok(())
    });
    let stderr_thread = thread::spawn(move || -> std::io::Result<()> {
        if let Some(s) = child_stderr.as_mut() {
            pump(s, &stderr_tx)?;
        }
        Ok(())
    });

    // Drain both channels concurrently on the main thread. We alternate
    // try_recv polls so neither sink starves the other; once a worker
    // thread finishes its sender is dropped and try_recv returns
    // Disconnected.
    let mut stdout_open = true;
    let mut stderr_open = true;
    while stdout_open || stderr_open {
        let mut made_progress = false;
        if stdout_open {
            match stdout_rx.try_recv() {
                Ok(chunk) => {
                    let _ = stdout.write_all(&chunk);
                    made_progress = true;
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    stdout_open = false;
                }
            }
        }
        if stderr_open {
            match stderr_rx.try_recv() {
                Ok(chunk) => {
                    let _ = stderr.write_all(&chunk);
                    made_progress = true;
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    stderr_open = false;
                }
            }
        }
        if !made_progress && (stdout_open || stderr_open) {
            // Yield briefly to avoid a busy spin while both channels are
            // empty. A 1ms park is negligible relative to network I/O
            // and keeps CPU near zero in the steady state.
            thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    let _ = stdout_thread.join();
    let _ = stderr_thread.join();

    // After both pump threads finish, wait for the child.
    let status = child.wait().context("waiting for agent to exit")?;
    let code = status
        .code()
        .ok_or_else(|| anyhow!("agent terminated by signal without an exit code"))?;
    Ok(code)
}

fn pump(stream: &mut impl Read, tx: &mpsc::Sender<Vec<u8>>) -> std::io::Result<()> {
    let mut buf = [0u8; 4096];
    loop {
        match stream.read(&mut buf) {
            Ok(0) => return Ok(()),
            Ok(n) => {
                if tx.send(buf[..n].to_vec()).is_err() {
                    return Ok(());
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
}

/// Render the temperature flag value for the child argv. Default `Display`
/// for f32 already produces the shortest round-trip representation
/// (`0.5`, `1`, `0.7`) — which matches what the contract expects. We
/// avoid fixed precision so `--temperature 0.5` round-trips verbatim.
fn format_temperature(t: f32) -> String {
    t.to_string()
}
