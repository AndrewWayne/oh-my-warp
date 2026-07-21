//! Integration tests for `omw notify-setup {install,status,uninstall}`.
//!
//! Uses the subprocess harness (`common::omw_cmd`), which `env_clear`s and sets
//! `HOME` to a temp dir, so `~/.claude` and `~/.codex` resolve inside it. We add
//! `OMW_DATA_DIR` so the materialized bridge scripts land under the temp dir too.

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
    dir.join(".codex").join("config.toml")
}

fn scripts_dir(dir: &Path) -> PathBuf {
    dir.join("data").join("notify-hooks")
}

fn claude_json(dir: &Path) -> serde_json::Value {
    serde_json::from_str(&read(&claude(dir))).expect("settings.json is valid JSON")
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
    for name in [
        "ai-notify.sh",
        "claude-notify-dispatch.sh",
        "codex-notify-dispatch.sh",
    ] {
        let p = scripts_dir(dir).join(name);
        assert!(p.is_file(), "missing script {name}");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o755, "{name} should be 0755, was {mode:o}");
        }
    }

    // Claude hooks: one Stop (done) + one Notification (input) pointing at our script.
    let v = claude_json(dir);
    let stop = v["hooks"]["Stop"].as_array().expect("Stop is array");
    assert_eq!(stop.len(), 1);
    let stop_cmd = stop[0]["hooks"][0]["command"].as_str().unwrap();
    assert!(
        stop_cmd.ends_with("claude-notify-dispatch.sh done"),
        "{stop_cmd}"
    );
    let note_cmd = v["hooks"]["Notification"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap();
    assert!(
        note_cmd.ends_with("claude-notify-dispatch.sh input"),
        "{note_cmd}"
    );

    // Codex notify points at our dispatcher.
    let c = read(&codex(dir));
    assert!(c.contains("codex-notify-dispatch.sh"), "{c}");
    assert!(c.contains("turn-ended"), "{c}");
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

    let v = claude_json(dir);
    assert_eq!(v["hooks"]["Stop"].as_array().unwrap().len(), 1);
    assert_eq!(v["hooks"]["Notification"].as_array().unwrap().len(), 1);
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
fn install_preserves_user_entries_and_keeps_foreign_codex_notify() {
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
        "model = \"gpt-5\"\nnotify = [\"/opt/mine.sh\"]\n",
    )
    .unwrap();

    omw(dir)
        .args(["notify-setup", "install"])
        .assert()
        .success();

    // Claude: user's own Stop hook kept, ours appended (2 groups), model preserved.
    let v = claude_json(dir);
    assert_eq!(v["model"], "opus");
    let stop = v["hooks"]["Stop"].as_array().unwrap();
    assert_eq!(stop.len(), 2, "user hook + ours");
    assert_eq!(stop[0]["hooks"][0]["command"], "/usr/local/bin/mine.sh");

    // Codex: foreign notify left untouched (no --force).
    assert!(read(&codex(dir)).contains("/opt/mine.sh"));
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
    let v = claude_json(dir);
    assert_eq!(v["model"], "opus");
    let stop = v["hooks"]["Stop"].as_array().unwrap();
    assert_eq!(stop.len(), 1);
    assert_eq!(stop[0]["hooks"][0]["command"], "/usr/local/bin/mine.sh");
    assert!(!scripts_dir(dir).exists());
}

#[test]
fn codex_force_overwrites_then_uninstall_removes() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    std::fs::create_dir_all(dir.join(".codex")).unwrap();
    std::fs::write(codex(dir), "notify = [\"/opt/mine.sh\"]\n").unwrap();

    omw(dir)
        .args(["notify-setup", "install", "--force"])
        .assert()
        .success();
    assert!(read(&codex(dir)).contains("codex-notify-dispatch.sh"));

    omw(dir)
        .args(["notify-setup", "uninstall"])
        .assert()
        .success();
    assert!(!read(&codex(dir)).contains("notify"));
}
