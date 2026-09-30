//! Integration tests for `omw notify-setup {install,status,uninstall}`.
//!
//! Uses the subprocess harness (`common::omw_cmd`), which `env_clear`s and sets
//! `HOME` to a temp dir, so `~/.claude` and `~/.codex` resolve inside it. We add
//! `OMW_DATA_DIR` so the materialized bridge scripts land under the temp dir too.
//!
//! Both agents now use Claude Code's JSON hooks format:
//!   Claude: ~/.claude/settings.json  (Stop, Notification)
//!   Codex:  ~/.codex/hooks.json      (Stop, PermissionRequest)
//! both pointing at the unified `agent-notify.sh <label>` dispatcher.

mod common;

use std::path::{Path, PathBuf};

use assert_cmd::Command as AssertCommand;

fn omw(dir: &Path) -> AssertCommand {
    let mut cmd = common::omw_cmd(dir);
    cmd.env("OMW_DATA_DIR", dir.join("data"));
    cmd
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

fn claude(dir: &Path) -> PathBuf {
    dir.join(".claude").join("settings.json")
}

fn codex(dir: &Path) -> PathBuf {
    dir.join(".codex").join("hooks.json")
}

fn scripts_dir(dir: &Path) -> PathBuf {
    dir.join("data").join("notify-hooks")
}

fn json_of(p: &Path) -> serde_json::Value {
    serde_json::from_str(&read(p)).unwrap_or_else(|_| panic!("{} is valid JSON", p.display()))
}

fn first_command(v: &serde_json::Value, event: &str) -> String {
    v["hooks"][event][0]["hooks"][0]["command"]
        .as_str()
        .unwrap_or_else(|| panic!("no command for {event}"))
        .to_string()
}

#[test]
fn install_writes_scripts_and_configs() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();

    omw(dir)
        .args(["notify-setup", "install"])
        .assert()
        .success();

    // Bridge scripts materialized and executable.
    for name in ["agent-notify.sh", "ai-notify.sh"] {
        let p = scripts_dir(dir).join(name);
        assert!(p.is_file(), "missing script {name}");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o755, "{name} should be 0755, was {mode:o}");
        }
    }

    // Claude: Stop + Notification -> agent-notify.sh Claude.
    let cj = json_of(&claude(dir));
    assert_eq!(cj["hooks"]["Stop"].as_array().unwrap().len(), 1);
    assert!(first_command(&cj, "Stop").ends_with("agent-notify.sh Claude"));
    assert!(first_command(&cj, "Notification").ends_with("agent-notify.sh Claude"));

    // Codex: Stop + PermissionRequest -> agent-notify.sh Codex, in ~/.codex/hooks.json.
    let xj = json_of(&codex(dir));
    assert!(first_command(&xj, "Stop").ends_with("agent-notify.sh Codex"));
    assert!(first_command(&xj, "PermissionRequest").ends_with("agent-notify.sh Codex"));
}

#[test]
fn install_is_idempotent() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();

    omw(dir)
        .args(["notify-setup", "install"])
        .assert()
        .success();
    omw(dir)
        .args(["notify-setup", "install"])
        .assert()
        .success();

    let cj = json_of(&claude(dir));
    assert_eq!(cj["hooks"]["Stop"].as_array().unwrap().len(), 1);
    assert_eq!(cj["hooks"]["Notification"].as_array().unwrap().len(), 1);
    let xj = json_of(&codex(dir));
    assert_eq!(xj["hooks"]["Stop"].as_array().unwrap().len(), 1);
    assert_eq!(
        xj["hooks"]["PermissionRequest"].as_array().unwrap().len(),
        1
    );
}

#[test]
fn status_reports_state() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();

    let before = omw(dir).args(["notify-setup", "status"]).assert().success();
    let s = String::from_utf8_lossy(&before.get_output().stdout).to_string();
    assert!(s.contains("not installed"), "{s}");

    omw(dir)
        .args(["notify-setup", "install"])
        .assert()
        .success();

    let after = omw(dir).args(["notify-setup", "status"]).assert().success();
    let s = String::from_utf8_lossy(&after.get_output().stdout).to_string();
    assert!(s.contains("Claude Code: installed"), "{s}");
    assert!(s.contains("Codex: installed"), "{s}");
    assert!(s.contains("present"), "{s}");
}

#[test]
fn install_preserves_user_entries() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    std::fs::create_dir_all(dir.join(".claude")).unwrap();
    std::fs::create_dir_all(dir.join(".codex")).unwrap();
    std::fs::write(
        claude(dir),
        r#"{"model":"opus","hooks":{"Stop":[{"matcher":"","hooks":[{"type":"command","command":"/usr/local/bin/mine.sh"}]}]}}"#,
    )
    .unwrap();
    std::fs::write(
        codex(dir),
        r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"/opt/mine.sh"}]}]}}"#,
    )
    .unwrap();

    omw(dir)
        .args(["notify-setup", "install"])
        .assert()
        .success();

    // Claude: user's own Stop hook kept, ours appended (2 groups); model preserved.
    let cj = json_of(&claude(dir));
    assert_eq!(cj["model"], "opus");
    assert_eq!(cj["hooks"]["Stop"].as_array().unwrap().len(), 2);
    assert_eq!(
        cj["hooks"]["Stop"][0]["hooks"][0]["command"],
        "/usr/local/bin/mine.sh"
    );

    // Codex: user's own PreToolUse hook untouched; our Stop + PermissionRequest added.
    let xj = json_of(&codex(dir));
    assert_eq!(
        xj["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
        "/opt/mine.sh"
    );
    assert!(first_command(&xj, "PermissionRequest").ends_with("agent-notify.sh Codex"));
}

#[test]
fn uninstall_removes_only_omw_entries() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    std::fs::create_dir_all(dir.join(".claude")).unwrap();
    std::fs::write(
        claude(dir),
        r#"{"model":"opus","hooks":{"Stop":[{"matcher":"","hooks":[{"type":"command","command":"/usr/local/bin/mine.sh"}]}]}}"#,
    )
    .unwrap();

    omw(dir)
        .args(["notify-setup", "install"])
        .assert()
        .success();
    omw(dir)
        .args(["notify-setup", "uninstall"])
        .assert()
        .success();

    // User's model + own hook survive; ours are gone; scripts dir removed.
    let cj = json_of(&claude(dir));
    assert_eq!(cj["model"], "opus");
    assert_eq!(cj["hooks"]["Stop"].as_array().unwrap().len(), 1);
    assert_eq!(
        cj["hooks"]["Stop"][0]["hooks"][0]["command"],
        "/usr/local/bin/mine.sh"
    );
    assert!(!scripts_dir(dir).exists());
}
