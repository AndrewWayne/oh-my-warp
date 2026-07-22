//! `omw notify-setup {install,status,uninstall}` — wire interactive AI-agent
//! completion/approval hooks (Claude Code, Codex) into omw's pane-focus
//! notifications so users don't hand-edit `~/.claude/settings.json` and
//! `~/.codex/hooks.json`.
//!
//! `install` materializes the bundled bridge scripts into the omw data dir and
//! idempotently merges the hook entries into each agent's hooks file (both use
//! Claude Code's JSON hooks format), touching only omw-owned entries.
//! `uninstall` reverses it. `status` reports what's wired.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::Context;
use clap::Args;
use serde_json::{json, Value};

use crate::db;

// Bridge scripts embedded at build time; materialized into the data dir on
// install so the CLI is self-contained. `agent-notify.sh` is the unified
// dispatcher (keys on the hook's `hook_event_name`); it calls the co-located
// `ai-notify.sh`, which emits an OSC 777 to the origin pane for omw to render.
const SCRIPTS: &[(&str, &str)] = &[
    (
        "agent-notify.sh",
        include_str!("../../assets/notify-hooks/agent-notify.sh"),
    ),
    (
        "ai-notify.sh",
        include_str!("../../assets/notify-hooks/ai-notify.sh"),
    ),
];

const DISPATCH: &str = "agent-notify.sh";

#[derive(Args, Debug)]
pub(crate) struct InstallArgs {}

// ---------------- path helpers ----------------

fn home_dir() -> anyhow::Result<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .context("neither HOME nor USERPROFILE is set")
}

fn claude_settings_path() -> anyhow::Result<PathBuf> {
    Ok(home_dir()?.join(".claude").join("settings.json"))
}

fn codex_hooks_path() -> anyhow::Result<PathBuf> {
    Ok(home_dir()?.join(".codex").join("hooks.json"))
}

fn scripts_dir() -> anyhow::Result<PathBuf> {
    Ok(db::data_dir()?.join("notify-hooks"))
}

fn write_atomic(path: &Path, contents: &str) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
    }
    let mut tmp_name = path
        .file_name()
        .context("path has no file name")?
        .to_os_string();
    tmp_name.push(".tmp");
    let tmp = path.with_file_name(tmp_name);
    std::fs::write(&tmp, contents).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("renaming into {}", path.display()))?;
    Ok(())
}

// ---------------- script materialization ----------------

fn install_scripts(dir: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    for (name, body) in SCRIPTS {
        let dest = dir.join(name);
        std::fs::write(&dest, body).with_context(|| format!("writing {}", dest.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755))
                .with_context(|| format!("chmod 0755 {}", dest.display()))?;
        }
    }
    Ok(())
}

fn scripts_present(dir: &Path) -> bool {
    SCRIPTS.iter().all(|(name, _)| dir.join(name).is_file())
}

// ---------------- JSON hooks (shared by Claude + Codex) ----------------

