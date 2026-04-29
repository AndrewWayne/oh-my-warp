//! Integration tests for `omw provider {list,add,remove}`.
//!
//! File-boundary note: this file is owned by the Test Overseer under the
//! TRD protocol. The Executor MUST NOT modify this file or any other
//! `tests/*` file. They author `crates/omw-cli/src/{lib.rs,main.rs}` and
//! the `[dependencies]` section of `Cargo.toml`.
//!
//! ## Executor checklist (DO BEFORE THESE TESTS COMPILE OR RUN)
//!
//! 1. Make `crates/omw-cli/Cargo.toml` a `[lib]` + `[[bin]]` crate. Lib at
//!    `src/lib.rs`, bin `name = "omw"`, path `src/main.rs`.
//! 2. Expose from `src/lib.rs`:
//!
//!    ```rust,ignore
//!    pub fn run(
//!        args: &[String],
//!        stdout: &mut dyn std::io::Write,
//!        stderr: &mut dyn std::io::Write,
//!    ) -> i32;
//!    ```
//!
//! 3. `src/main.rs` is a thin wrapper: collects `env::args()` (skipping
//!    argv[0]), calls `run()`, exits with the returned code.
//! 4. Add `[dependencies]`: `clap`, `toml_edit`, `anyhow`,
//!    `omw-config = { path = "../omw-config" }`,
//!    `omw-keychain = { path = "../omw-keychain" }`. (Optionally pin
//!    `clap`, `toml_edit`, `anyhow` in root `[workspace.dependencies]`.)
//! 5. Use `toml_edit` (NOT `toml`) for write-paths so comments survive.
//!
//! ## Why both subprocess AND in-process tests
//!
//! The `memory` keychain backend is per-process. A subprocess `omw provider
//! add foo --key sk-x` followed by a separate subprocess `omw provider list`
//! sees a fresh, empty memory store on the second call. So the only way to
//! verify "key was actually written to keychain" or "list shows stored
//! status" is in-process via `omw_cli::run`.
//!
//! Subprocess tests cover: exit codes, stderr/stdout shape, end-to-end
//! argv plumbing.
//! In-process tests cover: keychain side-effects, config TOML side-effects
//! that depend on a previously-written key.

mod common;

use omw_config::{Config, KeyRef, ProviderConfig};

use crate::common::{
    assert_no_secret_leak, env_lock, lib_mode_run, omw_cmd, read_config, seed_config,
};

/// A scratch directory whose path we hand to OMW_CONFIG for in-process tests.
/// We keep the TempDir alive for the duration of the test by holding it in a
/// local — the Drop impl removes the dir.
struct InProcEnv {
    _dir: tempfile::TempDir,
    config_path: std::path::PathBuf,
}

impl InProcEnv {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let cfg = dir.path().join("config.toml");
        // SAFETY: process-env mutation. The caller MUST hold `env_lock()` for
        // the entire duration of the test (set_var here + lib_mode_run later),
        // otherwise a parallel test's `set_var("OMW_CONFIG", ...)` can race in
        // between and cause `lib_mode_run` to read the wrong config path.
        std::env::set_var("OMW_CONFIG", &cfg);
        std::env::set_var("OMW_KEYCHAIN_BACKEND", "memory");
        Self {
            _dir: dir,
            config_path: cfg,
        }
    }
}

// =============================================================================
// `provider list`
// =============================================================================

