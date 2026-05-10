//! L1 — pure-function tests for the Agent settings page reducer + converters.
//! Lives as an integration test (not a lib unit test) to sidestep the broken
//! settings_view::mod_test.rs lib target.

#![cfg(feature = "omw_local")]

use omw_config::{ApprovalMode, ProviderConfig, ProviderId};
use std::collections::BTreeMap;
use std::str::FromStr;
use warp::test_exports::{
    apply_action, form_from_config, form_from_config_with_order, form_to_config,
    validate_form, DefaultProviderDropdownState, FormError, OmwAgentForm,
    OmwAgentPageAction, OmwAgentPageState, ProviderKindForm, ProviderRow,
};

fn empty_state() -> OmwAgentPageState {
    let cfg = omw_config::Config::default();
    OmwAgentPageState {
        form: form_from_config(&cfg),
        saved_config: cfg,
        pending_secrets: BTreeMap::new(),
        is_dirty: false,
        last_save_error: None,
        default_provider_dropdown: DefaultProviderDropdownState::default(),
        pending_renames: Vec::new(),
    }
}

// ---------------- form_from_config / form_to_config ----------------

#[test]
fn form_from_default_config_has_no_providers_and_default_approval_mode() {
    let cfg = omw_config::Config::default();
    let f = form_from_config(&cfg);
    assert!(f.providers.is_empty());
    assert!(f.default_provider.is_none());
    assert!(f.agent_enabled);
    assert_eq!(f.approval_mode, ApprovalMode::AskBeforeWrite);
}

#[test]
fn form_to_config_round_trip_with_openai_provider() {
    let mut form = OmwAgentForm {
        agent_enabled: true,
        approval_mode: ApprovalMode::Trusted,
        default_provider: Some("openai-prod".to_string()),
        providers: vec![ProviderRow {
            id: "openai-prod".to_string(),
            kind: ProviderKindForm::OpenAi,
            model: "gpt-4o".to_string(),
            base_url: String::new(),
            key_ref_token: "keychain:omw/openai-prod".to_string(),
            api_key_input: String::new(),
        }],
        agents_md_path: String::new(),
    };
    let cfg = form_to_config(&form, &BTreeMap::new()).unwrap();
    let back = form_from_config(&cfg);
    form.providers[0].api_key_input.clear();
    assert_eq!(form, back);
}

#[test]
fn form_to_config_round_trip_with_ollama_no_key() {
    let form = OmwAgentForm {
        agent_enabled: false,
        approval_mode: ApprovalMode::ReadOnly,
        default_provider: Some("ollama-local".to_string()),
        providers: vec![ProviderRow {
            id: "ollama-local".to_string(),
            kind: ProviderKindForm::Ollama,
            model: "llama3.1:8b".to_string(),
            base_url: "http://127.0.0.1:11434".to_string(),
            key_ref_token: String::new(),
            api_key_input: String::new(),
        }],
        agents_md_path: String::new(),
    };
    let cfg = form_to_config(&form, &BTreeMap::new()).unwrap();
    assert!(matches!(
        cfg.providers.get(&ProviderId::from_str("ollama-local").unwrap()).unwrap(),
        ProviderConfig::Ollama { .. }
    ));
}

// ---------------- validate_form ----------------

#[test]
fn validate_rejects_invalid_provider_id() {
    let mut form = OmwAgentForm {
        agent_enabled: true,
        approval_mode: ApprovalMode::AskBeforeWrite,
        default_provider: None,
        providers: vec![ProviderRow {
            id: "bad/id".to_string(),
            kind: ProviderKindForm::OpenAi,
            model: String::new(),
            base_url: String::new(),
            key_ref_token: "keychain:omw/foo".to_string(),
            api_key_input: String::new(),
        }],
        agents_md_path: String::new(),
    };
    let err = validate_form(&form).unwrap_err();
    assert!(matches!(err[0], FormError::InvalidProviderId(_)));
    form.providers[0].id = "ok-id".to_string();
    assert!(validate_form(&form).is_ok());
}

