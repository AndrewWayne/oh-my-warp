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

#[test]
fn save_atomic_scrubs_base_url_when_kind_changes_from_compatible_to_openai() {
    use omw_config::{KeyRef, ProviderConfig};

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        r#"
[providers.x]
kind = "openai-compatible"
key_ref = "keychain:omw/x"
base_url = "https://example.com/v1"
"#,
    )
    .unwrap();

    let mut cfg = Config::load_from(&path).unwrap();
    let id = ProviderId::from_str("x").unwrap();
    cfg.providers.insert(
        id.clone(),
        ProviderConfig::OpenAi {
            key_ref: KeyRef::from_str("keychain:omw/x").unwrap(),
            default_model: None,
        },
    );

    omw_config::save_atomic(&path, &cfg).unwrap();

    // Reload must succeed: if base_url were left in the [providers.x] table,
    // the schema's deny_unknown_fields would reject the kind="openai" variant.
    let reloaded = Config::load_from(&path).expect("reload must succeed after scrub");
    assert!(matches!(
        reloaded.providers.get(&id).unwrap(),
        ProviderConfig::OpenAi { .. }
    ));
}

#[test]
fn save_atomic_preserves_user_added_subfields_in_provider_table() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    // The user has added an unknown subfield `temperature` to a provider
    // table. Our typed schema doesn't include it (it's deny_unknown_fields
    // per-variant — but the user's config might not yet be loaded by the
    // typed path). save_atomic must not erase it.
    //
    // Note: deny_unknown_fields_inside_variant means Config::load_from
    // would reject this fixture. So we cannot use Config::load + mutate.
    // Instead, we exercise the writer's invariant directly: take a valid
    // Config, save it onto a TOML document that has the extra field
    // pre-populated, and verify the field survives.
    std::fs::write(
        &path,
        r#"
[providers.x]
kind = "openai"
key_ref = "keychain:omw/x"
default_model = "gpt-4o"
temperature = 0.7
"#,
    )
    .unwrap();

    // Construct a Config that round-trips this provider untouched, then save.
    // Config::load_from would reject `temperature` per deny_unknown_fields,
    // so we build the Config by hand.
    use omw_config::{KeyRef, ProviderConfig};
    use std::collections::BTreeMap;
    let mut providers = BTreeMap::new();
    providers.insert(
        ProviderId::from_str("x").unwrap(),
        ProviderConfig::OpenAi {
            key_ref: KeyRef::from_str("keychain:omw/x").unwrap(),
            default_model: Some("gpt-4o".to_string()),
        },
    );
    let cfg = Config {
        providers,
        ..Config::default()
    };

    omw_config::save_atomic(&path, &cfg).unwrap();

    let written = std::fs::read_to_string(&path).unwrap();
    assert!(
        written.contains("temperature = 0.7"),
        "user-added temperature subfield was erased; got:\n{written}"
    );
}
