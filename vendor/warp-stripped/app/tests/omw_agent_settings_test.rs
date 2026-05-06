//! L3a — interaction tests for the Agent settings page.
//! Lives as an integration-test binary to sidestep the broken
//! settings_view::mod_test.rs lib target (per spec D12).
//!
//! These tests construct `OmwAgentPageView` via the test-only
//! `new_inner()` constructor (no `warpui::App` context needed) and
//! call `dispatch` / `apply` directly. The settings page's click
//! handlers on Apply/Discard remain unwired (deferred from Task 6);
//! this test target exercises the same logic via the View struct's
//! public API.
//!
//! NOTE: tests use `OMW_CONFIG` env-var which is process-global. Run
//! serially with `cargo test ... -- --test-threads=1`. Without the
//! flag, `clicking_apply_writes_to_temp_config_path` may race with
//! other tests' env-var writes and save to a different path than
//! expected.

#![cfg(feature = "omw_local")]

use omw_config::ApprovalMode;
use warp::test_exports::{
    OmwAgentForm, OmwAgentPageAction, OmwAgentPageView, ProviderKindForm, ProviderRow,
};

#[test]
fn mounting_renders_with_loaded_config() {
    let dir = tempfile::tempdir().unwrap();
    let cfg_path = dir.path().join("does-not-exist.toml");
    std::env::set_var("OMW_CONFIG", &cfg_path);

    let view = OmwAgentPageView::new_inner();

    std::env::remove_var("OMW_CONFIG");

    assert!(view.state.form.providers.is_empty());
    assert!(view.state.form.default_provider.is_none());
    assert!(view.state.form.agent_enabled);
    assert_eq!(view.state.form.approval_mode, ApprovalMode::AskBeforeWrite);
    assert!(!view.state.is_dirty);
    assert!(view.state.last_save_error.is_none());
}

#[test]
fn clicking_add_provider_appends_form_row() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("OMW_CONFIG", dir.path().join("config.toml"));

    let mut view = OmwAgentPageView::new_inner();
    view.dispatch(OmwAgentPageAction::AddProvider);

    std::env::remove_var("OMW_CONFIG");

    assert_eq!(view.state.form.providers.len(), 1);
    assert_eq!(view.state.form.providers[0].kind, ProviderKindForm::OpenAi);
    assert!(view.state.is_dirty);
}

#[test]
fn editing_provider_kind_dropdown_dispatches_action() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("OMW_CONFIG", dir.path().join("config.toml"));

    let mut view = OmwAgentPageView::new_inner();
    view.dispatch(OmwAgentPageAction::AddProvider);
    view.dispatch(OmwAgentPageAction::SetProviderKind(0, ProviderKindForm::Ollama));

    std::env::remove_var("OMW_CONFIG");

    assert_eq!(view.state.form.providers[0].kind, ProviderKindForm::Ollama);
}

#[test]
fn clicking_apply_with_invalid_form_sets_save_error() {
    let dir = tempfile::tempdir().unwrap();
    let cfg_path = dir.path().join("config.toml");
    std::env::set_var("OMW_CONFIG", &cfg_path);

    let mut view = OmwAgentPageView::new_inner();
    view.dispatch(OmwAgentPageAction::AddProvider);
    view.dispatch(OmwAgentPageAction::Apply);

    std::env::remove_var("OMW_CONFIG");

    assert!(
        view.state.last_save_error.is_some(),
        "expected last_save_error to be set; got None"
    );
    assert!(
        !cfg_path.exists(),
        "config file should not exist after a failed Apply"
    );
}

#[test]
fn clicking_apply_writes_to_temp_config_path() {
    let dir = tempfile::tempdir().unwrap();
    let cfg_path = dir.path().join("config.toml");
    std::env::set_var("OMW_CONFIG", &cfg_path);

    let mut view = OmwAgentPageView::new_inner();

    // Hand-construct a valid form by mutating state directly. The
    // existing key_ref_token sidesteps the keychain write — we test
    // the validate → form_to_config → save_atomic pipeline in
    // isolation.
    view.state.form = OmwAgentForm {
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
    view.state.is_dirty = true;

    view.dispatch(OmwAgentPageAction::Apply);

    let err = view.state.last_save_error.clone();
    std::env::remove_var("OMW_CONFIG");

    assert!(err.is_none(), "apply failed: {err:?}");
    assert!(
        cfg_path.exists(),
        "config file should exist after successful apply"
    );

    let reloaded = omw_config::Config::load_from(&cfg_path).expect("must reload");
    assert_eq!(reloaded.approval.mode, ApprovalMode::Trusted);
    assert!(reloaded
        .providers
        .keys()
        .any(|k| k.as_str() == "openai-prod"));
}

#[test]
fn clicking_discard_resets_form_to_saved() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("OMW_CONFIG", dir.path().join("config.toml"));

    let mut view = OmwAgentPageView::new_inner();
    view.dispatch(OmwAgentPageAction::ToggleEnabled);
    assert!(view.state.is_dirty);

    view.dispatch(OmwAgentPageAction::Discard);

    std::env::remove_var("OMW_CONFIG");

    assert!(!view.state.is_dirty);
    assert!(
        view.state.form.agent_enabled,
        "Discard should restore the saved agent_enabled=true default"
    );
}
