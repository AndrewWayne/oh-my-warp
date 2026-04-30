//! Shared per-turn runner used by both `omw ask` (single turn) and
//! `omw agent` (REPL loop, one turn per stdin line).
//!
//! Spawns the resolved `omw-agent` binary with `ask <prompt> [flags]`,
//! streams stdout/stderr concurrently to the caller's sinks, and on a
//! clean exit best-effort parses the last stderr line as a usage JSON
//! payload to record into the local SQLite store.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

use anyhow::{anyhow, Context};
use serde::Deserialize;

use crate::db;

/// Per-turn options shared by `ask` and `agent`. Field semantics mirror
/// `AskArgs` but the type is owned by this module so the REPL can build
/// it once and reuse it across turns.
#[derive(Clone, Debug, Default)]
pub struct AgentOpts {
    pub agent_bin: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f64>,
    pub cwd: Option<PathBuf>,
}

/// `omw ask` exit code on internal spawn/IO failures (distinct from any
/// exit code the agent may legitimately return).
const SPAWN_FAIL_EXIT: i32 = 127;

/// Run a single agent turn. Returns the child's exit code on a successful
/// spawn/wait, or `SPAWN_FAIL_EXIT` if the runner itself failed (with a
/// diagnostic written to `stderr`).
pub fn run_one_turn(
    prompt: &str,
    opts: &AgentOpts,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> i32 {
    match run_inner(prompt, opts, stdout, stderr) {
        Ok(code) => code,
        Err(e) => {
            let _ = writeln!(stderr, "error: {e:#}");
            SPAWN_FAIL_EXIT
        }
    }
}

fn run_inner(
    prompt: &str,
    opts: &AgentOpts,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> anyhow::Result<i32> {
    let bin = opts
        .agent_bin
        .clone()
        .or_else(|| std::env::var("OMW_AGENT_BIN").ok())
        .unwrap_or_else(|| "omw-agent".to_string());

    let mut child_argv: Vec<String> = vec!["ask".to_string(), prompt.to_string()];
    if let Some(p) = &opts.provider {
        child_argv.push("--provider".to_string());
        child_argv.push(p.clone());
    }
    if let Some(m) = &opts.model {
        child_argv.push("--model".to_string());
        child_argv.push(m.clone());
    }
    if let Some(n) = opts.max_tokens {
        child_argv.push("--max-tokens".to_string());
        child_argv.push(n.to_string());
    }
    if let Some(t) = opts.temperature {
        child_argv.push("--temperature".to_string());
        child_argv.push(t.to_string());
    }

    let mut cmd = Command::new(&bin);
    cmd.args(&child_argv);
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    if let Some(cwd) = &opts.cwd {
        cmd.current_dir(cwd);
    }

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
    let (stdout_tx, stdout_rx) = mpsc::channel::<Vec<u8>>();
    let (stderr_tx, stderr_rx) = mpsc::channel::<Vec<u8>>();

    let mut child_stdout = child.stdout.take();
    let mut child_stderr = child.stderr.take();

    // Tee child stderr into a shared buffer so we can extract the LAST
    // line as a candidate usage-JSON payload after the child exits. We
    // still forward every byte to the caller's stderr in real time.
    let stderr_capture: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    let stderr_capture_worker = Arc::clone(&stderr_capture);

    let stdout_thread = thread::spawn(move || -> std::io::Result<()> {
        if let Some(s) = child_stdout.as_mut() {
            pump(s, &stdout_tx)?;
        }
        Ok(())
    });
    let stderr_thread = thread::spawn(move || -> std::io::Result<()> {
        if let Some(s) = child_stderr.as_mut() {
            pump_with_capture(s, &stderr_tx, &stderr_capture_worker)?;
        }
        Ok(())
    });

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
            thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    let _ = stdout_thread.join();
    let _ = stderr_thread.join();

    let status = child.wait().context("waiting for agent to exit")?;
    let code = status
        .code()
        .ok_or_else(|| anyhow!("agent terminated by signal without an exit code"))?;

    if code == 0 {
        let buf = stderr_capture.lock().map(|g| g.clone()).unwrap_or_default();
        if let Some(line) = last_nonempty_line(&buf) {
            try_record_usage(line, opts, stderr);
        }
    }

    Ok(code)
}

fn last_nonempty_line(buf: &[u8]) -> Option<&str> {
    let s = std::str::from_utf8(buf).ok()?;
    s.lines().rev().find(|l| !l.trim().is_empty())
}

#[derive(Debug, Deserialize)]
struct UsageJson {
    prompt_tokens: u64,
    completion_tokens: u64,
    total_tokens: u64,
    provider: String,
    model: String,
    duration_ms: u64,
}

fn try_record_usage(line: &str, opts: &AgentOpts, stderr: &mut dyn Write) {
    let usage: UsageJson = match serde_json::from_str(line) {
        Ok(u) => u,
        Err(_) => return,
    };

    let provider_id = opts
        .provider
        .clone()
        .unwrap_or_else(|| usage.provider.clone());

    let provider_kind = match resolve_provider_kind(&provider_id) {
        Some(k) => k,
        None => {
            let _ = writeln!(
                stderr,
                "warning: usage recorded for provider id `{provider_id}` whose kind could not be resolved"
            );
            return;
        }
    };

    let conn = match db::open() {
        Ok(c) => c,
        Err(_) => return,
    };

    let rec = db::UsageRecord {
        provider_id,
        provider_kind,
        model: usage.model,
        prompt_tokens: usage.prompt_tokens,
        completion_tokens: usage.completion_tokens,
        total_tokens: usage.total_tokens,
        duration_ms: usage.duration_ms,
    };
    let _ = db::record_usage(&conn, &rec);
}

fn resolve_provider_kind(provider_id: &str) -> Option<String> {
    let path = omw_config::config_path().ok()?;
    let cfg = omw_config::Config::load_from(&path).ok()?;
    let pid: omw_config::ProviderId = provider_id.parse().ok()?;
    cfg.providers.get(&pid).map(|p| p.kind_str().to_string())
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

fn pump_with_capture(
    stream: &mut impl Read,
    tx: &mpsc::Sender<Vec<u8>>,
    capture: &Arc<Mutex<Vec<u8>>>,
) -> std::io::Result<()> {
    let mut buf = [0u8; 4096];
    loop {
        match stream.read(&mut buf) {
            Ok(0) => return Ok(()),
            Ok(n) => {
                if let Ok(mut g) = capture.lock() {
                    g.extend_from_slice(&buf[..n]);
                }
                if tx.send(buf[..n].to_vec()).is_err() {
                    return Ok(());
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
}
