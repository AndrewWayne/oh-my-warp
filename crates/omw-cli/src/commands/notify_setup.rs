//! `omw notify-setup {install,status,uninstall}` — wire interactive AI-agent
//! completion hooks (Claude Code, Codex) into omw's pane-focus notifications so
//! users don't have to hand-edit `~/.claude/settings.json` and
//! `~/.codex/config.toml`.
//!
//! `install` materializes the bundled bridge scripts into the omw data dir and
//! idempotently merges the hook entries into each agent's config, touching only
//! omw-owned entries. `uninstall` reverses it. `status` reports what's wired.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::Context;
use clap::Args;
use serde_json::{json, Value};
use toml_edit::{value, Array, DocumentMut};

use crate::db;

// Bridge scripts embedded at build time; materialized into the data dir on
// install so the CLI is self-contained (no reliance on repo/app-bundle paths).
const SCRIPTS: &[(&str, &str)] = &[
    (
        "ai-notify.sh",
        include_str!("../../assets/notify-hooks/ai-notify.sh"),
    ),
    (
        "claude-notify-dispatch.sh",
        include_str!("../../assets/notify-hooks/claude-notify-dispatch.sh"),
    ),
    (
        "codex-notify-dispatch.sh",
        include_str!("../../assets/notify-hooks/codex-notify-dispatch.sh"),
    ),
];

const CLAUDE_DISPATCH: &str = "claude-notify-dispatch.sh";
const CODEX_DISPATCH: &str = "codex-notify-dispatch.sh";

#[derive(Args, Debug)]
pub(crate) struct InstallArgs {
    /// Replace an existing non-omw Codex `notify` program instead of leaving
    /// it untouched.
    #[arg(long)]
    force: bool,
}

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