#[test]
fn validate_rejects_duplicate_provider_id() {
    let row = ProviderRow {
        id: "dup".to_string(),
        kind: ProviderKindForm::OpenAi,
        model: String::new(),
        base_url: String::new(),
        key_ref_token: "keychain:omw/dup".to_string(),
        api_key_input: String::new(),
    };
    let form = OmwAgentForm {
        agent_enabled: true,
        approval_mode: ApprovalMode::AskBeforeWrite,
        default_provider: None,
        providers: vec![row.clone(), row],
        agents_md_path: String::new(),
    };
    let err = validate_form(&form).unwrap_err();
    assert!(err.iter().any(|e| matches!(e, FormError::DuplicateProviderId(_))));
}

#[test]
fn validate_requires_base_url_for_openai_compatible() {
    let form = OmwAgentForm {
        agent_enabled: true,
        approval_mode: ApprovalMode::AskBeforeWrite,
        default_provider: Some("azure".into()),
        providers: vec![ProviderRow {
            id: "azure".to_string(),
            kind: ProviderKindForm::OpenAiCompatible,
            model: String::new(),
            base_url: String::new(),
            key_ref_token: "keychain:omw/azure".to_string(),
            api_key_input: String::new(),
        }],
        agents_md_path: String::new(),
    };
    let err = validate_form(&form).unwrap_err();
    assert!(err.iter().any(|e| matches!(e, FormError::BaseUrlRequired(_))));
}

#[test]
fn validate_rejects_default_pointing_at_missing_provider() {
    let form = OmwAgentForm {
        agent_enabled: true,
        approval_mode: ApprovalMode::AskBeforeWrite,
        default_provider: Some("ghost".to_string()),
        providers: vec![],
        agents_md_path: String::new(),
    };
    let err = validate_form(&form).unwrap_err();
    assert!(err.iter().any(|e| matches!(e, FormError::DefaultProviderMissing(_))));
}

#[test]
fn validate_requires_key_for_openai_when_no_existing_keyref_and_no_paste() {
    let form = OmwAgentForm {
        agent_enabled: true,
        approval_mode: ApprovalMode::AskBeforeWrite,
        default_provider: Some("openai-prod".into()),
        providers: vec![ProviderRow {
            id: "openai-prod".to_string(),
            kind: ProviderKindForm::OpenAi,
            model: String::new(),
            base_url: String::new(),
            key_ref_token: String::new(),
            api_key_input: String::new(),
        }],
        agents_md_path: String::new(),
    };
    let err = validate_form(&form).unwrap_err();
    assert!(err.iter().any(|e| matches!(e, FormError::ApiKeyRequired(_))));
}

#[test]
fn validate_skips_completeness_for_non_default_rows() {
    // A non-default row missing api_key + base_url should pass validation
    // — it'll just be skipped at serialization time.
    let form = OmwAgentForm {
        agent_enabled: true,
        approval_mode: ApprovalMode::AskBeforeWrite,
        default_provider: Some("complete".into()),
        providers: vec![
            ProviderRow {
                id: "complete".to_string(),
                kind: ProviderKindForm::OpenAi,
                model: String::new(),
                base_url: String::new(),
                key_ref_token: "keychain:omw/complete".to_string(),
                api_key_input: String::new(),
            },
            ProviderRow {
                id: "stub".to_string(),
                kind: ProviderKindForm::OpenAiCompatible,
                model: String::new(),
                base_url: String::new(),
                key_ref_token: String::new(),
                api_key_input: String::new(),
            },
        ],
        agents_md_path: String::new(),
    };
    assert!(validate_form(&form).is_ok());
}

#[test]
fn validate_still_runs_syntactic_checks_on_non_default_rows() {
    let form = OmwAgentForm {
        agent_enabled: true,
        approval_mode: ApprovalMode::AskBeforeWrite,
        default_provider: None,
        providers: vec![ProviderRow {
            id: "bad/id".to_string(),
            kind: ProviderKindForm::OpenAi,
            model: String::new(),
            base_url: String::new(),
            key_ref_token: String::new(),
            api_key_input: String::new(),
        }],
        agents_md_path: String::new(),
    };
    let err = validate_form(&form).unwrap_err();
    assert!(err.iter().any(|e| matches!(e, FormError::InvalidProviderId(_))));
}

