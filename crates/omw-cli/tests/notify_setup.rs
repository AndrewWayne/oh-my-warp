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

#[cfg(target_os = "macos")]
#[test]
fn installed_dispatcher_emits_notifications_without_developer_environment() {
    use std::io::Write;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    omw(dir)
        .args(["notify-setup", "install"])
        .assert()
        .success();
    let target = dir.join("captured-tty");
    for (payload, expected) in [
        (
            serde_json::json!({"hook_event_name":"Stop", "last_assistant_message":"QA complete"}),
            Some("QA complete"),
        ),
        (
            serde_json::json!({"hook_event_name":"PermissionRequest", "tool_name":"bash", "tool_input":{"command":"echo approval-qa"}}),
            Some("echo approval-qa"),
        ),
        (
            serde_json::json!({"hook_event_name":"Notification", "message":"QA answer required"}),
            Some("QA answer required"),
        ),
        (
            serde_json::json!({"hook_event_name":"Notification", "message":"Waiting for your input"}),
            None,
        ),
        (serde_json::json!({"hook_event_name":"Unknown"}), None),
    ] {
        std::fs::write(&target, "").unwrap();
        let mut child = Command::new("/bin/bash")
            .arg(scripts_dir(dir).join("agent-notify.sh"))
            .arg("Codex")
            .env_clear()
            .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
            .env("HOME", dir)
            .env("AI_PANE_TTY", &target)
            .env("AI_NOTIFY_THROTTLE_SEC", "0")
            .env("AI_NOTIFY_LOG", "0")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(payload.to_string().as_bytes())
            .unwrap();
        let result = child.wait_with_output().unwrap();
        assert!(result.status.success());
        assert!(result.stdout.is_empty(), "hook must not approve a request");
        assert!(result.stderr.is_empty());
        let deadline = Instant::now()
            + if expected.is_some() {
                Duration::from_secs(5)
            } else {
                Duration::from_millis(500)
            };
        let output = loop {
            std::thread::sleep(Duration::from_millis(50));
            let output = read(&target);
            if !output.is_empty() || Instant::now() >= deadline {
                break output;
            }
        };
        match expected {
            Some(message) => {
                assert!(output.starts_with("\u{1b}]777;notify;"), "event: {payload}");
                assert!(output.ends_with('\u{7}'));
                assert!(output.contains("Codex"));
                assert!(output.contains(message));
            }
            None => assert!(output.is_empty()),
        }
    }
}

#[test]
fn install_recognizes_and_repairs_legacy_and_quoted_hooks_without_duplicates() {
    for quoted in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let data_dir = dir.join("Application Support").join("omw");
        let dispatch = data_dir.join("notify-hooks").join("agent-notify.sh");
        let command = if quoted {
            format!("\"{}\" Codex", dispatch.display())
        } else {
            format!("{} Codex", dispatch.display())
        };
        std::fs::create_dir_all(dir.join(".codex")).unwrap();
        let unrelated = format!("{}.other Codex", dispatch.display());
        let hooks = serde_json::json!({"hooks":{"Stop":[{"hooks":[
            {"type":"command", "command":command},
            {"type":"command", "command":unrelated},
        ]}]}});
        std::fs::write(codex(dir), hooks.to_string()).unwrap();
        let run = |args: &[&str]| {
            omw(dir)
                .env("OMW_DATA_DIR", &data_dir)
                .args(args)
                .assert()
                .success()
        };
        let status = run(&["notify-setup", "status", "--json"]);
        let status: serde_json::Value =
            serde_json::from_slice(&status.get_output().stdout).unwrap();
        assert_eq!(status["codex"], true);
        run(&["notify-setup", "install"]);
        let installed = json_of(&codex(dir));
        assert_eq!(installed["hooks"]["Stop"].as_array().unwrap().len(), 1);
        assert_eq!(
            first_command(&installed, "Stop"),
            format!("{} Codex", shell_words::quote(dispatch.to_str().unwrap()))
        );
        assert_eq!(
            installed["hooks"]["Stop"][0]["hooks"][1]["command"],
            unrelated
        );
        run(&["notify-setup", "uninstall"]);
        let uninstalled = json_of(&codex(dir));
        assert_eq!(uninstalled["hooks"]["Stop"].as_array().unwrap().len(), 1);
        assert_eq!(first_command(&uninstalled, "Stop"), unrelated);
        assert!(uninstalled["hooks"].get("PermissionRequest").is_none());
    }
}

#[test]
fn custom_codex_home_with_spaces_is_used_for_the_complete_lifecycle() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let custom_home = dir.join("custom codex home");
    let data_dir = dir.join("Application Support").join("omw");
    let run = |args: &[&str]| {
        omw(dir)
            .env("CODEX_HOME", &custom_home)
            .env("OMW_DATA_DIR", &data_dir)
            .args(args)
            .assert()
            .success()
    };

    run(&["notify-setup", "install"]);
    assert!(!codex(dir).exists(), "default home must not be modified");
    let hooks_path = custom_home.join("hooks.json");
    let installed = json_of(&hooks_path);
    let dispatch = data_dir.join("notify-hooks").join("agent-notify.sh");
    assert_eq!(
        first_command(&installed, "Stop"),
        format!("{} Codex", shell_words::quote(dispatch.to_str().unwrap()))
    );
    run(&["notify-setup", "install"]);
    assert_eq!(
        json_of(&hooks_path)["hooks"]["Stop"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let status = run(&["notify-setup", "status", "--json"]);
    let status: serde_json::Value = serde_json::from_slice(&status.get_output().stdout).unwrap();
    assert_eq!(status["codex"], true);
    run(&["notify-setup", "uninstall"]);
    assert!(json_of(&hooks_path).get("hooks").is_none());
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