#[test]
fn provider_list_on_empty_config() {
    let dir = tempfile::tempdir().unwrap();
    let assert = omw_cmd(dir.path()).args(["provider", "list"]).assert();
    let output = assert.get_output();
    assert_eq!(
        output.status.code(),
        Some(0),
        "expected exit 0 on empty config, got {:?}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{}\n{}", stdout, String::from_utf8_lossy(&output.stderr));
    let lower = combined.to_lowercase();
    assert!(
        lower.contains("no providers") || lower.contains("(no providers"),
        "expected 'no providers' message in output, got stdout={:?} stderr={:?}",
        stdout,
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn provider_list_with_one_missing_key() {
    let dir = tempfile::tempdir().unwrap();
    seed_config(
        dir.path(),
        r#"version = 1

[providers.openai-prod]
kind = "openai"
key_ref = "keychain:omw/openai-prod"
default_model = "gpt-4o"
"#,
    );

    let assert = omw_cmd(dir.path()).args(["provider", "list"]).assert();
    let output = assert.get_output();
    assert_eq!(
        output.status.code(),
        Some(0),
        "expected exit 0, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("openai-prod"),
        "stdout should mention the provider id, got {:?}",
        stdout
    );
    assert!(
        stdout.to_lowercase().contains("openai"),
        "stdout should mention the provider kind, got {:?}",
        stdout
    );
    assert!(
        stdout.to_lowercase().contains("missing"),
        "stdout should mark this provider's key as 'missing' (subprocess can't \
         see the parent's memory keychain), got {:?}",
        stdout
    );
}

#[test]
fn provider_list_with_default_marked() {
    let dir = tempfile::tempdir().unwrap();
    seed_config(
        dir.path(),
        r#"version = 1
default_provider = "anthropic-prod"

[providers.anthropic-prod]
kind = "anthropic"
key_ref = "keychain:omw/anthropic-prod"

[providers.openai-prod]
kind = "openai"
key_ref = "keychain:omw/openai-prod"
"#,
    );

    let assert = omw_cmd(dir.path()).args(["provider", "list"]).assert();
    let output = assert.get_output();
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Find the line containing the default provider id and assert it has
    // a default annotation. Substring-anchored to "anthropic-prod" so we
    // don't accidentally match the other line.
    let default_line = stdout
        .lines()
        .find(|l| l.contains("anthropic-prod"))
        .unwrap_or_else(|| panic!("no line for anthropic-prod in:\n{}", stdout));
    let lower = default_line.to_lowercase();
    assert!(
        lower.contains("(default)") || default_line.contains('*'),
        "default-provider line should be marked with '(default)' or '*' \
         prefix, got line={:?}",
        default_line,
    );

    // Sanity: the non-default line must NOT carry the marker. We reject
    // both `(default)` and any `*` — an impl that prefixes ALL providers
    // with `*` would otherwise pass the looser substring check.
    let other_line = stdout
        .lines()
        .find(|l| l.contains("openai-prod"))
        .unwrap_or_else(|| panic!("no line for openai-prod in:\n{}", stdout));
    assert!(
        !other_line.to_lowercase().contains("(default)") && !other_line.contains('*'),
        "non-default line should not be marked as default with '(default)' or '*', got {:?}",
        other_line,
    );
}

#[test]
fn provider_list_with_multiple_providers_stable_order() {
    let dir = tempfile::tempdir().unwrap();
    // Seed out of alphabetical order on purpose. An impl that preserves file
    // insertion order would emit `charlie, alpha, bravo`; an impl that
    // actually sorts (or round-trips through Config's BTreeMap) emits
    // `alpha, bravo, charlie`.
    seed_config(
        dir.path(),
        r#"version = 1

[providers.charlie]
kind = "ollama"

[providers.alpha]
kind = "openai"
key_ref = "keychain:omw/alpha"

[providers.bravo]
kind = "anthropic"
key_ref = "keychain:omw/bravo"
"#,
    );

    let collect_ids = || -> Vec<String> {
        let assert = omw_cmd(dir.path()).args(["provider", "list"]).assert();
        let output = assert.get_output();
        assert_eq!(output.status.code(), Some(0));
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let ids = ["alpha", "bravo", "charlie"];
        let mut found: Vec<(usize, String)> = ids
            .iter()
            .filter_map(|id| stdout.find(id).map(|i| (i, (*id).to_string())))
            .collect();
        assert_eq!(
            found.len(),
            3,
            "all three provider ids must appear in stdout:\n{}",
            stdout
        );
        found.sort_by_key(|(i, _)| *i);
        found.into_iter().map(|(_, id)| id).collect()
    };

    let order_first = collect_ids();
    let order_second = collect_ids();
    assert_eq!(
        order_first, order_second,
        "list output must be stable across two invocations"
    );
    // Config uses BTreeMap (alphabetical) — so we expect alpha, bravo, charlie.
    assert_eq!(
        order_first,
        vec![
            "alpha".to_string(),
            "bravo".to_string(),
            "charlie".to_string()
        ],
        "list must be in alphabetical (BTreeMap) order"
    );
}

// =============================================================================
// `provider add`
// =============================================================================

#[test]
fn provider_add_openai_non_interactive() {
    // In-process so we can verify the keychain side-effect.
    let _g = env_lock();
    let env = InProcEnv::new();

    const SECRET: &str = "sk-test-non-interactive-12345";

    let (code, stdout, stderr) = lib_mode_run(&[
        "provider",
        "add",
        "prod",
        "--kind",
        "openai",
        "--key",
        SECRET,
        "--default-model",
        "gpt-4o",
        "--non-interactive",
    ]);
    assert_eq!(
        code,
        0,
        "add should succeed; stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&stdout),
        String::from_utf8_lossy(&stderr)
    );

    // Config now contains the provider.
    let raw = std::fs::read_to_string(&env.config_path).expect("config.toml exists after add");
    assert!(
        raw.contains("[providers.prod]"),
        "config should have new [providers.prod] table:\n{}",
        raw
    );
    assert!(
        raw.contains("kind = \"openai\""),
        "config should record kind:\n{}",
        raw
    );
    assert!(
        raw.contains("keychain:omw/prod"),
        "config should reference keychain:omw/prod (NOT the secret value):\n{}",
        raw
    );
    // Defense in depth: the secret must NEVER hit disk.
    assert!(
        !raw.contains(SECRET),
        "config file MUST NOT contain plaintext secret"
    );

    // --default-model was passed; verify it round-trips through the canonical
    // loader and lands on the provider.
    let cfg = Config::load_from(&env.config_path).expect("config must load after add");
    let prov = cfg
        .providers
        .iter()
        .find(|(id, _)| id.as_str() == "prod")
        .map(|(_, p)| p)
        .expect("loaded config must contain `prod` provider");
    match prov {
        ProviderConfig::OpenAi { default_model, .. } => {
            assert_eq!(
                default_model.as_deref(),
                Some("gpt-4o"),
                "--default-model must be persisted to provider config"
            );
        }
        other => panic!("expected OpenAi provider, got {:?}", other),
    }

    // Subsequent `provider list` (also in-process so it shares the memory
    // backend) shows the provider with key status "stored".
    let (lcode, lstdout, lstderr) = lib_mode_run(&["provider", "list"]);
    assert_eq!(
        lcode,
        0,
        "list should succeed; stderr={:?}",
        String::from_utf8_lossy(&lstderr)
    );
    let listed = String::from_utf8_lossy(&lstdout);
    let listed_err = String::from_utf8_lossy(&lstderr);
    assert!(
        listed.contains("prod"),
        "list should include the new provider, got {:?}",
        listed
    );
    assert!(
        listed.to_lowercase().contains("stored"),
        "list should show key status 'stored' (in-process shares memory keychain), \
         got {:?}",
        listed
    );
    // Hard guarantee: `provider list` must not echo the plaintext secret on
    // either stream — an impl that prints "stored sk-test-non-interactive-12345"
    // would otherwise pass the substring check above.
    assert_no_secret_leak(&listed, SECRET, 4);
    assert_no_secret_leak(&listed_err, SECRET, 4);

    // Direct keychain check.
    let kr: KeyRef = "keychain:omw/prod".parse().unwrap();
    let secret = omw_keychain::get(&kr).expect("key should be present in memory backend");
    assert_eq!(
        secret.expose(),
        SECRET,
        "stored secret must round-trip exactly"
    );
}

#[test]
fn provider_add_openai_compatible_without_base_url_fails() {
    let dir = tempfile::tempdir().unwrap();
    let assert = omw_cmd(dir.path())
        .args([
            "provider",
            "add",
            "azure",
            "--kind",
            "openai-compatible",
            "--key",
            "sk-x",
            "--non-interactive",
        ])
        .assert();
    let output = assert.get_output();
    assert_ne!(
        output.status.code(),
        Some(0),
        "openai-compatible without --base-url must fail, got success: stdout={:?}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    let lower = stderr.to_lowercase();
    assert!(
        lower.contains("base") && lower.contains("url"),
        "stderr should mention 'base url' / 'base_url', got {:?}",
        stderr
    );
}

#[test]
fn provider_add_openai_compatible_with_base_url_succeeds() {
    // The negative test above ensures `--base-url` is REQUIRED for
    // openai-compatible. This positive twin ensures the impl actually
    // PERSISTS the variant correctly when the flag is present — without it,
    // an impl that rejects every openai-compatible add would pass the suite.
    let dir = tempfile::tempdir().unwrap();
    let assert = omw_cmd(dir.path())
        .args([
            "provider",
            "add",
            "azure",
            "--kind",
            "openai-compatible",
            "--key",
            "sk-x",
            "--base-url",
            "https://my-azure.openai.azure.com",
            "--default-model",
            "gpt-4o",
            "--non-interactive",
        ])
        .assert();
    let output = assert.get_output();
    assert_eq!(
        output.status.code(),
        Some(0),
        "openai-compatible add WITH --base-url must succeed; stderr={:?}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Parse the resulting config via the canonical loader and assert all
    // four user-supplied fields landed on the right variant.
    let cfg = Config::load_from(&dir.path().join("config.toml")).expect("config loads");
    let prov = cfg
        .providers
        .iter()
        .find(|(id, _)| id.as_str() == "azure")
        .map(|(_, p)| p)
        .expect("loaded config must contain `azure` provider");
    match prov {
        ProviderConfig::OpenAiCompatible {
            key_ref,
            base_url,
            default_model,
        } => {
            assert_eq!(
                key_ref.to_string(),
                "keychain:omw/azure",
                "key_ref must be a keychain ref derived from the provider id, \
                 got {:?}",
                key_ref
            );
            assert_eq!(
                base_url.as_str(),
                "https://my-azure.openai.azure.com/",
                "--base-url must be persisted (url crate appends trailing /)"
            );
            assert_eq!(
                default_model.as_deref(),
                Some("gpt-4o"),
                "--default-model must be persisted"
            );
        }
        other => panic!("expected OpenAiCompatible provider, got {:?}", other),
    }
}

#[test]
fn provider_add_ollama_no_key() {
    let dir = tempfile::tempdir().unwrap();
    let assert = omw_cmd(dir.path())
        .args([
            "provider",
            "add",
            "local",
            "--kind",
            "ollama",
            "--base-url",
            "http://127.0.0.1:11434",
            "--non-interactive",
        ])
        .assert();
    let output = assert.get_output();
    assert_eq!(
        output.status.code(),
        Some(0),
        "ollama add without key should succeed; stderr={:?}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify config picked up the kind.
    let raw = read_config(dir.path());
    assert!(
        raw.contains("[providers.local]"),
        "config must contain [providers.local] table:\n{}",
        raw
    );
    assert!(
        raw.contains("kind = \"ollama\""),
        "config must record kind=ollama:\n{}",
        raw
    );
    // No key was set, so config must not declare key_ref.
    assert!(
        !raw.contains("key_ref"),
        "ollama-no-key add must not write key_ref:\n{}",
        raw
    );

    // --base-url was passed; assert it lands on the persisted provider via
    // the canonical loader (not just substring-search the file).
    let cfg = Config::load_from(&dir.path().join("config.toml")).expect("config loads");
    let prov = cfg
        .providers
        .iter()
        .find(|(id, _)| id.as_str() == "local")
        .map(|(_, p)| p)
        .expect("loaded config must contain `local` provider");
    match prov {
        ProviderConfig::Ollama {
            base_url, key_ref, ..
        } => {
            assert_eq!(
                base_url.as_ref().map(|u| u.as_str()),
                Some("http://127.0.0.1:11434/"),
                "--base-url must be persisted to ollama provider config \
                 (note url crate normalizes by appending trailing slash)"
            );
            assert!(
                key_ref.is_none(),
                "ollama-no-key add must not set key_ref, got {:?}",
                key_ref
            );
        }
        other => panic!("expected Ollama provider, got {:?}", other),
    }
}

#[test]
fn provider_add_invalid_id() {
    let dir = tempfile::tempdir().unwrap();
    let assert = omw_cmd(dir.path())
        .args([
            "provider",
            "add",
            "foo.bar",
            "--kind",
            "openai",
            "--key",
            "sk-x",
            "--non-interactive",
        ])
        .assert();
    let output = assert.get_output();
    assert_ne!(
        output.status.code(),
        Some(0),
        "invalid id `foo.bar` must be rejected"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.is_empty(),
        "stderr must explain the rejection, got empty stderr"
    );
}

#[test]
fn provider_add_existing_fails_without_force() {
    // In-process: we want to assert keychain side-effects of --force, which
    // requires the parent process and the SUT to share the memory keychain.
    let _g = env_lock();
    let env = InProcEnv::new();
    seed_config(
        env.config_path.parent().unwrap(),
        r#"version = 1

[providers.dup]
kind = "openai"
key_ref = "keychain:omw/dup"
default_model = "gpt-3.5"
"#,
    );

    // Capture pre-attempt state so we can assert NO side-effects from a
    // failed-without-force run. An impl that writes config/keychain before
    // erroring would slip past assertions made only after the --force retry
    // (since --force is allowed to overwrite).
    let pre_config_bytes = std::fs::read(&env.config_path).expect("seeded config must be readable");
    let pre_kr: KeyRef = "keychain:omw/dup".parse().unwrap();
    match omw_keychain::get(&pre_kr) {
        Err(omw_keychain::KeychainError::NotFound) => {}
        Ok(_) => panic!("precondition: keychain entry for omw/dup must not exist before test"),
        Err(other) => panic!("precondition: unexpected keychain error: {other:?}"),
    }

    // Without --force: must fail and must NOT modify config or keychain.
    let (code, stdout, stderr) = lib_mode_run(&[
        "provider",
        "add",
        "dup",
        "--kind",
        "openai",
        "--key",
        "old-secret",
        "--default-model",
        "gpt-3.5",
        "--non-interactive",
    ]);
    assert_ne!(
        code,
        0,
        "duplicate add without --force must fail; stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&stdout),
        String::from_utf8_lossy(&stderr)
    );

    // Side-effect assertions: failed-without-force MUST be a no-op.
    let post_fail_bytes =
        std::fs::read(&env.config_path).expect("config must remain readable after failed add");
    assert_eq!(
        post_fail_bytes, pre_config_bytes,
        "failed-without-force must not modify config bytes"
    );
    match omw_keychain::get(&pre_kr) {
        Err(omw_keychain::KeychainError::NotFound) => {}
        Ok(_) => {
            panic!("failed-without-force must not write to keychain, but omw/dup is now present")
        }
        Err(other) => panic!("unexpected keychain error after failed add: {other:?}"),
    }

    // With --force: must succeed AND must actually update both the config
    // table and the keychain entry.
    let (code, _stdout, stderr) = lib_mode_run(&[
        "provider",
        "add",
        "dup",
        "--kind",
        "openai",
        "--key",
        "new-secret",
        "--default-model",
        "gpt-4o",
        "--non-interactive",
        "--force",
    ]);
    assert_eq!(
        code,
        0,
        "duplicate add WITH --force must succeed; stderr={:?}",
        String::from_utf8_lossy(&stderr)
    );

    // Config: parse and assert default_model rolled forward from gpt-3.5 to
    // gpt-4o. An impl that prints "ok" but skips the write would be caught
    // here.
    let cfg = Config::load_from(&env.config_path)
        .expect("config must remain loadable after --force overwrite");
    let prov = cfg
        .providers
        .iter()
        .find(|(id, _)| id.as_str() == "dup")
        .map(|(_, p)| p)
        .expect("`dup` provider must still exist after --force");
    match prov {
        ProviderConfig::OpenAi { default_model, .. } => {
            assert_eq!(
                default_model.as_deref(),
                Some("gpt-4o"),
                "--force must overwrite default_model from gpt-3.5 to gpt-4o"
            );
        }
        other => panic!("expected OpenAi provider, got {:?}", other),
    }

    // Keychain: the entry must hold the NEW secret, not the seeded
    // placeholder. We didn't seed the keychain ourselves, but a partial impl
    // that creates the config row but skips the keychain write would yield
    // NotFound here.
    let kr: KeyRef = "keychain:omw/dup".parse().unwrap();
    let stored =
        omw_keychain::get(&kr).expect("--force must (re)write the keychain entry, got NotFound");
    assert_eq!(
        stored.expose(),
        "new-secret",
        "--force must replace the keychain value with the new --key"
    );
}

#[test]
fn provider_add_creates_missing_config_with_version_1() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    assert!(
        !path.exists(),
        "precondition: config file must not exist before add"
    );

    let assert = omw_cmd(dir.path())
        .args([
            "provider",
            "add",
            "first",
            "--kind",
            "openai",
            "--key",
            "sk-x",
            "--non-interactive",
        ])
        .assert();
    let output = assert.get_output();
    assert_eq!(
        output.status.code(),
        Some(0),
        "add should create the config; stderr={:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(path.exists(), "config.toml must exist after add");

    let raw = read_config(dir.path());
    assert!(
        raw.contains("version = 1") || raw.contains("version=1"),
        "new config must declare version = 1:\n{}",
        raw
    );
    assert!(
        raw.contains("[providers.first]"),
        "new config must contain [providers.first]:\n{}",
        raw
    );

    // Final sanity: the file parses with the canonical loader.
    let cfg = Config::load_from(&path).expect("created config must parse");
    assert!(
        cfg.providers.keys().any(|id| id.as_str() == "first"),
        "loaded config must include `first` provider"
    );
}

#[test]
fn provider_add_preserves_comments() {
    // This is the test that gates `toml_edit` vs `toml`. A naïve impl that
    // round-trips through `toml`'s value model strips comments.
    let dir = tempfile::tempdir().unwrap();
    seed_config(
        dir.path(),
        r#"# top-level comment that must survive
version = 1

# block comment above foo
[providers.foo]
kind = "ollama"
"#,
    );

    let assert = omw_cmd(dir.path())
        .args([
            "provider",
            "add",
            "bar",
            "--kind",
            "openai",
            "--key",
            "sk-bar",
            "--non-interactive",
        ])
        .assert();
    let output = assert.get_output();
    assert_eq!(
        output.status.code(),
        Some(0),
        "add should succeed; stderr={:?}",
        String::from_utf8_lossy(&output.stderr)
    );

    let raw = read_config(dir.path());
    assert!(
        raw.contains("# top-level comment that must survive"),
        "top-level comment must survive — Executor must use toml_edit:\n{}",
        raw
    );
    assert!(
        raw.contains("# block comment above foo"),
        "block comment must survive — Executor must use toml_edit:\n{}",
        raw
    );
    assert!(
        raw.contains("[providers.bar]"),
        "new provider must be appended:\n{}",
        raw
    );
    assert!(
        raw.contains("[providers.foo]"),
        "pre-existing provider must remain:\n{}",
        raw
    );
}

#[test]
fn provider_add_make_default() {
    let dir = tempfile::tempdir().unwrap();
    let assert = omw_cmd(dir.path())
        .args([
            "provider",
            "add",
            "primary",
            "--kind",
            "openai",
            "--key",
            "sk-x",
            "--non-interactive",
            "--make-default",
        ])
        .assert();
    let output = assert.get_output();
    assert_eq!(
        output.status.code(),
        Some(0),
        "add --make-default should succeed; stderr={:?}",
        String::from_utf8_lossy(&output.stderr)
    );

    let raw = read_config(dir.path());
    assert!(
        raw.contains("default_provider = \"primary\"")
            || raw.contains("default_provider=\"primary\""),
        "default_provider must be set to the new provider:\n{}",
        raw
    );

    // Cross-check with the canonical loader.
    let cfg = Config::load_from(&dir.path().join("config.toml")).expect("config loads");
    assert_eq!(
        cfg.default_provider.as_ref().map(|p| p.as_str()),
        Some("primary"),
        "loaded Config.default_provider must equal `primary`"
    );
}

// =============================================================================
// `provider remove`
// =============================================================================

#[test]
fn provider_remove_existing_with_yes() {
    let _g = env_lock();
    let env = InProcEnv::new();
    seed_config(
        env.config_path.parent().unwrap(),
        r#"version = 1

[providers.gone]
kind = "openai"
key_ref = "keychain:omw/gone"
"#,
    );
    // Seed the keychain too, so we can verify removal cleans it up.
    let kr: KeyRef = "keychain:omw/gone".parse().unwrap();
    omw_keychain::set(&kr, "sk-soon-to-be-deleted").expect("seed keychain");

    let (code, stdout, stderr) = lib_mode_run(&["provider", "remove", "gone", "--yes"]);
    assert_eq!(
        code,
        0,
        "remove should succeed; stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&stdout),
        String::from_utf8_lossy(&stderr)
    );

    let raw = std::fs::read_to_string(&env.config_path).expect("config still readable");
    assert!(
        !raw.contains("[providers.gone]"),
        "[providers.gone] must be gone:\n{}",
        raw
    );

    // Keychain entry should be cleared too.
    match omw_keychain::get(&kr) {
        Err(omw_keychain::KeychainError::NotFound) => {}
        Ok(_) => panic!("keychain entry must be removed but get() succeeded"),
        Err(other) => panic!("unexpected keychain error: {other:?}"),
    }
}

#[test]
fn provider_remove_nonexistent_fails() {
    let dir = tempfile::tempdir().unwrap();
    seed_config(
        dir.path(),
        r#"version = 1

[providers.exists]
kind = "ollama"
"#,
    );

    let assert = omw_cmd(dir.path())
        .args(["provider", "remove", "ghost", "--yes"])
        .assert();
    let output = assert.get_output();
    assert_ne!(
        output.status.code(),
        Some(0),
        "remove of unknown provider must fail; stdout={:?}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    let lower = stderr.to_lowercase();
    assert!(
        lower.contains("not configured")
            || lower.contains("not found")
            || lower.contains("unknown")
            || lower.contains("no such"),
        "stderr should explain the missing provider, got {:?}",
        stderr
    );
}

#[test]
fn provider_remove_clears_default_provider() {
    let dir = tempfile::tempdir().unwrap();
    seed_config(
        dir.path(),
        r#"version = 1
default_provider = "foo"

[providers.foo]
kind = "openai"
key_ref = "keychain:omw/foo"
"#,
    );

    let assert = omw_cmd(dir.path())
        .args(["provider", "remove", "foo", "--yes"])
        .assert();
    let output = assert.get_output();
    assert_eq!(
        output.status.code(),
        Some(0),
        "remove should succeed; stderr={:?}",
        String::from_utf8_lossy(&output.stderr)
    );

    let raw = read_config(dir.path());
    assert!(
        !raw.contains("[providers.foo]"),
        "[providers.foo] must be gone:\n{}",
        raw
    );

    // Either the line is removed entirely, or the value was cleared. Either
    // way, the loader must not see a stale default_provider that references
    // a missing provider — that would fail validate(). So the cleanest
    // guarantee is: the resulting config LOADS cleanly.
    let cfg = Config::load_from(&dir.path().join("config.toml"))
        .expect("config must remain loadable after remove (default_provider must be cleared)");
    assert!(
        cfg.default_provider.is_none(),
        "default_provider must be cleared once it referenced a removed provider; got {:?}",
        cfg.default_provider
    );

    // Defense in depth: a literal `default_provider = "foo"` line must NOT
    // remain in the file — that would re-fail validation if the user adds a
    // different provider next.
    assert!(
        !raw.contains("default_provider = \"foo\""),
        "stale default_provider line must not remain:\n{}",
        raw
    );
}

// =============================================================================
// `provider add` - secret hygiene
// =============================================================================

#[test]
fn provider_add_does_not_echo_secret_to_stdout_or_stderr() {
    // Pseudo-secret with a sentinel shape — long enough for the partial-prefix
    // sweep at min_window=4 to be meaningful.
    const SENTINEL: &str = "omwsentinel-1234567890abcdef";
    let dir = tempfile::tempdir().unwrap();

    let assert = omw_cmd(dir.path())
        .args([
            "provider",
            "add",
            "hygiene",
            "--kind",
            "openai",
            "--key",
            SENTINEL,
            "--non-interactive",
        ])
        .assert();
    let output = assert.get_output();
    assert_eq!(
        output.status.code(),
        Some(0),
        "add should succeed; stderr={:?}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_no_secret_leak(&stdout, SENTINEL, 4);
    assert_no_secret_leak(&stderr, SENTINEL, 4);

    // Defense in depth: the on-disk config file must also not contain the
    // sentinel — i.e. the impl correctly stored the key in the keychain and
    // wrote a `keychain:` ref into config.
    let raw = read_config(dir.path());
    assert_no_secret_leak(&raw, SENTINEL, 4);
}