#[test]
fn form_to_config_skips_incomplete_non_default_rows() {
    let form = OmwAgentForm {
        agent_enabled: true,
        approval_mode: ApprovalMode::AskBeforeWrite,
        default_provider: Some("complete".into()),
        providers: vec![
            ProviderRow {
                id: "complete".into(),
                kind: ProviderKindForm::OpenAi,
                model: String::new(),
                base_url: String::new(),
                key_ref_token: "keychain:omw/complete".into(),
                api_key_input: String::new(),
            },
            ProviderRow {
                id: "stub".into(),
                kind: ProviderKindForm::OpenAiCompatible,
                model: String::new(),
                base_url: String::new(),
                key_ref_token: String::new(),
                api_key_input: String::new(),
            },
        ],
        agents_md_path: String::new(),
    };
    let cfg = form_to_config(&form, &BTreeMap::new()).unwrap();
    assert_eq!(cfg.providers.len(), 1, "incomplete stub should not be serialized");
    assert!(cfg
        .providers
        .contains_key(&ProviderId::from_str("complete").unwrap()));
    assert!(!cfg
        .providers
        .contains_key(&ProviderId::from_str("stub").unwrap()));
}

#[test]
fn validate_no_default_means_no_completeness_required() {
    let form = OmwAgentForm {
        agent_enabled: true,
        approval_mode: ApprovalMode::AskBeforeWrite,
        default_provider: None,
        providers: vec![ProviderRow {
            id: "stub".to_string(),
            kind: ProviderKindForm::OpenAi,
            model: String::new(),
            base_url: String::new(),
            key_ref_token: String::new(),
            api_key_input: String::new(),
        }],
        agents_md_path: String::new(),
    };
    assert!(validate_form(&form).is_ok());
}

// ---------------- apply_action ----------------

#[test]
fn apply_toggle_enabled_flips_field_and_marks_dirty() {
    let mut s = empty_state();
    apply_action(&mut s, OmwAgentPageAction::ToggleEnabled);
    assert!(!s.form.agent_enabled);
    assert!(s.is_dirty);
    apply_action(&mut s, OmwAgentPageAction::ToggleEnabled);
    assert!(s.form.agent_enabled);
    assert!(!s.is_dirty);
}

#[test]
fn apply_set_approval_mode_updates_form() {
    let mut s = empty_state();
    apply_action(&mut s, OmwAgentPageAction::SetApprovalMode(ApprovalMode::Trusted));
    assert_eq!(s.form.approval_mode, ApprovalMode::Trusted);
}

#[test]
fn apply_add_provider_appends_default_row() {
    let mut s = empty_state();
    apply_action(&mut s, OmwAgentPageAction::AddProvider);
    assert_eq!(s.form.providers.len(), 1);
    assert_eq!(s.form.providers[0].kind, ProviderKindForm::OpenAi);
}

#[test]
fn apply_remove_provider_clears_default_when_removing_default() {
    let mut s = empty_state();
    apply_action(&mut s, OmwAgentPageAction::AddProvider);
    s.form.default_provider = Some(s.form.providers[0].id.clone());
    apply_action(&mut s, OmwAgentPageAction::RemoveProvider(0));
    assert!(s.form.providers.is_empty());
    assert!(s.form.default_provider.is_none());
}

#[test]
fn apply_set_provider_id_renames_default_and_pending_secret() {
    let mut s = empty_state();
    apply_action(&mut s, OmwAgentPageAction::AddProvider);
    let old_id = s.form.providers[0].id.clone();
    s.form.default_provider = Some(old_id.clone());
    s.pending_secrets.insert(old_id.clone(), "sk-test".into());
    apply_action(
        &mut s,
        OmwAgentPageAction::SetProviderId(0, "renamed".into()),
    );
    assert_eq!(s.form.default_provider.as_deref(), Some("renamed"));
    assert_eq!(s.pending_secrets.get("renamed").map(|s| s.as_str()), Some("sk-test"));
    assert!(!s.pending_secrets.contains_key(&old_id));
}