fn codex_config_path() -> anyhow::Result<PathBuf> {
    Ok(home_dir()?.join(".codex").join("config.toml"))
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

// ---------------- Claude Code (JSON, array-valued hooks) ----------------

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

/// Whether any hook group in `arr` has a command belonging to omw (its command
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

fn claude_group(command: String) -> Value {
    json!({ "matcher": "", "hooks": [ { "type": "command", "command": command } ] })
}

/// Returns true if it changed the file.
fn claude_install(dispatch_path: &str) -> anyhow::Result<bool> {
    let path = claude_settings_path()?;
    let mut root = read_json_or_empty(&path)?;
    let obj = root
        .as_object_mut()
        .context("~/.claude/settings.json is not a JSON object")?;
    let hooks = obj.entry("hooks").or_insert_with(|| json!({}));
    let hooks = hooks
        .as_object_mut()
        .context("`hooks` in settings.json is not an object")?;

    let mut changed = false;
    for (event, arg) in [("Stop", "done"), ("Notification", "input")] {
        let entry = hooks.entry(event).or_insert_with(|| json!([]));
        let arr = entry
            .as_array_mut()
            .with_context(|| format!("`hooks.{event}` is not an array"))?;
        if !array_has_omw(arr, dispatch_path) {
            arr.push(claude_group(format!("{dispatch_path} {arg}")));
            changed = true;
        }
    }
    if changed {
        write_atomic(&path, &serde_json::to_string_pretty(&root)?)?;
    }
    Ok(changed)
}

fn claude_installed(dispatch_path: &str) -> anyhow::Result<bool> {
    let path = claude_settings_path()?;
    let root = read_json_or_empty(&path)?;
    Ok(root
        .get("hooks")
        .and_then(|h| h.get("Stop"))
        .and_then(Value::as_array)
        .map(|arr| array_has_omw(arr, dispatch_path))
        .unwrap_or(false))
}

/// Returns true if it changed the file.
fn claude_uninstall(dispatch_path: &str) -> anyhow::Result<bool> {
    let path = claude_settings_path()?;
    if !path.exists() {
        return Ok(false);
    }
    let mut root = read_json_or_empty(&path)?;
    let Some(obj) = root.as_object_mut() else {
        return Ok(false);
    };
    let Some(hooks) = obj.get_mut("hooks").and_then(Value::as_object_mut) else {
        return Ok(false);
    };

    let mut changed = false;
    for event in ["Stop", "Notification"] {
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
    // Prune emptied containers we may have created.
    for event in ["Stop", "Notification"] {
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
        write_atomic(&path, &serde_json::to_string_pretty(&root)?)?;
    }
    Ok(changed)
}

// ---------------- Codex (TOML, single-valued `notify`) ----------------

fn codex_notify_is_omw(doc: &DocumentMut) -> bool {
    doc.get("notify")
        .and_then(|i| i.as_array())
        .and_then(|a| a.get(0))
        .and_then(|v| v.as_str())
        .is_some_and(|s| s.contains(CODEX_DISPATCH))
}

fn codex_has_foreign_notify(doc: &DocumentMut) -> bool {
    doc.get("notify").is_some() && !codex_notify_is_omw(doc)
}

fn read_doc_or_empty(path: &Path) -> anyhow::Result<DocumentMut> {
    match std::fs::read_to_string(path) {
        Ok(s) => s
            .parse::<DocumentMut>()
            .with_context(|| format!("parsing TOML at {}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(DocumentMut::new()),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

/// Outcome of a Codex install attempt, so the caller can message the user.
enum CodexOutcome {
    Installed,
    AlreadyOurs,
    ForeignKept,
}

fn codex_install(dispatch: &str, force: bool) -> anyhow::Result<CodexOutcome> {
    let path = codex_config_path()?;
    let mut doc = read_doc_or_empty(&path)?;

    if codex_notify_is_omw(&doc) {
        return Ok(CodexOutcome::AlreadyOurs);
    }
    if codex_has_foreign_notify(&doc) && !force {
        return Ok(CodexOutcome::ForeignKept);
    }

    let mut arr = Array::new();
    arr.push(dispatch);
    arr.push("turn-ended");
    doc["notify"] = value(arr);
    write_atomic(&path, &doc.to_string())?;
    Ok(CodexOutcome::Installed)
}

fn codex_installed() -> anyhow::Result<bool> {
    let path = codex_config_path()?;
    if !path.exists() {
        return Ok(false);
    }
    Ok(codex_notify_is_omw(&read_doc_or_empty(&path)?))
}

/// Returns true if it changed the file.
fn codex_uninstall() -> anyhow::Result<bool> {
    let path = codex_config_path()?;
    if !path.exists() {
        return Ok(false);
    }
    let mut doc = read_doc_or_empty(&path)?;
    if !codex_notify_is_omw(&doc) {
        return Ok(false);
    }
    doc.remove("notify");
    write_atomic(&path, &doc.to_string())?;
    Ok(true)
}

// ---------------- subcommands ----------------

pub(crate) fn install(
    args: InstallArgs,
    stdout: &mut dyn Write,
    _stderr: &mut dyn Write,
) -> anyhow::Result<()> {
    let dir = scripts_dir()?;
    install_scripts(&dir)?;
    let claude_dispatch = dir.join(CLAUDE_DISPATCH);
    let codex_dispatch = dir.join(CODEX_DISPATCH);
    let claude_dispatch = claude_dispatch
        .to_str()
        .context("script path is not valid UTF-8")?;
    let codex_dispatch = codex_dispatch
        .to_str()
        .context("script path is not valid UTF-8")?;

    writeln!(stdout, "Bridge scripts: {}", dir.display())?;

    let claude_changed = claude_install(claude_dispatch)?;
    writeln!(
        stdout,
        "Claude Code: {}",
        if claude_changed {
            "hooks installed (~/.claude/settings.json)"
        } else {
            "already installed"
        }
    )?;

    match codex_install(codex_dispatch, args.force)? {
        CodexOutcome::Installed => {
            writeln!(stdout, "Codex: notify installed (~/.codex/config.toml)")?
        }
        CodexOutcome::AlreadyOurs => writeln!(stdout, "Codex: already installed")?,
        CodexOutcome::ForeignKept => writeln!(
            stdout,
            "Codex: left your existing `notify` untouched. To run both, set \
             CODEX_PREVIOUS_NOTIFY to your program (the omw dispatcher forwards \
             to it), or re-run with --force to replace it."
        )?,
    }

    writeln!(
        stdout,
        "\nDone. Restart Claude Code / Codex sessions for the hooks to take effect."
    )?;
    Ok(())
}

pub(crate) fn status(stdout: &mut dyn Write, _stderr: &mut dyn Write) -> anyhow::Result<()> {
    let dir = scripts_dir()?;
    let claude_dispatch = dir.join(CLAUDE_DISPATCH);
    let claude_dispatch = claude_dispatch
        .to_str()
        .context("script path is not valid UTF-8")?;

    let scripts = if scripts_present(&dir) {
        "present"
    } else {
        "missing"
    };
    writeln!(stdout, "Bridge scripts ({}): {}", dir.display(), scripts)?;
    writeln!(
        stdout,
        "Claude Code: {}",
        yes_no(claude_installed(claude_dispatch)?)
    )?;
    writeln!(stdout, "Codex: {}", yes_no(codex_installed()?))?;
    Ok(())
}

pub(crate) fn uninstall(stdout: &mut dyn Write, _stderr: &mut dyn Write) -> anyhow::Result<()> {
    let dir = scripts_dir()?;
    let claude_dispatch = dir.join(CLAUDE_DISPATCH);
    let claude_dispatch = claude_dispatch
        .to_str()
        .context("script path is not valid UTF-8")?;

    let claude = claude_uninstall(claude_dispatch)?;
    let codex = codex_uninstall()?;
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
            "notify removed"
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
