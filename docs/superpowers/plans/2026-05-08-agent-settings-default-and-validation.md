# Agent Settings — Default Dropdown + Default-Only Validation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the read-only "Default provider" label in Settings → Agent with an interactive dropdown selector, gate completeness validation to only the default row (so users can save with incomplete drafts), and fix two related rename/kind-change paper cuts.

**Architecture:** All changes land in one source file (`vendor/warp-stripped/app/src/settings_view/omw_agent_page.rs`) and its sibling integration test (`vendor/warp-stripped/app/tests/omw_agent_page_logic_test.rs`). The dropdown follows the open/close + highlighted-index pattern from `update_environment_form.rs::GithubReposDropdownState` but stripped of repo-domain state (no auth_url, no scroll_state, no async loading — the row list is derived from `form.providers`). New action variant `SetDefaultProviderById(Option<String>)` replaces `SetDefault(usize)` so the dropdown and the per-row [Set Default] button share one canonical mutation. Validation splits into a syntactic pass (id format, dup ids, key_ref shape — all rows) and a completeness pass (kind-required fields — default row only); `form_to_config` filters incomplete non-default rows before serialization, so they vanish on TOML write.

**Tech Stack:** Rust, warpui (the in-tree GUI framework), `cargo test --no-default-features --features omw_local` for the test target, `cargo run -p warp --bin warp-oss --no-default-features --features omw_local` for manual smoke.

---

## File Structure

- **Modify:** `vendor/warp-stripped/app/src/settings_view/omw_agent_page.rs`
  - Add `DefaultProviderDropdownState` struct (open + highlighted index).
  - Add new action variants: `SetDefaultProviderById`, `ToggleDefaultProviderDropdown`, `CloseDefaultProviderDropdown`, `MoveDefaultProviderHighlight`.
  - Remove old `SetDefault(usize)` variant + reducer arm.
  - Refactor `validate_form` (split syntactic / completeness).
  - Refactor `form_to_config` (skip incomplete non-default rows).
  - Patch `SetProviderId` reducer (rebuild canonical key_ref_token).
  - Patch `SetProviderKind` reducer (clear api_key/key_ref on key-requirement boundary cross).
  - Replace label render at line 882-898 with dropdown render.
  - Replace `[Set Default]` button dispatch at line 1094.
- **Modify:** `vendor/warp-stripped/app/tests/omw_agent_page_logic_test.rs`
  - Adjust the existing `validate_requires_*` tests that now only fire on the default row.
  - Add tests for the new behaviors (default-only validation, draft skipping, SetDefaultProviderById, rename canonical rebuild, kind-change clearing).
- **Modify (re-export only):** `vendor/warp-stripped/app/src/lib.rs`'s `test_exports` mod — if any of the new public action variants need to be exposed for tests, mirror the existing pattern.

The plan deliberately keeps `omw_agent_page.rs` as a single file even though it's already 1100+ lines. Splitting it is out of scope per the surgical-changes principle in `CLAUDE.md` §3.

---

## Test invocation reference

All tests in this plan run with:

```
cargo test --manifest-path vendor/warp-stripped/Cargo.toml \
    -p warp --test omw_agent_page_logic_test \
    --no-default-features --features omw_local <test_name>
```

Manual smoke build/run:

```
cargo run --manifest-path vendor/warp-stripped/Cargo.toml \
    -p warp --bin warp-oss --no-default-features --features omw_local
```

---

## Task 1: Refactor `validate_form` to gate completeness on default row only

**Files:**
- Modify: `vendor/warp-stripped/app/src/settings_view/omw_agent_page.rs:128-191`
- Test: `vendor/warp-stripped/app/tests/omw_agent_page_logic_test.rs`

- [ ] **Step 1: Update existing test that asserted strict-on-all-rows behavior**

`tests/omw_agent_page_logic_test.rs` — modify `validate_requires_key_for_openai_when_no_existing_keyref_and_no_paste` (currently at line 156) so the row is the default. Without this the test will start failing once we relax validation:

```rust
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
```

Also update `validate_requires_base_url_for_openai_compatible` (line 124-141) the same way — set `default_provider: Some("azure".into())`.

- [ ] **Step 2: Add new failing tests**

Append to `tests/omw_agent_page_logic_test.rs`:

```rust
#[test]
fn validate_skips_completeness_for_non_default_rows() {
    // A non-default row missing api_key + base_url should pass validation
    // — it'll just be skipped at serialization time. Friend's bug fix.
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
                base_url: String::new(),         // would normally fire BaseUrlRequired
                key_ref_token: String::new(),    // would normally fire ApiKeyRequired
                api_key_input: String::new(),
            },
        ],
    };
    assert!(validate_form(&form).is_ok());
}

#[test]
fn validate_still_runs_syntactic_checks_on_non_default_rows() {
    // Even non-default rows must have a valid id and unique id.
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
fn validate_no_default_means_no_completeness_required() {
    // No default set + only an incomplete row → ok. User just hasn't picked
    // a default yet; that's a runtime concern (start_default errors), not
    // a save-time concern.
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
```

- [ ] **Step 3: Run new tests to verify they fail**