#[test]
fn apply_set_provider_api_key_records_pending_secret() {
    let mut s = empty_state();
    apply_action(&mut s, OmwAgentPageAction::AddProvider);
    let id = s.form.providers[0].id.clone();
    apply_action(&mut s, OmwAgentPageAction::SetProviderApiKey(0, "sk-foo".into()));
    assert_eq!(s.pending_secrets.get(&id).map(|s| s.as_str()), Some("sk-foo"));
}

#[test]
fn apply_toggle_default_provider_dropdown_flips_expanded() {
    let mut s = empty_state();
    assert!(!s.default_provider_dropdown.is_expanded);
    apply_action(&mut s, OmwAgentPageAction::ToggleDefaultProviderDropdown);
    assert!(s.default_provider_dropdown.is_expanded);
    apply_action(&mut s, OmwAgentPageAction::ToggleDefaultProviderDropdown);
    assert!(!s.default_provider_dropdown.is_expanded);
}

#[test]
fn apply_close_default_provider_dropdown_resets_state() {
    let mut s = empty_state();
    apply_action(&mut s, OmwAgentPageAction::ToggleDefaultProviderDropdown);
    apply_action(&mut s, OmwAgentPageAction::CloseDefaultProviderDropdown);
    assert!(!s.default_provider_dropdown.is_expanded);
}

#[test]
fn apply_set_provider_kind_clears_key_fields_when_key_no_longer_required() {
    let mut s = empty_state();
    apply_action(&mut s, OmwAgentPageAction::AddProvider);
    s.form.providers[0].kind = ProviderKindForm::OpenAi;
    s.form.providers[0].key_ref_token = "keychain:omw/foo".into();
    s.form.providers[0].api_key_input = "sk-typed".into();
    let id = s.form.providers[0].id.clone();
    s.pending_secrets.insert(id.clone(), "sk-typed".into());

    apply_action(
        &mut s,
        OmwAgentPageAction::SetProviderKind(0, ProviderKindForm::Ollama),
    );

    assert_eq!(s.form.providers[0].kind, ProviderKindForm::Ollama);
    assert!(s.form.providers[0].key_ref_token.is_empty());
    assert!(s.form.providers[0].api_key_input.is_empty());
    assert!(!s.pending_secrets.contains_key(&id));
}

#[test]
fn apply_set_provider_kind_preserves_key_fields_across_key_required_kinds() {
    // Switching between two kinds that both require a key (OpenAI ↔
    // Anthropic ↔ OpenAiCompatible) must NOT clear what the user typed.
    let mut s = empty_state();
    apply_action(&mut s, OmwAgentPageAction::AddProvider);
    s.form.providers[0].kind = ProviderKindForm::OpenAi;
    s.form.providers[0].key_ref_token = "keychain:omw/foo".into();

    apply_action(
        &mut s,
        OmwAgentPageAction::SetProviderKind(0, ProviderKindForm::Anthropic),
    );

    assert_eq!(s.form.providers[0].kind, ProviderKindForm::Anthropic);
    assert_eq!(s.form.providers[0].key_ref_token, "keychain:omw/foo");
}

#[test]
fn apply_set_provider_id_pushes_pending_rename_when_canonical() {
    let mut s = empty_state();
    apply_action(&mut s, OmwAgentPageAction::AddProvider);
    s.form.providers[0].id = "old".into();
    s.form.providers[0].key_ref_token = "keychain:omw/old".into();
    apply_action(
        &mut s,
        OmwAgentPageAction::SetProviderId(0, "renamed".into()),
    );
    assert_eq!(
        s.pending_renames,
        vec![("old".to_string(), "renamed".to_string())]
    );
}

#[test]
fn apply_set_provider_id_does_not_push_rename_when_non_canonical() {
    let mut s = empty_state();
    apply_action(&mut s, OmwAgentPageAction::AddProvider);
    s.form.providers[0].id = "old".into();
    s.form.providers[0].key_ref_token = "keychain:omw/shared".into();
    apply_action(
        &mut s,
        OmwAgentPageAction::SetProviderId(0, "renamed".into()),
    );
    assert!(s.pending_renames.is_empty());
}

