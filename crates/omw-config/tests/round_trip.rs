//! L2 — round-trip tests for the omw-config writer. Pinning fixtures so
//! comment + unknown-table preservation can't regress silently.

use omw_config::{Config, ProviderId};
use std::str::FromStr;

const FIXTURE: &str = r#"# Top-level user comment about why we use openai-prod.
version = 1
default_provider = "openai-prod"

[approval]
mode = "ask_before_write"

[agent]
enabled = true

[providers.openai-prod]
kind = "openai"
key_ref = "keychain:omw/openai-prod"
default_model = "gpt-4o"

# A v0.3 forward-compat block; should survive a write that doesn't touch it.
[routing]
default = "openai-prod"
"#;

#[test]
fn save_atomic_preserves_top_level_comment() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, FIXTURE).unwrap();

    let cfg = Config::load_from(&path).unwrap();
    omw_config::save_atomic(&path, &cfg).unwrap();

    let written = std::fs::read_to_string(&path).unwrap();
    assert!(
        written.contains("# Top-level user comment"),
        "comment lost; got:\n{written}"
    );
}

#[test]
fn save_atomic_preserves_unknown_table() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, FIXTURE).unwrap();

    let cfg = Config::load_from(&path).unwrap();
    omw_config::save_atomic(&path, &cfg).unwrap();

    let written = std::fs::read_to_string(&path).unwrap();
    assert!(
        written.contains("[routing]") && written.contains("default = \"openai-prod\""),
        "[routing] block dropped; got:\n{written}"
    );
}

#[test]
fn save_atomic_round_trips_managed_field_change() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, FIXTURE).unwrap();

    let mut cfg = Config::load_from(&path).unwrap();
    let id = ProviderId::from_str("openai-prod").unwrap();
    if let omw_config::ProviderConfig::OpenAi { default_model, .. } =
        cfg.providers.get_mut(&id).unwrap()
    {
        *default_model = Some("gpt-5".to_string());
    } else {
        panic!("expected openai variant");
    }

    omw_config::save_atomic(&path, &cfg).unwrap();
    let reloaded = Config::load_from(&path).unwrap();
    let provider = reloaded.providers.get(&id).unwrap();
    assert_eq!(provider.default_model(), Some("gpt-5"));
}

#[test]
fn save_atomic_creates_parent_dir() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested/sub/config.toml");
    let cfg = Config::default();
    omw_config::save_atomic(&path, &cfg).unwrap();
    assert!(path.exists(), "parent dir should be created");
}

#[test]
fn save_atomic_writes_to_temp_then_renames() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "version = 1\n").unwrap();

    // After save, no .tmp file should remain.
    let cfg = Config::default();
    omw_config::save_atomic(&path, &cfg).unwrap();
    let entries: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
    let tmp_exists = entries.iter().any(|e| {
        let n = e.as_ref().unwrap().file_name();
        n.to_string_lossy().ends_with(".tmp")
    });
    assert!(!tmp_exists, "leftover .tmp file");
}