```
cargo test --manifest-path vendor/warp-stripped/Cargo.toml -p warp \
    --test omw_agent_page_logic_test --no-default-features --features omw_local \
    validate_skips_completeness_for_non_default_rows \
    validate_still_runs_syntactic_checks_on_non_default_rows \
    validate_no_default_means_no_completeness_required
```

Expected: `validate_skips_completeness_for_non_default_rows` and `validate_no_default_means_no_completeness_required` FAIL (current strict validator rejects); `validate_still_runs_syntactic_checks_on_non_default_rows` PASSES (id format check is unchanged).

- [ ] **Step 4: Refactor `validate_form` in `omw_agent_page.rs`**

Replace the body at lines 128-191 with a two-pass implementation. The completeness pass only runs when the row's `id == default_provider`:

```rust
pub fn validate_form(form: &OmwAgentForm) -> Result<(), Vec<FormError>> {
    let mut errs = Vec::new();
    let mut seen = std::collections::HashSet::new();

    let default_id: Option<&str> = form.default_provider.as_deref();

    for row in &form.providers {
        // Syntactic pass — applies to every row.
        if ProviderId::from_str(&row.id).is_err() {
            errs.push(FormError::InvalidProviderId(row.id.clone()));
            continue;
        }
        if !seen.insert(row.id.clone()) {
            errs.push(FormError::DuplicateProviderId(row.id.clone()));
        }
        if !row.key_ref_token.is_empty() && KeyRef::from_str(&row.key_ref_token).is_err() {
            errs.push(FormError::KeyRefInvalid(row.id.clone()));
        }

        // Completeness pass — only runs for the row marked as default.
        // Other rows are drafts; they'll be skipped at serialization.
        if default_id != Some(row.id.as_str()) {
            continue;
        }
        match row.kind {
            ProviderKindForm::OpenAiCompatible => {
                if row.base_url.is_empty() {
                    errs.push(FormError::BaseUrlRequired(row.id.clone()));
                } else if BaseUrl::from_str(&row.base_url).is_err() {
                    errs.push(FormError::BaseUrlInvalid(row.id.clone()));
                }
                if row.key_ref_token.is_empty() && row.api_key_input.is_empty() {
                    errs.push(FormError::ApiKeyRequired(row.id.clone()));
                }
            }
            ProviderKindForm::OpenAi => {
                if !row.base_url.is_empty() && BaseUrl::from_str(&row.base_url).is_err() {
                    errs.push(FormError::BaseUrlInvalid(row.id.clone()));
                }
                if row.key_ref_token.is_empty() && row.api_key_input.is_empty() {
                    errs.push(FormError::ApiKeyRequired(row.id.clone()));
                }
            }
            ProviderKindForm::Anthropic => {
                if row.key_ref_token.is_empty() && row.api_key_input.is_empty() {
                    errs.push(FormError::ApiKeyRequired(row.id.clone()));
                }
            }
            ProviderKindForm::Ollama => {
                if !row.base_url.is_empty() && BaseUrl::from_str(&row.base_url).is_err() {
                    errs.push(FormError::BaseUrlInvalid(row.id.clone()));
                }
            }
        }
    }

    if let Some(d) = &form.default_provider {
        if !form.providers.iter().any(|r| &r.id == d) {
            errs.push(FormError::DefaultProviderMissing(d.clone()));
        }
    }

    if errs.is_empty() {
        Ok(())
    } else {
        Err(errs)
    }
}
```

- [ ] **Step 5: Run all logic tests**

```
cargo test --manifest-path vendor/warp-stripped/Cargo.toml -p warp \
    --test omw_agent_page_logic_test --no-default-features --features omw_local
```

Expected: all tests PASS, including the three new ones and the two adjusted ones.

- [ ] **Step 6: Commit**

```
git add vendor/warp-stripped/app/src/settings_view/omw_agent_page.rs \
        vendor/warp-stripped/app/tests/omw_agent_page_logic_test.rs
git commit -m "agent-settings: validate only the default row's completeness

Non-default rows are drafts. Strict per-row validation blocked Apply
whenever any row was missing required fields, even if the user wasn't
ready to use that row. Move completeness checks behind a default-row
guard; syntactic checks (id grammar, dup ids, key_ref shape) still run
on every row.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: `form_to_config` skips incomplete non-default rows

**Files:**
- Modify: `vendor/warp-stripped/app/src/settings_view/omw_agent_page.rs:193-269` (form_to_config + helpers)
- Test: `vendor/warp-stripped/app/tests/omw_agent_page_logic_test.rs`

- [ ] **Step 1: Add failing test**

```rust
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
                base_url: String::new(), // missing required field
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
```

- [ ] **Step 2: Run test to verify it fails**

```
cargo test --manifest-path vendor/warp-stripped/Cargo.toml -p warp \
    --test omw_agent_page_logic_test --no-default-features --features omw_local \
    form_to_config_skips_incomplete_non_default_rows