#[test]
fn apply_remove_provider_drops_matching_pending_rename() {
    let mut s = empty_state();
    apply_action(&mut s, OmwAgentPageAction::AddProvider);
    s.form.providers[0].id = "old".into();
    s.form.providers[0].key_ref_token = "keychain:omw/old".into();
    apply_action(
        &mut s,
        OmwAgentPageAction::SetProviderId(0, "renamed".into()),
    );
    assert_eq!(s.pending_renames.len(), 1);
    apply_action(&mut s, OmwAgentPageAction::RemoveProvider(0));
    assert!(s.pending_renames.is_empty(),
            "removing the renamed row should drop its rename entry");
}

#[test]
fn apply_discard_clears_pending_renames() {
    let mut s = empty_state();
    apply_action(&mut s, OmwAgentPageAction::AddProvider);
    s.form.providers[0].id = "old".into();
    s.form.providers[0].key_ref_token = "keychain:omw/old".into();
    apply_action(
        &mut s,
        OmwAgentPageAction::SetProviderId(0, "renamed".into()),
    );
    apply_action(&mut s, OmwAgentPageAction::Discard);
    assert!(s.pending_renames.is_empty());
}

#[test]
fn apply_set_provider_id_rebuilds_canonical_key_ref_token() {
    let mut s = empty_state();
    apply_action(&mut s, OmwAgentPageAction::AddProvider);
    s.form.providers[0].id = "old".into();
    s.form.providers[0].key_ref_token = "keychain:omw/old".into();
    apply_action(
        &mut s,
        OmwAgentPageAction::SetProviderId(0, "renamed".into()),
    );
    assert_eq!(
        s.form.providers[0].key_ref_token, "keychain:omw/renamed",
        "canonical token should follow the rename"
    );
}

#[test]
fn apply_set_provider_id_leaves_non_canonical_key_ref_token_alone() {
    // If the user manually pasted a non-canonical key_ref (e.g.
    // 'keychain:omw/shared') we must NOT silently rewrite it on rename.
    let mut s = empty_state();
    apply_action(&mut s, OmwAgentPageAction::AddProvider);
    s.form.providers[0].id = "old".into();
    s.form.providers[0].key_ref_token = "keychain:omw/shared".into();
    apply_action(
        &mut s,
        OmwAgentPageAction::SetProviderId(0, "renamed".into()),
    );
    assert_eq!(s.form.providers[0].key_ref_token, "keychain:omw/shared");
}

#[test]
fn apply_set_default_provider_by_id_sets_and_clears_default() {
    let mut s = empty_state();
    apply_action(&mut s, OmwAgentPageAction::AddProvider);
    let id = s.form.providers[0].id.clone();
    apply_action(
        &mut s,
        OmwAgentPageAction::SetDefaultProviderById(Some(id.clone())),
    );
    assert_eq!(s.form.default_provider, Some(id));
    apply_action(&mut s, OmwAgentPageAction::SetDefaultProviderById(None));
    assert!(s.form.default_provider.is_none());
}

#[test]
fn apply_set_default_provider_by_id_ignores_unknown_ids() {
    let mut s = empty_state();
    apply_action(
        &mut s,
        OmwAgentPageAction::SetDefaultProviderById(Some("ghost".into())),
    );
    assert!(s.form.default_provider.is_none());
}

// ---------------- Trivial user-flow tests ----------------
//
// These walk the user's "add provider, fill in fields, set as default,
// click Apply" path through the pure reducer + serialization layers and
// pin both intermediate state (form, default_provider) and final
// disk-shape (typed Config + TOML round-trip).

