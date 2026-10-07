// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 Shenhao Miao and the omw contributors
// Copyright (C) 2020-2026 Denver Technologies, Inc.
// This file is part of the AGPL-3.0 derivative work. See LICENSE.

#![cfg(all(feature = "omw_local", target_os = "macos"))]

use std::collections::BTreeMap;
use warp::test_exports::{
    apply_action, form_from_config, provider_test_endpoint, test_provider_connection,
    validate_provider_test_inputs, DefaultProviderDropdownState, OmwAgentPageAction,
    OmwAgentPageState, ProviderKindForm, ProviderRow, ProviderTestStatus,
};

fn row(kind: ProviderKindForm, base_url: &str) -> ProviderRow {
    ProviderRow {
        id: "qa".into(), kind, model: "qa-model".into(), base_url: base_url.into(),
        key_ref_token: String::new(), api_key_input: String::new(),
    }
}

#[test]
fn endpoints_use_provider_defaults_and_preserve_base_paths() {
    for (kind, expected) in [
        (ProviderKindForm::OpenAi, "https://api.openai.com/v1/models"),
        (ProviderKindForm::Anthropic, "https://api.anthropic.com/v1/models"),
        (ProviderKindForm::Ollama, "http://127.0.0.1:11434/v1/models"),
    ] {
        assert_eq!(provider_test_endpoint(&row(kind, "")).unwrap(), expected);
    }
    assert_eq!(provider_test_endpoint(&row(ProviderKindForm::OpenAiCompatible,
        "https://example.com/custom/v1/?api-version=1")).unwrap(),
        "https://example.com/custom/v1/models?api-version=1");
    assert!(provider_test_endpoint(&row(ProviderKindForm::OpenAiCompatible, "")).is_err());
    assert!(provider_test_endpoint(&row(ProviderKindForm::Ollama, "file:///tmp/qa")).is_err());
}

#[test]
fn keyless_ollama_is_valid_but_keyed_providers_require_a_secret() {
    assert!(validate_provider_test_inputs(&row(ProviderKindForm::Ollama, ""), false).is_ok());
    for kind in [ProviderKindForm::OpenAi, ProviderKindForm::Anthropic, ProviderKindForm::OpenAiCompatible] {
        let row = row(kind, "http://127.0.0.1/v1");
        assert_eq!(validate_provider_test_inputs(&row, false).unwrap_err(), "API key is required");
        assert!(validate_provider_test_inputs(&row, true).is_ok());
    }
}

#[tokio::test]
async fn test_requests_use_real_http_and_correct_keyless_or_bearer_headers() {
    for (kind, secret) in [
        (ProviderKindForm::Ollama, None),
        (ProviderKindForm::Ollama, Some("qa-key".to_owned())),
        (ProviderKindForm::OpenAi, Some("qa-key".to_owned())),
        (ProviderKindForm::OpenAiCompatible, Some("qa-key".to_owned())),
    ] {
        let mut server = mockito::Server::new_async().await;
        let authorization = if secret.is_some() { mockito::Matcher::Exact("Bearer qa-key".into()) }
            else { mockito::Matcher::Missing };
        let request = server.mock("GET", "/v1/models")
            .match_header("authorization", authorization)
            .match_header("accept", "application/json")
            .with_status(200).with_body("{\"data\":[]}").create_async().await;
        test_provider_connection(row(kind, &format!("{}/v1", server.url())), secret).await.unwrap();
        request.assert_async().await;
    }
}

#[tokio::test]
async fn failures_omit_response_bodies_secrets_and_configured_urls() {
    let mut server = mockito::Server::new_async().await;
    let request = server.mock("GET", "/v1/models").with_status(401)
        .with_body("qa-secret echoed by provider").create_async().await;
    let error = test_provider_connection(row(ProviderKindForm::OpenAiCompatible,
        &format!("{}/v1", server.url())), Some("qa-secret".into())).await.unwrap_err();
    assert!(error.starts_with("HTTP 401"), "{error}");
    assert!(!error.contains("qa-secret") && !error.contains(&server.url()), "{error}");
    request.assert_async().await;
}

#[tokio::test]
async fn redirects_are_refused_instead_of_forwarding_authentication() {
    let mut server = mockito::Server::new_async().await;
    let redirect = server.mock("GET", "/v1/models").with_status(302)
        .with_header("Location", &format!("{}/stolen", server.url())).create_async().await;
    let destination = server.mock("GET", "/stolen").expect(0).create_async().await;
    let error = test_provider_connection(row(ProviderKindForm::OpenAiCompatible,
        &format!("{}/v1", server.url())), Some("qa-secret".into())).await.unwrap_err();
    assert!(error.starts_with("HTTP 302"), "{error}");
    redirect.assert_async().await;
    destination.assert_async().await;
}

#[tokio::test]
async fn unreachable_endpoint_returns_a_generic_failure() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    assert_eq!(test_provider_connection(row(ProviderKindForm::Ollama,
        &format!("http://{address}/v1")), None).await.unwrap_err(), "connection failed");
}

fn state() -> OmwAgentPageState {
    let config = omw_config::Config::default();
    let mut form = form_from_config(&config);
    form.providers = vec![row(ProviderKindForm::Ollama, ""), row(ProviderKindForm::Ollama, "")];
    OmwAgentPageState {
        form, saved_config: config, pending_secrets: BTreeMap::new(), is_dirty: true,
        last_save_error: None, provider_test_status: BTreeMap::new(), next_provider_test_request_id: 0,
        default_provider_dropdown: DefaultProviderDropdownState::default(), pending_renames: Vec::new(),
    }
}

#[test]
fn edits_apply_discard_and_removal_reject_stale_results() {
    for action in [
        OmwAgentPageAction::SetProviderId(0, "new-id".into()),
        OmwAgentPageAction::SetProviderKind(0, ProviderKindForm::OpenAi),
        OmwAgentPageAction::SetProviderModel(0, "new-model".into()),
        OmwAgentPageAction::SetProviderBaseUrl(0, "http://127.0.0.1/v1".into()),
        OmwAgentPageAction::SetProviderApiKey(0, "qa-secret".into()),
        OmwAgentPageAction::RemoveProvider(0), OmwAgentPageAction::Apply, OmwAgentPageAction::Discard,
    ] {
        let mut state = state();
        state.provider_test_status.insert(0, ProviderTestStatus::Testing(1));
        apply_action(&mut state, action.clone());
        state.finish_provider_test(0, 1, Ok(()));
        assert!(!state.provider_test_status.contains_key(&0), "{action:?}");
    }
}

#[test]
fn newer_request_and_other_provider_results_are_isolated() {
    let mut state = state();
    state.provider_test_status.insert(0, ProviderTestStatus::Testing(2));
    state.provider_test_status.insert(1, ProviderTestStatus::Succeeded);
    state.finish_provider_test(0, 1, Ok(()));
    assert_eq!(state.provider_test_status[&0], ProviderTestStatus::Testing(2));
    state.finish_provider_test(0, 2, Err("HTTP 401".into()));
    assert_eq!(state.provider_test_status[&0], ProviderTestStatus::Failed("HTTP 401".into()));
    assert_eq!(state.provider_test_status[&1], ProviderTestStatus::Succeeded);
    apply_action(&mut state, OmwAgentPageAction::RemoveProvider(0));
    assert!(state.provider_test_status.is_empty());
}