```

Expected: FAIL — current form_to_config tries to convert every row, the stub row's missing base_url surfaces as a typed-config error.

- [ ] **Step 3: Add `is_row_complete` helper and gate the loop on it**

Insert before `form_to_config`:

```rust
/// Returns true iff this row has all kind-required fields populated such that
/// the typed `ProviderConfig` constructor will succeed. Mirrors the
/// completeness logic in `validate_form` for the default row.
fn is_row_complete(row: &ProviderRow, persisted_secrets: &BTreeMap<String, KeyRef>) -> bool {
    let has_key = !row.key_ref_token.is_empty()
        || !row.api_key_input.is_empty()
        || persisted_secrets.contains_key(&row.id);
    match row.kind {
        ProviderKindForm::OpenAiCompatible => !row.base_url.is_empty() && has_key,
        ProviderKindForm::OpenAi => has_key,
        ProviderKindForm::Anthropic => has_key,
        ProviderKindForm::Ollama => true,
    }
}
```

In `form_to_config`, after `validate_form(form)?`, change the row iteration to skip incomplete rows:

```rust
for row in &form.providers {
    if !is_row_complete(row, persisted_secrets) {
        continue;
    }
    // ... existing per-row conversion stays the same ...
}
```

- [ ] **Step 4: Run logic tests**

```
cargo test --manifest-path vendor/warp-stripped/Cargo.toml -p warp \
    --test omw_agent_page_logic_test --no-default-features --features omw_local
```

Expected: all PASS.

- [ ] **Step 5: Commit**

```
git add vendor/warp-stripped/app/src/settings_view/omw_agent_page.rs \
        vendor/warp-stripped/app/tests/omw_agent_page_logic_test.rs
git commit -m "agent-settings: skip incomplete rows when serializing config

Drafts only live in form state; only complete rows reach config.toml.
Pairs with validation gating from the prior commit so users can save
with incomplete non-default rows. Drafts vanish on app restart.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: Replace `SetDefault(usize)` with `SetDefaultProviderById(Option<String>)`

**Files:**
- Modify: `vendor/warp-stripped/app/src/settings_view/omw_agent_page.rs:74` (action enum), `:337-340` (reducer), `:1094` (button dispatch).
- Test: `vendor/warp-stripped/app/tests/omw_agent_page_logic_test.rs`

- [ ] **Step 1: Add failing test**

```rust
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
    apply_action(
        &mut s,
        OmwAgentPageAction::SetDefaultProviderById(None),
    );
    assert!(s.form.default_provider.is_none());
}

#[test]
fn apply_set_default_provider_by_id_ignores_unknown_ids() {
    let mut s = empty_state();
    apply_action(
        &mut s,
        OmwAgentPageAction::SetDefaultProviderById(Some("ghost".into())),
    );
    // Unknown id is rejected silently — keeps prior default (None here).
    assert!(s.form.default_provider.is_none());
}
```

- [ ] **Step 2: Run tests to verify they fail to compile**

```
cargo test --manifest-path vendor/warp-stripped/Cargo.toml -p warp \
    --test omw_agent_page_logic_test --no-default-features --features omw_local
```

Expected: FAIL — `SetDefaultProviderById` variant doesn't exist yet.

- [ ] **Step 3: Update enum and reducer**

In `omw_agent_page.rs`, replace the `SetDefault(usize),` enum variant (line 74) with:

```rust
SetDefaultProviderById(Option<String>),
```

Replace the reducer arm at lines 337-340 with:

```rust
OmwAgentPageAction::SetDefaultProviderById(maybe_id) => match maybe_id {
    Some(id) if state.form.providers.iter().any(|r| r.id == id) => {
        state.form.default_provider = Some(id);
    }
    Some(_) => {
        // Unknown id — ignore silently. Reachable only if dropdown
        // state desyncs from form.providers (e.g. row removed between
        // toggle-open and click).
    }
    None => {
        state.form.default_provider = None;
    }
},
```

- [ ] **Step 4: Update the per-row [Set Default] button dispatch (line 1094)**

Find the button dispatch site:

```rust
ctx.dispatch_typed_action(OmwAgentPageAction::SetDefault(idx));
```

Replace with:

```rust
let id = row.id.clone();
ctx.dispatch_typed_action(OmwAgentPageAction::SetDefaultProviderById(Some(id)));
```

(`row` is in scope from the surrounding `for (idx, row) in form.providers.iter().enumerate()` loop at line 934.)

- [ ] **Step 5: Update the `test_exports` mod if needed**

If `OmwAgentPageAction::SetDefault` was re-exported individually for tests, replace the export name. If `OmwAgentPageAction` is re-exported as the whole enum (most likely — check `app/src/lib.rs:122` `test_exports`), no change is needed. Run `cargo build` to verify.

- [ ] **Step 6: Run logic tests**

```
cargo test --manifest-path vendor/warp-stripped/Cargo.toml -p warp \
    --test omw_agent_page_logic_test --no-default-features --features omw_local
```

Expected: all PASS.

- [ ] **Step 7: Commit**

