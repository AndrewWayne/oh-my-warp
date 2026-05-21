//! QA-only harness for native iOS simulator testing against a real PTY.
//!
//! The Node runners start this binary, open the emitted pair URL in Mobile
//! Safari, and drive the real Web Controller against a real `omw-remote`
//! server. Keep this binary narrow: it is a local QA helper, not product
//! surface.

use std::collections::HashMap;
use std::env;
use std::error::Error;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use omw_remote::{
    HostKey, NonceStore, Pairings, RevocationList, ServerConfig, ShellSpec, make_router, open_db,
};
use omw_server::{SessionRegistry, SessionSpec};
use serde_json::json;
use tokio::net::TcpListener;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let root = env_path("OMW_QA_REAL_ROOT").unwrap_or_else(|| {
        env::temp_dir().join(format!("omw-mobile-remote-control-{}", std::process::id()))
    });
    let work_dir = env_path("OMW_QA_REAL_WORKDIR").unwrap_or_else(|| root.join("remote-workdir"));
    let data_dir = root.join("data");
    let byte_dump =
        env_path("OMW_BYTE_DUMP").unwrap_or_else(|| root.join("remote-control-byte-dump.bin"));
    let input_dump =
        env_path("OMW_INPUT_DUMP").unwrap_or_else(|| root.join("remote-control-input-dump.bin"));

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
    let host_id =
        env::var("OMW_QA_REAL_HOST_ID").unwrap_or_else(|_| "omw-qa-remote-control".to_string());
    let start_mode = env::var("OMW_QA_REAL_START_MODE").unwrap_or_else(|_| "claude".to_string());

    let registry = SessionRegistry::new();
    let clean_shell =
        env::var("OMW_QA_REAL_CLEAN_SHELL").unwrap_or_else(|_| "0".to_string()) == "1";
    let (session_id, shell_spec) = match start_mode.as_str() {
        "claude" => {
            let mut child_env = HashMap::new();
            child_env.insert("TERM".to_string(), "xterm-256color".to_string());
            child_env.insert("COLORTERM".to_string(), "truecolor".to_string());
            child_env.insert("TERM_PROGRAM".to_string(), "omw".to_string());

            let id = registry
                .register(SessionSpec {
                    name: "Claude Code QA".to_string(),
                    command: claude_bin.clone(),
                    args: claude_args.clone(),
                    cwd: Some(work_dir.clone()),
                    env: Some(child_env),
                    cols: Some(80),
                    rows: Some(24),
                })
                .await?
                .to_string();

            (
                Some(id),
                ShellSpec {
                    program: claude_bin.clone().into(),
                    args: claude_args.clone().into_iter().map(Into::into).collect(),
                },
            )
        }
        "shell" => {
            std::env::set_current_dir(&work_dir)?;
            (None, qa_shell_spec(clean_shell, &work_dir))
        }
        other => return Err(format!("unsupported OMW_QA_REAL_START_MODE={other:?}").into()),
    };

    let host_key = Arc::new(HostKey::generate());
    let db = open_db(&data_dir.join("omw-remote.sqlite3"))?;
    let pairings = Arc::new(Pairings::new(db));
    let bind: SocketAddr = env::var("OMW_QA_REAL_BIND")
        .unwrap_or_else(|_| "127.0.0.1:0".to_string())
        .parse()?;
    let listener = TcpListener::bind(bind).await?;
    let addr = listener.local_addr()?;
    let public_base_url =
        env::var("OMW_QA_PUBLIC_BASE_URL").unwrap_or_else(|_| format!("http://{addr}"));
    let pair_token = pairings.issue(Duration::from_secs(600))?;
    let pair_url = format!(
        "{}/pair?t={}",
        public_base_url.trim_end_matches('/'),
        pair_token.to_base32()
    );

    let shell_program = shell_spec.program.to_string_lossy().to_string();
    let shell_args: Vec<String> = shell_spec
        .args
        .iter()
        .map(|arg| arg.to_string_lossy().to_string())
        .collect();

    let config = ServerConfig {
        bind: addr,
        host_key,
        pinned_origins: vec![public_base_url.clone()],
        inactivity_timeout: Duration::from_secs(60),
        revocations: RevocationList::new(),
        nonce_store: NonceStore::new(Duration::from_secs(60)),
        pairings: Some(pairings),
        shell: shell_spec,
        pty_registry: registry,
        host_id,
    };

    println!(
        "OMW_QA_REAL_READY {}",
        json!({
            "baseUrl": public_base_url,
            "bind": addr,
            "pairUrl": pair_url,
            "sessionId": session_id,
            "mode": start_mode,
            "shellProgram": shell_program,
            "shellArgs": shell_args,
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

fn qa_shell_spec(clean_shell: bool, work_dir: &std::path::Path) -> ShellSpec {
    #[cfg(target_os = "macos")]
    {
        let inner_shell = if clean_shell {
            "exec /bin/zsh -f -i"
        } else {
            "exec /bin/zsh -i"
        };
        let command = format!(
            "cd {} && {}",
            shell_quote(work_dir.to_string_lossy().as_ref()),
            inner_shell,
        );
        let args = if clean_shell {
            vec!["-f".into(), "-i".into(), "-c".into(), command.into()]
        } else {
            vec!["-i".into(), "-c".into(), command.into()]
        };
        return ShellSpec {
            program: "/bin/zsh".into(),
            args,
        };
    }

    #[cfg(not(target_os = "macos"))]
    {
        if !clean_shell {
            return ShellSpec::default_for_host();
        }

        ShellSpec {
            program: "/bin/sh".into(),
            args: vec![],
        }
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;
    use std::path::Path;

    #[test]
    #[cfg(target_os = "macos")]
    fn default_qa_shell_keeps_default_zsh_startup_and_cd_target() {
        let spec = qa_shell_spec(false, Path::new("/tmp/omw qa"));

        assert_eq!(spec.program, OsStr::new("/bin/zsh"));
        assert_eq!(spec.args[0], OsStr::new("-i"));
        assert_eq!(spec.args[1], OsStr::new("-c"));
        assert!(
            spec.args[2]
                .to_string_lossy()
                .contains("cd '/tmp/omw qa' && exec /bin/zsh -i")
        );
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn clean_qa_shell_is_explicitly_stripped() {
        let spec = qa_shell_spec(true, Path::new("/tmp/omw qa"));

        assert_eq!(spec.program, OsStr::new("/bin/zsh"));
        assert_eq!(spec.args[0], OsStr::new("-f"));
        assert_eq!(spec.args[1], OsStr::new("-i"));
        assert_eq!(spec.args[2], OsStr::new("-c"));
        assert!(
            spec.args[3]
                .to_string_lossy()
                .contains("cd '/tmp/omw qa' && exec /bin/zsh -f -i")
        );
    }
}
