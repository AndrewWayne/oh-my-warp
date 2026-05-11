//! QA-only harness for native iOS simulator testing against a real PTY.
//!
//! The Node runner starts this binary, opens the emitted pair URL in Mobile
//! Safari, and drives the real Web Controller against a pre-registered Claude
//! Code session. Keep this binary narrow: it is a local QA helper, not product
//! surface.

use std::collections::HashMap;
use std::env;
use std::error::Error;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use omw_remote::{
    make_router, open_db, HostKey, NonceStore, Pairings, RevocationList, ServerConfig, ShellSpec,
};
use omw_server::{SessionRegistry, SessionSpec};
use serde_json::json;
use tokio::net::TcpListener;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let root = env_path("OMW_QA_REAL_ROOT").unwrap_or_else(|| {
        env::temp_dir().join(format!("omw-mobile-real-claude-{}", std::process::id()))
    });
    let work_dir = env_path("OMW_QA_REAL_WORKDIR").unwrap_or_else(|| root.join("claude-workdir"));
    let data_dir = root.join("data");
    let byte_dump = env_path("OMW_BYTE_DUMP").unwrap_or_else(|| root.join("claude-byte-dump.bin"));
    let input_dump =
        env_path("OMW_INPUT_DUMP").unwrap_or_else(|| root.join("claude-input-dump.bin"));

    std::fs::create_dir_all(&work_dir)?;
    std::fs::create_dir_all(&data_dir)?;
    std::env::set_var("OMW_BYTE_DUMP", &byte_dump);
    std::env::set_var("OMW_INPUT_DUMP", &input_dump);

    let readme = work_dir.join("README.md");
    if !readme.exists() {
        std::fs::write(
            &readme,
            "Temporary omw mobile QA workspace.\n\nThis directory is disposable and used only by the simulator runner.\n",
        )?;
    }

    let claude_bin = env::var("OMW_QA_CLAUDE_BIN").unwrap_or_else(|_| "claude".to_string());
    let claude_args = claude_args()?;
    let host_id = env::var("OMW_QA_REAL_HOST_ID").unwrap_or_else(|_| "omw-qa-claude".to_string());

    let registry = SessionRegistry::new();
    let mut child_env = HashMap::new();
    child_env.insert("TERM".to_string(), "xterm-256color".to_string());
    child_env.insert("COLORTERM".to_string(), "truecolor".to_string());
    child_env.insert("TERM_PROGRAM".to_string(), "omw".to_string());

    let session_id = registry
        .register(SessionSpec {
            name: "Claude Code QA".to_string(),
            command: claude_bin.clone(),
            args: claude_args.clone(),
            cwd: Some(work_dir.clone()),
            env: Some(child_env),
            cols: Some(80),
            rows: Some(24),
        })
        .await?;

    let host_key = Arc::new(HostKey::generate());
    let db = open_db(&data_dir.join("omw-remote.sqlite3"))?;
    let pairings = Arc::new(Pairings::new(db));
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let base_url = format!("http://{addr}");
    let pair_token = pairings.issue(Duration::from_secs(600))?;
    let pair_url = format!("{base_url}/pair?t={}", pair_token.to_base32());

    let config = ServerConfig {
        bind: addr,
        host_key,
        pinned_origins: vec![base_url.clone()],
        inactivity_timeout: Duration::from_secs(60),
        revocations: RevocationList::new(),
        nonce_store: NonceStore::new(Duration::from_secs(60)),
        pairings: Some(pairings),
        shell: ShellSpec {
            program: claude_bin.into(),
            args: claude_args.into_iter().map(Into::into).collect(),
        },
        pty_registry: registry,
        host_id,
    };

    println!(
        "OMW_QA_REAL_READY {}",
        json!({
            "baseUrl": base_url,
            "pairUrl": pair_url,
            "sessionId": session_id,
            "root": root,
            "workDir": work_dir,
            "byteDump": byte_dump,
            "inputDump": input_dump,
        })
    );

    axum::serve(listener, make_router(config).into_make_service()).await?;
    Ok(())
}

fn env_path(name: &str) -> Option<PathBuf> {
    env::var_os(name).map(PathBuf::from)
}

fn claude_args() -> Result<Vec<String>, Box<dyn Error>> {
    if let Ok(raw) = env::var("OMW_QA_CLAUDE_ARGS_JSON") {
        return Ok(serde_json::from_str(&raw)?);
    }
    if let Ok(raw) = env::var("OMW_QA_CLAUDE_ARGS") {
        return Ok(raw
            .split_whitespace()
            .filter(|part| !part.is_empty())
            .map(ToString::to_string)
            .collect());
    }
    Ok(Vec::new())
}