```
git add vendor/warp-stripped/app/src/settings_view/omw_agent_page.rs \
        vendor/warp-stripped/app/tests/omw_agent_page_logic_test.rs
git commit -m "agent-settings: unify default-provider mutation under id-keyed action

Replace SetDefault(usize) with SetDefaultProviderById(Option<String>),
so the upcoming dropdown and the existing per-row [Set Default] button
share one canonical mutation. None clears the default.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: Patch `SetProviderId` to rebuild canonical `key_ref_token`

**Files:**
- Modify: `vendor/warp-stripped/app/src/settings_view/omw_agent_page.rs:301-310`
- Test: `vendor/warp-stripped/app/tests/omw_agent_page_logic_test.rs`

- [ ] **Step 1: Add failing test**

```rust
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
```

- [ ] **Step 2: Run tests to verify they fail**

```
cargo test --manifest-path vendor/warp-stripped/Cargo.toml -p warp \
    --test omw_agent_page_logic_test --no-default-features --features omw_local \
    apply_set_provider_id_rebuilds_canonical_key_ref_token \
    apply_set_provider_id_leaves_non_canonical_key_ref_token_alone
```

Expected: `apply_set_provider_id_rebuilds_canonical_key_ref_token` FAILS (current code doesn't update key_ref_token); `apply_set_provider_id_leaves_non_canonical_key_ref_token_alone` PASSES (current code preserves the field by default).

- [ ] **Step 3: Patch `SetProviderId` reducer arm at lines 301-310**

Replace:

```rust
OmwAgentPageAction::SetProviderId(idx, new_id) => {
    if let Some(row) = state.form.providers.get_mut(idx) {
        let old = std::mem::replace(&mut row.id, new_id.clone());
        if state.form.default_provider.as_deref() == Some(&old) {
            state.form.default_provider = Some(new_id.clone());
        }
        if let Some(secret) = state.pending_secrets.remove(&old) {
            state.pending_secrets.insert(new_id, secret);
        }
    }
}
```

With:

```rust
OmwAgentPageAction::SetProviderId(idx, new_id) => {
    if let Some(row) = state.form.providers.get_mut(idx) {
        let old = std::mem::replace(&mut row.id, new_id.clone());
        // If the row's key_ref_token is the canonical form
        // `keychain:omw/<old_id>` (what Apply writes), rebuild it to
        // match the new id so the keychain lookup follows the rename.
        // Non-canonical user-pasted tokens are left alone.
        let canonical_old = format!("keychain:omw/{old}");
        if row.key_ref_token == canonical_old {
            row.key_ref_token = format!("keychain:omw/{new_id}");
        }
        if state.form.default_provider.as_deref() == Some(&old) {
            state.form.default_provider = Some(new_id.clone());
        }
        if let Some(secret) = state.pending_secrets.remove(&old) {
            state.pending_secrets.insert(new_id, secret);
        }
    }
}
```

- [ ] **Step 4: Run logic tests**

```
cargo test --manifest-path vendor/warp-stripped/Cargo.toml -p warp \
    --test omw_agent_page_logic_test --no-default-features --features omw_local
```

Expected: all PASS, including both new tests and `apply_set_provider_id_renames_default_and_pending_secret` (existing).

- [ ] **Step 5: Commit**

```
git add vendor/warp-stripped/app/src/settings_view/omw_agent_page.rs \
        vendor/warp-stripped/app/tests/omw_agent_page_logic_test.rs
git commit -m "agent-settings: rebuild canonical key_ref_token on rename

When a row's key_ref_token has the Apply-written form keychain:omw/<id>,
update it to match the new id so the keychain lookup follows the
rename. Non-canonical user-pasted tokens are preserved unchanged.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 5: Patch `SetProviderKind` to clear key fields on key-requirement boundary cross

**Files:**
- Modify: `vendor/warp-stripped/app/src/settings_view/omw_agent_page.rs:312-316`
- Test: `vendor/warp-stripped/app/tests/omw_agent_page_logic_test.rs`

- [ ] **Step 1: Add failing tests**

```rust
#[test]
fn apply_set_provider_kind_clears_key_fields_when_key_no_longer_required() {
    let mut s = empty_state();
    apply_action(&mut s, OmwAgentPageAction::AddProvider);
    s.form.providers[0].kind = ProviderKindForm::OpenAi;
    s.form.providers[0].key_ref_token = "keychain:omw/foo".into();
    s.form.providers[0].api_key_input = "sk-typed".into();
    s.pending_secrets.insert(s.form.providers[0].id.clone(), "sk-typed".into());

    apply_action(
        &mut s,
        OmwAgentPageAction::SetProviderKind(0, ProviderKindForm::Ollama),
    );

    assert_eq!(s.form.providers[0].kind, ProviderKindForm::Ollama);
    assert!(s.form.providers[0].key_ref_token.is_empty());
    assert!(s.form.providers[0].api_key_input.is_empty());
    assert!(!s.pending_secrets.contains_key(&s.form.providers[0].id));
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
```

- [ ] **Step 2: Run tests to verify they fail**

```
cargo test --manifest-path vendor/warp-stripped/Cargo.toml -p warp \
    --test omw_agent_page_logic_test --no-default-features --features omw_local \
    apply_set_provider_kind_clears_key_fields_when_key_no_longer_required \
    apply_set_provider_kind_preserves_key_fields_across_key_required_kinds
```