fn read_json_or_empty(path: &Path) -> anyhow::Result<Value> {
    match std::fs::read_to_string(path) {
        Ok(s) if s.trim().is_empty() => Ok(json!({})),
        Ok(s) => {
            serde_json::from_str(&s).with_context(|| format!("parsing JSON at {}", path.display()))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

/// Whether any hook group in `arr` has a command belonging to omw (command
/// string starts with our dispatch path).
fn array_has_omw(arr: &[Value], dispatch_path: &str) -> bool {
    arr.iter().any(|group| {
        group
            .get("hooks")
            .and_then(Value::as_array)
            .map(|hooks| {
                hooks.iter().any(|h| {
                    h.get("command")
                        .and_then(Value::as_str)
                        .is_some_and(|c| c.starts_with(dispatch_path))
                })
            })
            .unwrap_or(false)
    })
}

fn hook_group(command: String) -> Value {
    json!({ "matcher": "", "hooks": [ { "type": "command", "command": command } ] })
}

/// Idempotently add our hook to each `event` in the JSON hooks file at `path`.
/// `command` is the full command line; `dispatch_path` is the script path used
/// to detect our own entries. Returns true if the file changed.
fn configure_hooks(
    path: &Path,
    events: &[&str],
    command: &str,
    dispatch_path: &str,
) -> anyhow::Result<bool> {
    let mut root = read_json_or_empty(path)?;
    let obj = root
        .as_object_mut()
        .with_context(|| format!("{} is not a JSON object", path.display()))?;
    let hooks = obj.entry("hooks").or_insert_with(|| json!({}));
    let hooks = hooks.as_object_mut().context("`hooks` is not an object")?;

    let mut changed = false;
    for event in events {
        let entry = hooks.entry(*event).or_insert_with(|| json!([]));
        let arr = entry
            .as_array_mut()
            .with_context(|| format!("`hooks.{event}` is not an array"))?;
        if !array_has_omw(arr, dispatch_path) {
            arr.push(hook_group(command.to_string()));
            changed = true;
        }
    }
    if changed {
        write_atomic(path, &serde_json::to_string_pretty(&root)?)?;
    }
    Ok(changed)
}

fn hooks_installed(path: &Path, dispatch_path: &str) -> anyhow::Result<bool> {
    let root = read_json_or_empty(path)?;
    Ok(root
        .get("hooks")
        .and_then(|h| h.get("Stop"))
        .and_then(Value::as_array)
        .map(|arr| array_has_omw(arr, dispatch_path))
        .unwrap_or(false))
}

/// Remove only omw-owned hook groups from every event. Returns true if changed.
fn remove_hooks(path: &Path, dispatch_path: &str) -> anyhow::Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    let mut root = read_json_or_empty(path)?;
    let Some(obj) = root.as_object_mut() else {
        return Ok(false);
    };
    let Some(hooks) = obj.get_mut("hooks").and_then(Value::as_object_mut) else {
        return Ok(false);
    };

    let mut changed = false;
    let events: Vec<String> = hooks.keys().cloned().collect();
    for event in &events {
        if let Some(arr) = hooks.get_mut(event).and_then(Value::as_array_mut) {
            let before = arr.len();
            arr.retain(|group| {
                group
                    .get("hooks")
                    .and_then(Value::as_array)
                    .map(|hs| {
                        !hs.iter().any(|h| {
                            h.get("command")
                                .and_then(Value::as_str)
                                .is_some_and(|c| c.starts_with(dispatch_path))
                        })
                    })
                    .unwrap_or(true)
            });
            if arr.len() != before {
                changed = true;
            }
        }
    }
    // Prune emptied containers.
    for event in &events {
        if hooks
            .get(event)
            .and_then(Value::as_array)
            .is_some_and(|a| a.is_empty())
        {
            hooks.remove(event);
        }
    }
    if hooks.is_empty() {
        obj.remove("hooks");
    }
    if changed {
        write_atomic(path, &serde_json::to_string_pretty(&root)?)?;
    }
    Ok(changed)
}

// ---------------- subcommands ----------------

pub(crate) fn install(
    _args: InstallArgs,
    stdout: &mut dyn Write,
    _stderr: &mut dyn Write,
) -> anyhow::Result<()> {
    let dir = scripts_dir()?;
    install_scripts(&dir)?;
    let dispatch = dir.join(DISPATCH);
    let dispatch = dispatch
        .to_str()
        .context("script path is not valid UTF-8")?;

    writeln!(stdout, "Bridge scripts: {}", dir.display())?;

    let claude_changed = configure_hooks(
        &claude_settings_path()?,
        &["Stop", "Notification"],
        &format!("{dispatch} Claude"),
        dispatch,
    )?;
    writeln!(
        stdout,
        "Claude Code: {}",
        if claude_changed {
            "hooks installed (~/.claude/settings.json)"
        } else {
            "already installed"
        }
    )?;

    let codex_changed = configure_hooks(
        &codex_hooks_path()?,
        &["Stop", "PermissionRequest"],
        &format!("{dispatch} Codex"),
        dispatch,
    )?;
    writeln!(
        stdout,
        "Codex: {}",
        if codex_changed {
            "hooks installed (~/.codex/hooks.json)"
        } else {
            "already installed"
        }
    )?;

    writeln!(
        stdout,
        "\nNext steps:\n  \
         1) Add a shell wrapper so the pane's tty reaches the (detached) hooks —\n     \
         required for pane-focus. Append to ~/.zshrc (or ~/.bashrc), then open a new shell:\n       \
         claude() {{ AI_PANE_TTY=\"$(tty 2>/dev/null)\" command claude \"$@\"; }}\n       \
         codex()  {{ AI_PANE_TTY=\"$(tty 2>/dev/null)\" command codex  \"$@\"; }}\n  \
         2) Restart Claude Code. In Codex, run /hooks and trust the hooks once.\n  \
         Requires an omw build with pane-focus notifications."
    )?;
    Ok(())
}

pub(crate) fn status(stdout: &mut dyn Write, _stderr: &mut dyn Write) -> anyhow::Result<()> {
    let dir = scripts_dir()?;
    let dispatch = dir.join(DISPATCH);
    let dispatch = dispatch
        .to_str()
        .context("script path is not valid UTF-8")?;

    writeln!(
        stdout,
        "Bridge scripts ({}): {}",
        dir.display(),
        if scripts_present(&dir) {
            "present"
        } else {
            "missing"
        }
    )?;
    writeln!(
        stdout,
        "Claude Code: {}",
        yes_no(hooks_installed(&claude_settings_path()?, dispatch)?)
    )?;
    writeln!(
        stdout,
        "Codex: {}",
        yes_no(hooks_installed(&codex_hooks_path()?, dispatch)?)
    )?;
    Ok(())
}

pub(crate) fn uninstall(stdout: &mut dyn Write, _stderr: &mut dyn Write) -> anyhow::Result<()> {
    let dir = scripts_dir()?;
    let dispatch = dir.join(DISPATCH);
    let dispatch = dispatch
        .to_str()
        .context("script path is not valid UTF-8")?;

    let claude = remove_hooks(&claude_settings_path()?, dispatch)?;
    let codex = remove_hooks(&codex_hooks_path()?, dispatch)?;
    if dir.exists() {
        std::fs::remove_dir_all(&dir).with_context(|| format!("removing {}", dir.display()))?;
    }

    writeln!(
        stdout,
        "Claude Code: {}",
        if claude {
            "hooks removed"
        } else {
            "nothing to remove"
        }
    )?;
    writeln!(
        stdout,
        "Codex: {}",
        if codex {
            "hooks removed"
        } else {
            "nothing to remove"
        }
    )?;
    writeln!(stdout, "Bridge scripts: removed")?;
    Ok(())
}

fn yes_no(b: bool) -> &'static str {
    if b {
        "installed"
    } else {
        "not installed"
    }
}
