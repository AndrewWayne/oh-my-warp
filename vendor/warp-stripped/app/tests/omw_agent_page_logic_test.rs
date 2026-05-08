//! L1 — pure-function tests for the Agent settings page reducer + converters.
//! Lives as an integration test (not a lib unit test) to sidestep the broken
//! settings_view::mod_test.rs lib target.

#![cfg(feature = "omw_local")]

use omw_config::{ApprovalMode, ProviderConfig, ProviderId};
use std::collections::BTreeMap;
use std::str::FromStr;
use warp::test_exports::{
    apply_action, form_from_config, form_to_config, validate_form,
    DefaultProviderDropdownState, FormError, OmwAgentForm, OmwAgentPageAction,
    OmwAgentPageState, ProviderKindForm, ProviderRow,
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
    s.default_provider_dropdown.highlighted_index = Some(2);
    apply_action(&mut s, OmwAgentPageAction::CloseDefaultProviderDropdown);
    assert!(!s.default_provider_dropdown.is_expanded);
    assert!(s.default_provider_dropdown.highlighted_index.is_none());
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