Expected: `apply_set_provider_kind_clears_key_fields_when_key_no_longer_required` FAILS (current SetProviderKind doesn't touch key fields); the preserve test PASSES.

- [ ] **Step 3: Patch `SetProviderKind` reducer arm at lines 312-316**

Add a `kind_requires_key` helper near the other module-private helpers (e.g. just before `apply_action`):

```rust
fn kind_requires_key(k: ProviderKindForm) -> bool {
    matches!(
        k,
        ProviderKindForm::OpenAi
            | ProviderKindForm::Anthropic
            | ProviderKindForm::OpenAiCompatible,
    )
}
```

Replace the `SetProviderKind` arm:

```rust
OmwAgentPageAction::SetProviderKind(idx, k) => {
    if let Some(row) = state.form.providers.get_mut(idx) {
        let prev = row.kind;
        row.kind = k;
        // When crossing the key-required boundary (e.g. OpenAI → Ollama),
        // clear stale key fields so validation matches the new kind's
        // requirements instead of carrying ghosts from the previous kind.
        if kind_requires_key(prev) && !kind_requires_key(k) {
            row.key_ref_token.clear();
            row.api_key_input.clear();
            state.pending_secrets.remove(&row.id);
        }
    }
}
```

- [ ] **Step 4: Run logic tests**

```
cargo test --manifest-path vendor/warp-stripped/Cargo.toml -p warp \
    --test omw_agent_page_logic_test --no-default-features --features omw_local
```

Expected: all PASS.

- [ ] **Step 5: Commit**

```
git add vendor/warp-stripped/app/src/settings_view/omw_agent_page.rs \
        vendor/warp-stripped/app/tests/omw_agent_page_logic_test.rs
git commit -m "agent-settings: clear stale key fields on kind change to keyless

Switching a row from a key-requiring kind (OpenAI / Anthropic /
OpenAiCompatible) to Ollama clears key_ref_token, api_key_input, and
the row's pending_secret so validation matches user intent. Switches
between key-requiring kinds preserve fields.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 6: Add `DefaultProviderDropdownState` + open/close/highlight actions

**Files:**
- Modify: `vendor/warp-stripped/app/src/settings_view/omw_agent_page.rs` — append struct near line 50, action variants near line 65, reducer arms in `apply_action`.
- Test: `vendor/warp-stripped/app/tests/omw_agent_page_logic_test.rs`

- [ ] **Step 1: Add failing tests**

```rust
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
```

These will fail compilation until the struct + actions exist.

Also extend `empty_state()` (test helper at line 15-24) to initialize the new field:

```rust
fn empty_state() -> OmwAgentPageState {
    let cfg = omw_config::Config::default();
    OmwAgentPageState {
        form: form_from_config(&cfg),
        saved_config: cfg,
        pending_secrets: BTreeMap::new(),
        is_dirty: false,
        last_save_error: None,
        default_provider_dropdown: Default::default(),
    }
}
```

- [ ] **Step 2: Run tests to verify failure**

```
cargo test --manifest-path vendor/warp-stripped/Cargo.toml -p warp \
    --test omw_agent_page_logic_test --no-default-features --features omw_local
```

Expected: compile error — types/variants don't exist.

- [ ] **Step 3: Add struct, action variants, reducer arms, and state field**

In `omw_agent_page.rs`, near the other module-public structs (after `OmwAgentPageState` at line ~50), add:

```rust
/// Open/closed state for the default-provider dropdown trigger. The
/// list of selectable items is derived from `OmwAgentForm::providers`
/// at render time — we don't cache it here.
#[derive(Clone, Default, Debug, PartialEq, Eq)]
pub struct DefaultProviderDropdownState {
    pub is_expanded: bool,
    pub highlighted_index: Option<usize>,
}
```

Add a field to `OmwAgentPageState`:

```rust
pub default_provider_dropdown: DefaultProviderDropdownState,
```

Add new action variants to `OmwAgentPageAction` (near line 65):

```rust
ToggleDefaultProviderDropdown,
CloseDefaultProviderDropdown,
MoveDefaultProviderHighlight(DefaultProviderHighlightDirection),
```

Add the direction enum near the action enum:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefaultProviderHighlightDirection {
    Up,
    Down,
}
```

Add reducer arms in `apply_action`:

```rust
OmwAgentPageAction::ToggleDefaultProviderDropdown => {
    state.default_provider_dropdown.is_expanded =
        !state.default_provider_dropdown.is_expanded;
    if !state.default_provider_dropdown.is_expanded {
        state.default_provider_dropdown.highlighted_index = None;
    }
}
OmwAgentPageAction::CloseDefaultProviderDropdown => {
    state.default_provider_dropdown.is_expanded = false;
    state.default_provider_dropdown.highlighted_index = None;
}
OmwAgentPageAction::MoveDefaultProviderHighlight(dir) => {
    let total = state.form.providers.len() + 1; // +1 for "(none)"
    if total == 0 {
        return;
    }
    let cur = state
        .default_provider_dropdown
        .highlighted_index
        .unwrap_or(0);
    let next = match dir {
        DefaultProviderHighlightDirection::Down => (cur + 1) % total,
        DefaultProviderHighlightDirection::Up => (cur + total - 1) % total,
    };
    state.default_provider_dropdown.highlighted_index = Some(next);
}
```

Update `OmwAgentPageView::new` (line ~465 area) so `OmwAgentPageState` instantiation includes `default_provider_dropdown: DefaultProviderDropdownState::default()`. The compile error from Step 2 points at every constructor that needs updating.

Update `test_exports` at `vendor/warp-stripped/app/src/lib.rs:122` to re-export `DefaultProviderDropdownState` and `DefaultProviderHighlightDirection` so the test file can name them (only if it needs to — the tests above only access via field access, no direct type names required, but check for compile errors).

- [ ] **Step 4: Run logic tests**

```
cargo test --manifest-path vendor/warp-stripped/Cargo.toml -p warp \
    --test omw_agent_page_logic_test --no-default-features --features omw_local
```

Expected: all PASS.

- [ ] **Step 5: Commit**

```
git add vendor/warp-stripped/app/src/settings_view/omw_agent_page.rs \
        vendor/warp-stripped/app/src/lib.rs \
        vendor/warp-stripped/app/tests/omw_agent_page_logic_test.rs
git commit -m "agent-settings: add DefaultProviderDropdown state + reducer arms

Open/closed + highlighted-index state for the upcoming dropdown
selector. Render comes in the next commit. Highlight actions wrap
around (down past last → first; up past first → last).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 7: Render the dropdown — replace the read-only label

**Files:**
- Modify: `vendor/warp-stripped/app/src/settings_view/omw_agent_page.rs:882-898` (label) and surrounding render context.

This task is GUI rendering with no unit-test surface. Verification is manual smoke.

- [ ] **Step 1: Read the GithubReposDropdown render entry point for shape reference**

Run:

```
grep -n "fn render\|render_repos_dropdown\|build_dropdown\|github_dropdown_state\|REPOS_DROPDOWN_ANCHOR" \
    vendor/warp-stripped/app/src/settings_view/update_environment_form.rs | head -30
```

Identify the function that renders the dropdown trigger + its expanded popover. Expect a pattern like: a `Button` for the closed trigger, an `if state.is_expanded { ... }` block that renders a vertical column of clickable items anchored below the trigger.

- [ ] **Step 2: Replace the label render at omw_agent_page.rs:882-898**

```rust
// default provider dropdown — replaces the prior read-only label.
{
    let trigger_text = form
        .default_provider
        .as_deref()
        .unwrap_or("(none)")
        .to_string();
    let mut row = Flex::row();
    row.add_child(
        Container::new(
            Text::new(
                "Default provider:".to_owned(),
                appearance.ui_font_family(),
                CONTENT_FONT_SIZE,
            )
            .with_color(active)
            .finish(),
        )
        .with_margin_right(8.)
        .finish(),
    );
    row.add_child(
        Button::new(format!("{trigger_text} ▾"))
            .on_click(|_, _, ctx| {
                ctx.dispatch_typed_action(
                    OmwAgentPageAction::ToggleDefaultProviderDropdown,
                );
            })
            .finish(),
    );
    col.add_child(Container::new(row.finish()).with_margin_bottom(4.).finish());

    if state.default_provider_dropdown.is_expanded {
        let mut menu = Flex::col();
        // "(none)" entry first.
        menu.add_child(
            Button::new("(none)".to_string())
                .on_click(|_, _, ctx| {
                    ctx.dispatch_typed_action(
                        OmwAgentPageAction::SetDefaultProviderById(None),
                    );
                    ctx.dispatch_typed_action(
                        OmwAgentPageAction::CloseDefaultProviderDropdown,
                    );
                })
                .finish(),
        );
        // Provider rows.
        for row in form.providers.iter() {
            let id = row.id.clone();
            let label = id.clone();
            menu.add_child(
                Button::new(label)
                    .on_click(move |_, _, ctx| {
                        ctx.dispatch_typed_action(
                            OmwAgentPageAction::SetDefaultProviderById(Some(id.clone())),
                        );
                        ctx.dispatch_typed_action(
                            OmwAgentPageAction::CloseDefaultProviderDropdown,
                        );
                    })
                    .finish(),
            );
        }
        col.add_child(
            Container::new(menu.finish())
                .with_margin_left(16.)
                .with_margin_bottom(12.)
                .finish(),
        );
    } else {
        col.add_child(Container::new(Text::empty()).with_margin_bottom(12.).finish());
    }
}
```

The exact `Button` / `Flex` / `Container` API surface lives in the `warpui` crate — adjust types/method names to match what's already imported at the top of `omw_agent_page.rs`. The block above is a structural template; replace `Button::new`, `on_click`, etc. with whatever the existing render code uses (line 826-877 `approval_row` is the reference for closures over indices, line 1090-1100 is the reference for click → dispatch on a per-row button).

- [ ] **Step 3: cargo build**

```
cargo build --manifest-path vendor/warp-stripped/Cargo.toml \
    -p warp --bin warp-oss --no-default-features --features omw_local
```

Expected: clean build. If any API shape is wrong, the compiler will name the missing method on `Button` / `Flex` / etc.; cross-reference against the approval_row render at line 826-877 of the same file.

- [ ] **Step 4: Manual smoke test**

```
cargo run --manifest-path vendor/warp-stripped/Cargo.toml \
    -p warp --bin warp-oss --no-default-features --features omw_local
```

In the running app:
1. Open Settings → Agent.
2. Confirm the trigger button shows the current default (or "(none)") next to "Default provider:".
3. Click the trigger → menu expands showing "(none)" + each provider id.
4. Click a provider id → menu collapses, trigger updates to show the new default.
5. Click "(none)" → trigger updates to "(none)", default cleared.
6. Add a provider, click trigger again → new id appears in the menu.
7. Remove the row currently set as default → trigger shows "(none)".
8. Verify Apply still works for the default row only when complete; incomplete non-default stub no longer blocks Apply (the original friend-bug repro).

- [ ] **Step 5: Commit**

```
git add vendor/warp-stripped/app/src/settings_view/omw_agent_page.rs
git commit -m "agent-settings: render Default provider as a dropdown selector

Replaces the read-only 'Default provider: foo' label with an expandable
trigger + (none)/<provider-id> menu. Wires to SetDefaultProviderById
+ CloseDefaultProviderDropdown actions added previously.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 9: Auto-create `~/.config/omw/config.toml` on first .app load

**Files:**
- Modify: `crates/omw-config/src/lib.rs` — add `Config::load_or_create_default(path: &Path) -> Result<Self, ConfigError>`.
- Test: `crates/omw-config/src/lib.rs` (the existing `#[cfg(test)] mod tests` block at line 128+).
- Modify: `vendor/warp-stripped/app/src/settings_view/omw_agent_page.rs:464` and `vendor/warp-stripped/app/src/ai_assistant/omw_agent_state.rs:679` — switch the .app's two `Config::load()` callers to `Config::load_or_create_default(&omw_config::config_path()?)`.

The CLI tools (`crates/omw-cli/src/commands/*.rs`) and `vendor/warp-stripped/app/src/terminal/input.rs:1461` continue to use plain `Config::load()` / `Config::load_from()`. Auto-create is a .app-only side effect — CLI users opt in by running `omw provider add ...`.

- [ ] **Step 1: Add failing test in `crates/omw-config/src/lib.rs`**

Append inside `#[cfg(test)] mod tests`:

```rust
#[test]
fn load_or_create_default_writes_file_when_missing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("omw").join("config.toml");
    assert!(!path.exists());

    let cfg = Config::load_or_create_default(&path).unwrap();
    assert_eq!(cfg, Config::default());
    assert!(path.exists(), "expected file to be created on first call");
    let on_disk = std::fs::read_to_string(&path).unwrap();
    assert!(
        on_disk.contains("version = 1"),
        "expected serialized default config, got: {on_disk:?}"
    );
}

#[test]
fn load_or_create_default_is_a_noop_when_file_exists() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        "version = 1\n[approval]\nmode = \"trusted\"\n[agent]\nenabled = false\n",
    )
    .unwrap();
    let mtime_before = std::fs::metadata(&path).unwrap().modified().unwrap();

    let cfg = Config::load_or_create_default(&path).unwrap();
    assert_eq!(cfg.approval.mode, ApprovalMode::Trusted);
    assert!(!cfg.agent.enabled);

    let mtime_after = std::fs::metadata(&path).unwrap().modified().unwrap();
    assert_eq!(
        mtime_before, mtime_after,
        "existing file must not be rewritten"
    );
}

#[test]
fn load_or_create_default_propagates_parse_errors() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "this = = is not valid toml").unwrap();
    let err = Config::load_or_create_default(&path).unwrap_err();
    assert!(matches!(err, ConfigError::Parse { .. }), "got: {err:?}");
}
```

- [ ] **Step 2: Run tests to verify failure**

```
cargo test --manifest-path Cargo.toml -p omw-config \
    load_or_create_default_writes_file_when_missing \
    load_or_create_default_is_a_noop_when_file_exists \
    load_or_create_default_propagates_parse_errors
```

Expected: compile error — method doesn't exist.

- [ ] **Step 3: Implement the method in `crates/omw-config/src/lib.rs`**

Add as a new method on the existing `impl Config` block (just after `load_from` at line 79-96):

```rust
/// Load the config from `path`. If the file does not exist, write a
/// fresh `Config::default()` to it (creating parent directories as
/// needed) and return that default. Other I/O / parse errors are
/// surfaced unchanged. Idempotent: existing files are never rewritten.
///
/// .app callers use this to materialize `~/.config/omw/config.toml` on
/// first launch so the file is discoverable to CLI users and to
/// hand-editors. CLI tools keep using `load` / `load_from` and create
/// the file lazily on the first `omw provider add ...` etc.
pub fn load_or_create_default(path: &Path) -> Result<Self, ConfigError> {
    if path.exists() {
        return Self::load_from(path);
    }
    let cfg = Self::default();
    cfg.save_atomic(path)?;
    Ok(cfg)
}
```

- [ ] **Step 4: Run tests, expect PASS**

```
cargo test --manifest-path Cargo.toml -p omw-config
```

Expected: full omw-config test suite PASSES, including the three new tests.

- [ ] **Step 5: Switch the two .app `Config::load()` callers**

`vendor/warp-stripped/app/src/settings_view/omw_agent_page.rs:464`:

Replace:

```rust
let cfg = omw_config::Config::load().unwrap_or_default();
```

With:

```rust
let cfg = match omw_config::config_path() {
    Ok(p) => omw_config::Config::load_or_create_default(&p).unwrap_or_default(),
    Err(_) => omw_config::Config::default(),
};
```

`vendor/warp-stripped/app/src/ai_assistant/omw_agent_state.rs:679`:

Replace:

```rust
let cfg = omw_config::Config::load().map_err(|e| e.to_string())?;
```

With:

```rust
let path = omw_config::config_path().map_err(|e| e.to_string())?;
let cfg = omw_config::Config::load_or_create_default(&path).map_err(|e| e.to_string())?;
```

Leave `vendor/warp-stripped/app/src/terminal/input.rs:1461` unchanged — that path is reached via terminal command processing and shouldn't auto-write.

- [ ] **Step 6: Build the .app and confirm a fresh-install simulation**

```
rm -rf /tmp/omw-fresh-config && \
mkdir -p /tmp/omw-fresh-config && \
OMW_CONFIG=/tmp/omw-fresh-config/config.toml \
cargo run --manifest-path vendor/warp-stripped/Cargo.toml \
    -p warp --bin warp-oss --no-default-features --features omw_local
```

In the running app: open Settings → Agent (this is enough to trigger the page-side load_or_create_default).

In another terminal:

```
cat /tmp/omw-fresh-config/config.toml
```

Expected: a TOML file containing `version = 1`, `[approval] mode = "ask_before_write"`, `[agent] enabled = true`, `[providers]` (empty table). Quit the app.

- [ ] **Step 7: Commit**

```
git add crates/omw-config/src/lib.rs \
        vendor/warp-stripped/app/src/settings_view/omw_agent_page.rs \
        vendor/warp-stripped/app/src/ai_assistant/omw_agent_state.rs
git commit -m "omw-config: add load_or_create_default for .app first-launch

Materializes ~/.config/omw/config.toml on first .app launch so the
file is discoverable to omw-cli users and to anyone editing TOML by
hand. CLI tools and the terminal command handler keep their pure-load
semantics — they create the file lazily on the first mutation.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 8: Final smoke + sanity test of the full v0.0.3 friend-bug repro path

**Files:** none

- [ ] **Step 1: Repro the friend's flow**

In the running dev build:
1. Settings → Agent.
2. Click "Add Provider" — get a stub OpenAI row with no api_key.
3. Click Apply.
4. Expected: Apply succeeds, no `ApiKeyRequired` error, `last_save_error` is empty.
5. Reopen Settings → Agent. The stub row is **gone** (drafts vanish on restart, per design). Document this in commit notes if not already.

- [ ] **Step 2: Repro the rename + key_ref tracking**

1. Add a complete provider, fill in api_key, mark it default, Apply. Confirm `~/.config/omw/config.toml` has `key_ref = "keychain:omw/<id>"`.
2. Rename the row id in the UI.
3. Apply.
4. Confirm `config.toml` now has `key_ref = "keychain:omw/<new-id>"` and the keychain entry under the new account exists (`security find-generic-password -s omw -a "omw/<new-id>" -w`).
5. (The prior keychain entry for `<old-id>` may still exist — it's orphaned. That's a separate cleanup-on-rename concern, out of scope here.)

- [ ] **Step 3: Repro the kind-change clearing**

1. Add an OpenAI row, type an api_key. Confirm `pending_secrets` has the entry (e.g. via a temporary `dbg!` if needed, or just observe behavior).
2. Switch the row's kind to Ollama.
3. Confirm api_key field is now empty in the UI.
4. Switch back to OpenAI.
5. Confirm api_key field stays empty (we cleared on the cross; switching back doesn't restore — by design).

- [ ] **Step 4: Final commit if any tweaks made during smoke**

If smoke testing surfaces any small bugs, fix them in-place in `omw_agent_page.rs` and commit with descriptive messages. Otherwise no extra commit.

---

## Self-Review

**Spec coverage:**
- ✅ Default provider editable → Tasks 6, 7.
- ✅ Validate only default row → Task 1.
- ✅ Drafts vanish on restart → Task 2 (form_to_config skips incomplete; reload from TOML produces no row).
- ✅ Rename canonical key_ref rebuild → Task 4.
- ✅ Kind-change clearing → Task 5.
- ✅ Action consolidation → Task 3.
- ✅ Manual smoke covers all three friend-bug repros → Task 8.

**Placeholder scan:** none. Each task has concrete code blocks and exact paths.

**Type consistency:** `SetDefaultProviderById(Option<String>)` is named identically across Tasks 3, 6, 7. `DefaultProviderDropdownState`, `DefaultProviderHighlightDirection` likewise consistent. `kind_requires_key` is a private helper — only Task 5 references it.

**Task 7 caveat:** the dropdown render uses placeholder API names (`Button::new`, `Flex::col`) that the implementer must reconcile with the actual `warpui` types already imported at the top of `omw_agent_page.rs`. The reference is the approval_row render at line 826-877 and the per-row Set Default button at line 1090-1100 — both demonstrate the real API surface.