/// Drive the trivial happy-path flow: add one provider, fill it in, mark
/// it default, run form_to_config. Verifies the row is captured at every
/// layer (form, typed Config, TOML on disk through save/load).
#[test]
fn flow_add_one_provider_set_default_persists_through_toml_roundtrip() {
    let mut s = empty_state();

    // 1. Add a provider — appends a default row at index 0.
    apply_action(&mut s, OmwAgentPageAction::AddProvider);
    assert_eq!(s.form.providers.len(), 1);

    // 2. Fill it in via the same Set* actions the page subscriptions
    //    dispatch from each input's Submit. Order mirrors what the user
    //    might click.
    apply_action(
        &mut s,
        OmwAgentPageAction::SetProviderId(0, "openai-prod".into()),
    );
    apply_action(
        &mut s,
        OmwAgentPageAction::SetProviderKind(0, ProviderKindForm::OpenAi),
    );
    apply_action(
        &mut s,
        OmwAgentPageAction::SetProviderModel(0, "gpt-5.5".into()),
    );
    apply_action(
        &mut s,
        OmwAgentPageAction::SetProviderApiKey(0, "sk-secret".into()),
    );

    // 3. Mark it default through the dropdown's id-keyed action.
    apply_action(
        &mut s,
        OmwAgentPageAction::SetDefaultProviderById(Some("openai-prod".into())),
    );
    assert_eq!(
        s.form.default_provider.as_deref(),
        Some("openai-prod"),
        "form-state default should track the user's selection"
    );

    // 4. Build the typed Config the way `apply()` does. Form's
    //    pending_secrets feeds the persisted_secrets map.
    let mut persisted_secrets = BTreeMap::new();
    for (id, _secret) in &s.pending_secrets {
        let kr = format!("keychain:omw/{id}").parse().unwrap();
        persisted_secrets.insert(id.clone(), kr);
    }
    let cfg = form_to_config(&s.form, &persisted_secrets).expect("form_to_config");
    assert_eq!(
        cfg.default_provider.as_ref().map(|p| p.as_str()),
        Some("openai-prod"),
        "cfg.default_provider must carry the user's selection"
    );
    let id = ProviderId::from_str("openai-prod").unwrap();
    assert!(cfg.providers.contains_key(&id), "cfg should contain the row");
    match cfg.providers.get(&id).unwrap() {
        ProviderConfig::OpenAi {
            default_model,
            ..
        } => {
            assert_eq!(default_model.as_deref(), Some("gpt-5.5"));
        }
        other => panic!("expected OpenAi variant, got {other:?}"),
    }

    // 5. Round-trip through save_atomic + load_from to confirm the
    //    on-disk TOML actually carries default_provider + providers.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    cfg.save_atomic(&path).expect("save_atomic");
    let on_disk = std::fs::read_to_string(&path).unwrap();
    assert!(
        on_disk.contains("default_provider = \"openai-prod\""),
        "TOML missing default_provider: {on_disk}"
    );
    assert!(
        on_disk.contains("[providers.openai-prod]"),
        "TOML missing providers table: {on_disk}"
    );
    let reloaded = omw_config::Config::load_from(&path).expect("load_from");
    assert_eq!(reloaded, cfg, "TOML round-trip must be lossless");
}

/// Two-provider variant: add both, fill both, then flip the default
/// between them via the dropdown's SetDefaultProviderById. Each flip
/// should immediately reflect in form state. Verifies the bug-2 surface:
/// "click an item in the dropdown, the right id wins".
#[test]
fn flow_two_providers_default_can_flip_between_them() {
    let mut s = empty_state();

    apply_action(&mut s, OmwAgentPageAction::AddProvider);
    apply_action(&mut s, OmwAgentPageAction::SetProviderId(0, "deepseek".into()));
    apply_action(
        &mut s,
        OmwAgentPageAction::SetProviderKind(0, ProviderKindForm::OpenAiCompatible),
    );
    apply_action(
        &mut s,
        OmwAgentPageAction::SetProviderBaseUrl(0, "https://api.deepseek.com/v1".into()),
    );
    apply_action(
        &mut s,
        OmwAgentPageAction::SetProviderApiKey(0, "ds-key".into()),
    );

    apply_action(&mut s, OmwAgentPageAction::AddProvider);
    apply_action(&mut s, OmwAgentPageAction::SetProviderId(1, "openai-crs".into()));
    apply_action(
        &mut s,
        OmwAgentPageAction::SetProviderKind(1, ProviderKindForm::OpenAi),
    );
    apply_action(
        &mut s,
        OmwAgentPageAction::SetProviderApiKey(1, "oa-key".into()),
    );

    // Flip default to deepseek.
    apply_action(
        &mut s,
        OmwAgentPageAction::SetDefaultProviderById(Some("deepseek".into())),
    );
    assert_eq!(s.form.default_provider.as_deref(), Some("deepseek"));

    // Flip to openai-crs.
    apply_action(
        &mut s,
        OmwAgentPageAction::SetDefaultProviderById(Some("openai-crs".into())),
    );
    assert_eq!(
        s.form.default_provider.as_deref(),
        Some("openai-crs"),
        "second SetDefaultProviderById should overwrite the first"
    );

    // Clear back to none.
    apply_action(&mut s, OmwAgentPageAction::SetDefaultProviderById(None));
    assert!(s.form.default_provider.is_none());

    // Re-select openai-crs after going through none — bug-2 user
    // sequence ("click none, then click openai-crs" had previously
    // landed on the wrong provider).
    apply_action(
        &mut s,
        OmwAgentPageAction::SetDefaultProviderById(Some("openai-crs".into())),
    );
    assert_eq!(
        s.form.default_provider.as_deref(),
        Some("openai-crs"),
        "selecting after a none-detour must land on the clicked id, \
         not on a sibling"
    );
}

