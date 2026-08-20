// SPDX-License-Identifier: AGPL-3.0-only
//
// omw-authored file in the in-tree Warp fork (warpdotdev/warp), part of the
// AGPL-3.0 derivative work. See specs/fork-strategy.md §3.
//
// Copyright (C) 2026 Shenhao Miao and the omw contributors
// Copyright (C) 2020-2026 Denver Technologies, Inc.
//
// This program is free software: you can redistribute it and/or modify it
// under the terms of the GNU Affero General Public License, version 3, as
// published by the Free Software Foundation. See the LICENSE file at the
// repository root for the full text.

//! L3a — interaction tests for the Agent settings page.
//! Lives as an integration-test binary to sidestep the broken
//! settings_view::mod_test.rs lib target (per spec D12).
//!
//! These tests construct `OmwAgentPageView` via the test-only
//! `new_inner()` constructor (no `warpui::App` context needed) and
//! call `dispatch` / `apply` directly. The Apply/Discard click handlers
//! on the rendered page dispatch typed [`OmwAgentPageAction`] frames
//! that `TypedActionView::handle_action` routes back into
//! [`OmwAgentPageView::dispatch`] — same code path the tests below
//! exercise directly, plus a compile-checked assertion that the typed-
//! action wiring is in place.
//!
//! NOTE: tests use `OMW_CONFIG` env-var which is process-global. Run
//! serially with `cargo test ... -- --test-threads=1`. Without the
//! flag, `clicking_apply_writes_to_temp_config_path` may race with
//! other tests' env-var writes and save to a different path than
//! expected.

#![cfg(feature = "omw_local")]

use omw_config::{ApprovalMode, KeyRef};
use omw_keychain::KeychainError;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::time::Duration;
use warp::test_exports::{
    provider_test_endpoint, test_provider_connection_for_test, validate_provider_test_inputs,
    OmwAgentForm, OmwAgentPageAction, OmwAgentPageView, ProviderKindForm, ProviderRow,
};
#[cfg(feature = "test-exports")]
use warp::{appearance::Appearance, settings::AppEditorSettings};
use warpui::TypedActionView;
#[cfg(feature = "test-exports")]
use warpui::{platform::WindowStyle, App};

fn spawn_single_response_server(
    status: &str,
    body: &str,
) -> (String, mpsc::Receiver<String>, std::thread::JoinHandle<()>) {
    spawn_single_response_server_with_headers(status, "", body)
}

fn spawn_single_response_server_with_headers(
    status: &str,
    headers: &str,
    body: &str,
) -> (String, mpsc::Receiver<String>, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let status = status.to_owned();
    let headers = headers.to_owned();
    let body = body.to_owned();
    let (request_tx, request_rx) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut bytes = Vec::new();
        let mut chunk = [0_u8; 2048];
        while !bytes.windows(4).any(|window| window == b"\r\n\r\n") {
            let count = stream.read(&mut chunk).unwrap();
            if count == 0 {
                break;
            }
            bytes.extend_from_slice(&chunk[..count]);
        }
        let request = String::from_utf8(bytes).unwrap();
        let response = format!(
            "HTTP/1.1 {status}\r\n{headers}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(response.as_bytes()).unwrap();
        request_tx.send(request).unwrap();
    });
    (format!("http://{address}/v1"), request_rx, handle)
}

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
    view.dispatch(OmwAgentPageAction::SetProviderKind(
        0,
        ProviderKindForm::Ollama,
    ));

    std::env::remove_var("OMW_CONFIG");

    assert_eq!(view.state.form.providers[0].kind, ProviderKindForm::Ollama);
}

