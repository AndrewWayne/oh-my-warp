//! Integration tests for `omw config {path,show}`.
//!
//! File-boundary note: this file is owned by the Test Overseer under the
//! TRD protocol. The Executor MUST NOT modify it.
//!
//! See `cli_provider.rs` for the full Executor checklist (lib + bin layout
//! and the `pub fn run` signature).

mod common;

use crate::common::{
    assert_no_secret_leak, env_lock, lib_mode_run, omw_cmd, read_config, seed_config,
};

#[test]
fn config_path_honors_omw_config_env() {
    // OMW_CONFIG is the highest-precedence resolver in `omw_config::config_path`.
    let dir = tempfile::tempdir().unwrap();
    let custom = dir.path().join("custom").join("config.toml");

    let mut cmd = omw_cmd(dir.path());
    cmd.env("OMW_CONFIG", &custom);

    let assert = cmd.args(["config", "path"]).assert();
    let output = assert.get_output();
    assert_eq!(
        output.status.code(),
        Some(0),
        "config path should exit 0, stderr={:?}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    // One line, no trailing junk. Allow trailing newline.
    let trimmed = stdout.trim_end();
    assert_eq!(
        trimmed,
        custom.to_string_lossy(),
        "stdout must be exactly the OMW_CONFIG value (one line). got {:?}",
        stdout
    );
}

#[test]
fn config_path_default_xdg_home() {
    let dir = tempfile::tempdir().unwrap();
    let xdg = dir.path().join("xdg-home");

    let mut cmd = omw_cmd(dir.path());
    // Clear OMW_CONFIG (omw_cmd sets it by default) so XDG resolution kicks in.
    cmd.env_remove("OMW_CONFIG");
    cmd.env("XDG_CONFIG_HOME", &xdg);

    let assert = cmd.args(["config", "path"]).assert();
    let output = assert.get_output();
    assert_eq!(
        output.status.code(),
        Some(0),
        "config path with XDG should exit 0, stderr={:?}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let trimmed = stdout.trim_end();
    let expected = xdg.join("omw").join("config.toml");
    assert_eq!(
        trimmed,
        expected.to_string_lossy(),
        "stdout must be <XDG_CONFIG_HOME>/omw/config.toml; got {:?}",
        stdout
    );
}

#[test]
fn config_show_empty_config() {
    let dir = tempfile::tempdir().unwrap();

    let assert = omw_cmd(dir.path()).args(["config", "show"]).assert();
    let output = assert.get_output();
    assert_eq!(
        output.status.code(),
        Some(0),
        "config show on empty must exit 0, stderr={:?}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{}\n{}", stdout, String::from_utf8_lossy(&output.stderr));
    let lower = combined.to_lowercase();
    // Any plausible "empty config" indication is acceptable. Don't pin format.
    assert!(
        lower.contains("no providers")
            || lower.contains("(no providers")
            || lower.contains("empty")
            || lower.contains("version = 1")
            || lower.contains("version=1")
            || lower.contains("version 1"),
        "config show on empty config should indicate emptiness or print \
         version=1; got stdout={:?} stderr={:?}",
        stdout,
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn config_show_with_providers_no_secret_leak() {
    // Sentinel-shaped secret. We seed the in-memory keychain in-process via
    // lib_mode_run for the "stored" provider, then run `config show` in the
    // SAME process so the memory backend sees the seeded key.
    const SENTINEL: &str = "omwsentinel-1234567890abcdef";

    // Use the in-process env so a single process spans seed + show + keychain.
    let _g = env_lock();
    let dir = tempfile::tempdir().unwrap();
    let cfg_path = dir.path().join("config.toml");
    std::env::set_var("OMW_CONFIG", &cfg_path);
    std::env::set_var("OMW_KEYCHAIN_BACKEND", "memory");

    seed_config(
        dir.path(),
        r#"version = 1

[providers.has-key]
kind = "openai"
key_ref = "keychain:omw/has-key"

[providers.no-key]
kind = "openai"
key_ref = "keychain:omw/no-key"
"#,
    );

    // Seed only one of the two providers' keys.
    let kr: omw_config::KeyRef = "keychain:omw/has-key".parse().unwrap();
    omw_keychain::set(&kr, SENTINEL).expect("seed keychain");

    let (code, stdout, stderr) = lib_mode_run(&["config", "show"]);
    assert_eq!(
        code,
        0,
        "config show should succeed; stderr={:?}",
        String::from_utf8_lossy(&stderr)
    );

    let stdout_str = String::from_utf8_lossy(&stdout);
    let stderr_str = String::from_utf8_lossy(&stderr);

    // Both providers must appear.
    assert!(
        stdout_str.contains("has-key"),
        "show must list has-key; got {:?}",
        stdout_str
    );
    assert!(
        stdout_str.contains("no-key"),
        "show must list no-key; got {:?}",
        stdout_str
    );

    // Status indicators: at least one "stored" and at least one "missing".
    let lower = stdout_str.to_lowercase();
    assert!(
        lower.contains("stored"),
        "show must mark seeded provider as 'stored'; got {:?}",
        stdout_str
    );
    assert!(
        lower.contains("missing"),
        "show must mark un-seeded provider as 'missing'; got {:?}",
        stdout_str
    );

    // The hard guarantee: secret VALUES never leak to stdout/stderr.
    assert_no_secret_leak(&stdout_str, SENTINEL, 4);
    assert_no_secret_leak(&stderr_str, SENTINEL, 4);

    // Defense in depth: the on-disk config also must not contain the secret.
    let raw = read_config(dir.path());
    assert_no_secret_leak(&raw, SENTINEL, 4);
}