/// User authors providers in a non-alphabetical order, applies, and the
/// reloaded form must keep that order — otherwise per-slot editor inputs
/// + MouseStateHandles point at the wrong row underneath.
#[test]
fn flow_apply_preserves_user_authored_row_order() {
    let mut s = empty_state();

    apply_action(&mut s, OmwAgentPageAction::AddProvider);
    apply_action(&mut s, OmwAgentPageAction::SetProviderId(0, "zebra".into()));
    apply_action(
        &mut s,
        OmwAgentPageAction::SetProviderKind(0, ProviderKindForm::OpenAi),
    );
    apply_action(
        &mut s,
        OmwAgentPageAction::SetProviderApiKey(0, "z-key".into()),
    );

    apply_action(&mut s, OmwAgentPageAction::AddProvider);
    apply_action(&mut s, OmwAgentPageAction::SetProviderId(1, "alpha".into()));
    apply_action(
        &mut s,
        OmwAgentPageAction::SetProviderKind(1, ProviderKindForm::Anthropic),
    );
    apply_action(
        &mut s,
        OmwAgentPageAction::SetProviderApiKey(1, "a-key".into()),
    );

    apply_action(
        &mut s,
        OmwAgentPageAction::SetDefaultProviderById(Some("zebra".into())),
    );

    // Pre-Apply: form is in user-authored order (zebra, alpha).
    let user_order: Vec<String> =
        s.form.providers.iter().map(|r| r.id.clone()).collect();
    assert_eq!(user_order, vec!["zebra".to_string(), "alpha".to_string()]);

    // Convert to typed cfg — providers BTreeMap is alphabetical.
    let mut persisted_secrets = BTreeMap::new();
    for (id, _) in &s.pending_secrets {
        let kr = format!("keychain:omw/{id}").parse().unwrap();
        persisted_secrets.insert(id.clone(), kr);
    }
    let cfg = form_to_config(&s.form, &persisted_secrets).expect("form_to_config");
    let cfg_keys: Vec<&str> = cfg.providers.keys().map(|k| k.as_str()).collect();
    assert_eq!(cfg_keys, vec!["alpha", "zebra"], "BTreeMap is alphabetical");

    // Rebuild form preserving user order — kinds must also stay attached
    // to the right id.
    let rebuilt = form_from_config_with_order(&cfg, &user_order);
    let rebuilt_order: Vec<String> =
        rebuilt.providers.iter().map(|r| r.id.clone()).collect();
    assert_eq!(
        rebuilt_order, user_order,
        "form_from_config_with_order must respect preferred_order"
    );
    assert_eq!(rebuilt.providers[0].kind, ProviderKindForm::OpenAi);
    assert_eq!(rebuilt.providers[1].kind, ProviderKindForm::Anthropic);
    assert_eq!(rebuilt.default_provider.as_deref(), Some("zebra"));
}