#[cfg(feature = "test-exports")]
#[test]
fn keyed_to_ollama_clears_only_that_rows_live_api_key_buffer() {
    struct EnvRestore(Vec<(&'static str, Option<std::ffi::OsString>)>);
    impl Drop for EnvRestore {
        fn drop(&mut self) {
            for (name, value) in self.0.drain(..) {
                match value {
                    Some(value) => std::env::set_var(name, value),
                    None => std::env::remove_var(name),
                }
            }
        }
    }

    let dir = tempfile::tempdir().unwrap();
    let _env_restore = EnvRestore(
        ["OMW_CONFIG", "OMW_AGENTS_MD_PATH"]
            .into_iter()
            .map(|name| (name, std::env::var_os(name)))
            .collect(),
    );
    std::env::set_var("OMW_CONFIG", dir.path().join("config.toml"));
    std::env::set_var("OMW_AGENTS_MD_PATH", dir.path().join("AGENTS.md"));

    App::test((), |mut app| async move {
        app.add_singleton_model(AppEditorSettings::new_with_omw_test_defaults);
        app.add_singleton_model(|_| Appearance::mock());
        let (_, page) = app.add_window(WindowStyle::NotStealFocus, OmwAgentPageView::new);

        page.update(&mut app, |view, ctx| {
            view.handle_action(&OmwAgentPageAction::AddProvider, ctx);
            view.handle_action(&OmwAgentPageAction::AddProvider, ctx);

            let first_id = view.state.form.providers[0].id.clone();
            let first_editor = view.provider_editors[0]
                .api_key_input
                .as_ref(ctx)
                .editor()
                .clone();
            let second_editor = view.provider_editors[1]
                .api_key_input
                .as_ref(ctx)
                .editor()
                .clone();
            first_editor.update(ctx, |editor, ctx| {
                editor.set_buffer_text("secret-a", ctx);
            });
            second_editor.update(ctx, |editor, ctx| {
                editor.set_buffer_text("secret-b", ctx);
            });

            view.handle_action(
                &OmwAgentPageAction::SetProviderKind(0, ProviderKindForm::Ollama),
                ctx,
            );

            assert_eq!(
                first_editor.read(ctx, |editor, ctx| editor.buffer_text(ctx)),
                "",
                "the switched row retained a stale password buffer"
            );
            assert_eq!(
                second_editor.read(ctx, |editor, ctx| editor.buffer_text(ctx)),
                "secret-b",
                "clearing one row must not discard another row's unsaved secret"
            );
            assert!(view.state.form.providers[0].api_key_input.is_empty());
            assert!(!view.state.pending_secrets.contains_key(&first_id));

            // Model is intentionally empty, so Test fails validation before
            // networking. Its input flush must not resurrect the old secret.
            view.handle_action(&OmwAgentPageAction::TestProvider(0), ctx);
            assert!(view.state.form.providers[0].api_key_input.is_empty());
            assert!(!view.state.pending_secrets.contains_key(&first_id));
            assert_eq!(
                second_editor.read(ctx, |editor, ctx| editor.buffer_text(ctx)),
                "secret-b"
            );
        });
    });
}

#[test]
fn provider_test_endpoints_match_each_supported_provider_surface() {
    let row = |kind, base_url: &str| ProviderRow {
        id: "connection-check".to_owned(),
        kind,
        model: "fixture-model".to_owned(),
        base_url: base_url.to_owned(),
        key_ref_token: String::new(),
        api_key_input: String::new(),
    };

    assert_eq!(
        provider_test_endpoint(&row(ProviderKindForm::OpenAi, "")).unwrap(),
        "https://api.openai.com/v1/models"
    );
    assert_eq!(
        provider_test_endpoint(&row(ProviderKindForm::Anthropic, "")).unwrap(),
        "https://api.anthropic.com/v1/models"
    );
    assert_eq!(
        provider_test_endpoint(&row(
            ProviderKindForm::OpenAiCompatible,
            "http://127.0.0.1:18480/v1/",
        ))
        .unwrap(),
        "http://127.0.0.1:18480/v1/models"
    );
    assert_eq!(
        provider_test_endpoint(&row(ProviderKindForm::Ollama, "")).unwrap(),
        "http://127.0.0.1:11434/v1/models"
    );
}

#[test]
fn provider_test_validation_requires_model_key_and_compatible_base_url() {
    let mut row = ProviderRow {
        id: "connection-check".to_owned(),
        kind: ProviderKindForm::OpenAiCompatible,
        model: String::new(),
        base_url: String::new(),
        key_ref_token: String::new(),
        api_key_input: String::new(),
    };
    assert_eq!(
        validate_provider_test_inputs(&row, false),
        Err("model is required".to_owned())
    );
    row.model = "fixture-model".to_owned();
    assert_eq!(
        validate_provider_test_inputs(&row, false),
        Err("API key is required".to_owned())
    );
    assert_eq!(
        validate_provider_test_inputs(&row, true),
        Err("base URL is required".to_owned())
    );
    row.base_url = "http://127.0.0.1:18480/v1".to_owned();
    assert_eq!(validate_provider_test_inputs(&row, true), Ok(()));

    row.kind = ProviderKindForm::Ollama;
    row.base_url.clear();
    assert_eq!(validate_provider_test_inputs(&row, false), Ok(()));
}

#[tokio::test(flavor = "current_thread")]
async fn provider_connection_test_uses_bearer_auth_and_redacts_error_bodies() {
    let (base_url, request_rx, server) = spawn_single_response_server("200 OK", r#"{"data":[]}"#);
    let mut row = ProviderRow {
        id: "connection-check".to_owned(),
        kind: ProviderKindForm::OpenAiCompatible,
        model: "fixture-model".to_owned(),
        base_url,
        key_ref_token: String::new(),
        api_key_input: String::new(),
    };
    assert_eq!(
        test_provider_connection_for_test(row.clone(), Some("fixture-key".to_owned())).await,
        Ok(())
    );
    let request = request_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    server.join().unwrap();
    let request_lower = request.to_ascii_lowercase();
    assert!(request.starts_with("GET /v1/models HTTP/1.1\r\n"));
    assert!(request_lower.contains("authorization: bearer fixture-key\r\n"));

    let (base_url, request_rx, server) =
        spawn_single_response_server("401 Unauthorized", r#"{"error":"BODY_SECRET"}"#);
    row.base_url = base_url;
    let error = test_provider_connection_for_test(row, Some("fixture-key".to_owned()))
        .await
        .unwrap_err();
    let _ = request_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    server.join().unwrap();
    assert_eq!(error, "HTTP 401 Unauthorized");
    assert!(!error.contains("BODY_SECRET"));
    assert!(!error.contains("fixture-key"));
}

#[tokio::test(flavor = "current_thread")]
async fn provider_connection_test_refuses_redirects() {
    let redirect_target = TcpListener::bind("127.0.0.1:0").unwrap();
    redirect_target.set_nonblocking(true).unwrap();
    let location = format!(
        "Location: http://{}/v1/models\r\n",
        redirect_target.local_addr().unwrap()
    );
    let (base_url, request_rx, server) =
        spawn_single_response_server_with_headers("302 Found", &location, "");
    let row = ProviderRow {
        id: "redirect-check".to_owned(),
        kind: ProviderKindForm::OpenAiCompatible,
        model: "fixture-model".to_owned(),
        base_url,
        key_ref_token: String::new(),
        api_key_input: String::new(),
    };

    let error = test_provider_connection_for_test(row, Some("fixture-key".to_owned()))
        .await
        .unwrap_err();
    let _ = request_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    server.join().unwrap();
    assert_eq!(error, "HTTP 302 Found");
    assert!(matches!(
        redirect_target.accept(),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn provider_connection_test_ollama_without_key_sends_no_authorization() {
    let (base_url, request_rx, server) = spawn_single_response_server("200 OK", r#"{"data":[]}"#);
    let row = ProviderRow {
        id: "ollama-check".to_owned(),
        kind: ProviderKindForm::Ollama,
        model: "fixture-model".to_owned(),
        base_url,
        key_ref_token: String::new(),
        api_key_input: String::new(),
    };

    assert_eq!(test_provider_connection_for_test(row, None).await, Ok(()));
    let request = request_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    server.join().unwrap();
    assert!(request.starts_with("GET /v1/models HTTP/1.1\r\n"));
    assert!(!request.to_ascii_lowercase().contains("authorization:"));
}

#[test]
fn clicking_apply_with_incomplete_non_default_draft_omits_the_draft() {
    let dir = tempfile::tempdir().unwrap();
    let cfg_path = dir.path().join("config.toml");
    std::env::set_var("OMW_CONFIG", &cfg_path);

    let mut view = OmwAgentPageView::new_inner();
    view.dispatch(OmwAgentPageAction::AddProvider);
    view.dispatch(OmwAgentPageAction::Apply);

    std::env::remove_var("OMW_CONFIG");

    assert!(
        view.state.last_save_error.is_none(),
        "Apply should accept a non-default draft"
    );
    assert!(
        cfg_path.exists(),
        "config file should be saved after omitting the incomplete draft"
    );
    let reloaded = omw_config::Config::load_from(&cfg_path).expect("must reload");
    assert!(
        reloaded.providers.is_empty(),
        "incomplete draft must not be serialized"
    );
    assert!(reloaded.default_provider.is_none());
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
        agents_md_path: String::new(),
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
fn apply_keeps_secret_out_of_toml_and_migrates_then_deletes_keychain_entry() {
    struct EnvRestore(Vec<(&'static str, Option<std::ffi::OsString>)>);
    impl Drop for EnvRestore {
        fn drop(&mut self) {
            for (name, value) in self.0.drain(..) {
                match value {
                    Some(value) => std::env::set_var(name, value),
                    None => std::env::remove_var(name),
                }
            }
        }
    }

    struct KeyCleanup(Vec<KeyRef>);
    impl Drop for KeyCleanup {
        fn drop(&mut self) {
            for key_ref in &self.0 {
                let _ = omw_keychain::delete(key_ref);
            }
        }
    }

    let dir = tempfile::tempdir().unwrap();
    let cfg_path = dir.path().join("config.toml");
    let agents_path = dir.path().join("AGENTS.md");
    let old_id = format!("settings-secret-{}", std::process::id());
    let new_id = format!("settings-secret-renamed-{}", std::process::id());
    let old_ref: KeyRef = format!("keychain:omw/{old_id}").parse().unwrap();
    let new_ref: KeyRef = format!("keychain:omw/{new_id}").parse().unwrap();
    let _key_cleanup = KeyCleanup(vec![old_ref.clone(), new_ref.clone()]);
    let _env_restore = EnvRestore(
        ["OMW_CONFIG", "OMW_KEYCHAIN_BACKEND", "OMW_AGENTS_MD_PATH"]
            .into_iter()
            .map(|name| (name, std::env::var_os(name)))
            .collect(),
    );
    std::env::set_var("OMW_CONFIG", &cfg_path);
    std::env::set_var("OMW_KEYCHAIN_BACKEND", "memory");
    std::env::set_var("OMW_AGENTS_MD_PATH", &agents_path);

    let secret = "settings-secret-fixture-value";
    let mut view = OmwAgentPageView::new_inner();
    view.dispatch(OmwAgentPageAction::AddProvider);
    view.dispatch(OmwAgentPageAction::SetProviderId(0, old_id.clone()));
    view.dispatch(OmwAgentPageAction::SetProviderKind(
        0,
        ProviderKindForm::OpenAi,
    ));
    view.dispatch(OmwAgentPageAction::SetProviderModel(
        0,
        "fixture-model".into(),
    ));
    view.dispatch(OmwAgentPageAction::SetProviderApiKey(0, secret.into()));
    view.dispatch(OmwAgentPageAction::SetDefaultProviderById(Some(
        old_id.clone(),
    )));
    view.dispatch(OmwAgentPageAction::Apply);

    assert!(
        view.state.last_save_error.is_none(),
        "initial Apply failed: {:?}",
        view.state.last_save_error
    );
    let raw = std::fs::read_to_string(&cfg_path).unwrap();
    assert!(!raw.contains(secret), "plaintext secret leaked into TOML");
    assert!(
        raw.contains(&old_ref.to_string()),
        "TOML lacks the key reference"
    );
    assert_eq!(omw_keychain::get(&old_ref).unwrap().expose(), secret);

    view.dispatch(OmwAgentPageAction::SetProviderId(0, new_id.clone()));
    view.dispatch(OmwAgentPageAction::Apply);
    assert!(
        view.state.last_save_error.is_none(),
        "rename Apply failed: {:?}",
        view.state.last_save_error
    );
    assert!(matches!(
        omw_keychain::get(&old_ref),
        Err(KeychainError::NotFound)
    ));
    assert_eq!(omw_keychain::get(&new_ref).unwrap().expose(), secret);
    let raw = std::fs::read_to_string(&cfg_path).unwrap();
    assert!(
        !raw.contains(secret),
        "plaintext secret leaked after rename"
    );
    assert!(
        raw.contains(&new_ref.to_string()),
        "renamed key reference was not saved"
    );

    // A keyed → keyless kind change keeps the provider id but removes its
    // key_ref. Apply must delete the now-unreachable keychain entry; cleanup
    // cannot rely only on a provider-id disappearing from config.toml.
    view.dispatch(OmwAgentPageAction::SetProviderKind(
        0,
        ProviderKindForm::Ollama,
    ));
    view.dispatch(OmwAgentPageAction::Apply);
    assert!(
        view.state.last_save_error.is_none(),
        "kind-change Apply failed: {:?}",
        view.state.last_save_error
    );
    assert!(matches!(
        omw_keychain::get(&new_ref),
        Err(KeychainError::NotFound)
    ));
    let reloaded = omw_config::Config::load_from(&cfg_path).unwrap();
    match reloaded
        .providers
        .values()
        .next()
        .expect("Ollama provider must remain after the kind change")
    {
        omw_config::ProviderConfig::Ollama { key_ref, .. } => {
            assert!(key_ref.is_none(), "stale key_ref survived the kind change");
        }
        other => panic!("expected Ollama after kind change, got {other:?}"),
    }

    // Ollama supports an optional explicitly-entered key. Re-add one so the
    // existing remove-provider path still proves that Apply deletes it.
    view.dispatch(OmwAgentPageAction::SetProviderApiKey(0, secret.into()));
    view.dispatch(OmwAgentPageAction::Apply);
    assert_eq!(omw_keychain::get(&new_ref).unwrap().expose(), secret);

    view.dispatch(OmwAgentPageAction::RemoveProvider(0));
    view.dispatch(OmwAgentPageAction::Apply);
    assert!(
        view.state.last_save_error.is_none(),
        "remove Apply failed: {:?}",
        view.state.last_save_error
    );
    assert!(matches!(
        omw_keychain::get(&new_ref),
        Err(KeychainError::NotFound)
    ));
    let raw = std::fs::read_to_string(&cfg_path).unwrap();
    assert!(
        !raw.contains(secret),
        "plaintext secret leaked after removal"
    );
    assert!(
        !raw.contains(&new_ref.to_string()),
        "removed key reference remains in TOML"
    );
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

/// Compile-time check that `OmwAgentPageView` exposes
/// `TypedActionView<Action = OmwAgentPageAction>`. The Apply/Discard
/// `on_click` closures call `ctx.dispatch_typed_action(OmwAgentPageAction::*)`,
/// and warpui only routes the action back into a view that implements
/// this trait with the matching associated type. If a future refactor
/// changes the action type or removes the impl, this test fails to
/// compile.
#[test]
fn typed_action_view_is_wired_for_apply_discard() {
    fn assert_routes<T: TypedActionView<Action = OmwAgentPageAction>>() {}
    assert_routes::<OmwAgentPageView>();
}