/// Same setup as the previous test, but reloads via the order-naive
/// `form_from_config` to confirm the regression we just fixed: without
/// the order hint, rows come back in alphabetical order, which is what
/// drives bug 4 (kinds appear to swap between rows on Apply because the
/// per-slot inputs stay positional).
#[test]
fn flow_apply_without_order_hint_reorders_alphabetically() {
    let mut s = empty_state();
    apply_action(&mut s, OmwAgentPageAction::AddProvider);
    apply_action(&mut s, OmwAgentPageAction::SetProviderId(0, "zebra".into()));
    apply_action(
        &mut s,
        OmwAgentPageAction::SetProviderApiKey(0, "z".into()),
    );
    apply_action(&mut s, OmwAgentPageAction::AddProvider);
    apply_action(&mut s, OmwAgentPageAction::SetProviderId(1, "alpha".into()));
    apply_action(
        &mut s,
        OmwAgentPageAction::SetProviderApiKey(1, "a".into()),
    );

    let mut persisted_secrets = BTreeMap::new();
    for (id, _) in &s.pending_secrets {
        let kr = format!("keychain:omw/{id}").parse().unwrap();
        persisted_secrets.insert(id.clone(), kr);
    }
    let cfg = form_to_config(&s.form, &persisted_secrets).unwrap();
    let naive = form_from_config(&cfg);
    let naive_order: Vec<String> =
        naive.providers.iter().map(|r| r.id.clone()).collect();
    assert_eq!(
        naive_order,
        vec!["alpha".to_string(), "zebra".to_string()],
        "form_from_config without order hint emits alphabetical — \
         documenting the regression the order-aware variant fixes"
    );
}

#[test]
fn apply_discard_resets_form_and_clears_pending() {
    let mut s = empty_state();
    apply_action(&mut s, OmwAgentPageAction::ToggleEnabled);
    apply_action(&mut s, OmwAgentPageAction::AddProvider);
    apply_action(&mut s, OmwAgentPageAction::SetProviderApiKey(0, "sk-foo".into()));
    apply_action(&mut s, OmwAgentPageAction::Discard);
    assert!(s.form.providers.is_empty());
    assert!(s.form.agent_enabled);
    assert!(s.pending_secrets.is_empty());
    assert!(!s.is_dirty);
}

// ---------------- AGENTS.md path field ----------------

#[test]
fn apply_set_agents_md_path_trims_and_stores() {
    let mut s = empty_state();
    apply_action(
        &mut s,
        OmwAgentPageAction::SetAgentsMdPath("  /Users/me/AGENTS.md  ".into()),
    );
    assert_eq!(s.form.agents_md_path, "/Users/me/AGENTS.md");
    assert!(s.is_dirty);
}

#[test]
fn apply_set_agents_md_path_empty_clears_field() {
    let mut s = empty_state();
    s.form.agents_md_path = "/old/path.md".into();
    apply_action(&mut s, OmwAgentPageAction::SetAgentsMdPath(String::new()));
    assert!(s.form.agents_md_path.is_empty());
}

#[test]
fn form_to_config_round_trips_agents_md_path() {
    let form = OmwAgentForm {
        agent_enabled: true,
        approval_mode: ApprovalMode::AskBeforeWrite,
        default_provider: None,
        providers: vec![],
        agents_md_path: "/Users/me/dotfiles/AGENTS.md".into(),
    };
    let cfg = form_to_config(&form, &BTreeMap::new()).unwrap();
    assert_eq!(
        cfg.agent.agents_md_path.as_ref().and_then(|p| p.to_str()),
        Some("/Users/me/dotfiles/AGENTS.md"),
    );
    let back = form_from_config(&cfg);
    assert_eq!(back.agents_md_path, form.agents_md_path);
}

#[test]
fn form_to_config_empty_agents_md_path_serializes_as_none() {
    let form = OmwAgentForm {
        agent_enabled: true,
        approval_mode: ApprovalMode::AskBeforeWrite,
        default_provider: None,
        providers: vec![],
        agents_md_path: "   ".into(), // whitespace-only, treated as unset
    };
    let cfg = form_to_config(&form, &BTreeMap::new()).unwrap();
    assert!(
        cfg.agent.agents_md_path.is_none(),
        "whitespace-only path must serialize as None"
    );
}
