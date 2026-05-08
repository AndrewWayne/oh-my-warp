//! Agent settings page. State, reducer, validators are pure functions
//! tested via the integration test at `app/tests/omw_agent_page_logic_test.rs`
//! (sidesteps the broken settings_view::mod_test.rs lib target). Render
//! lives in this same module under a separate `pub fn render(...)` once the
//! data layer is locked in (see plan Task 6).

#![cfg(feature = "omw_local")]

use std::collections::BTreeMap;
use std::str::FromStr;

use omw_config::{
    AgentConfig, ApprovalConfig, ApprovalMode, BaseUrl, Config, KeyRef, ProviderConfig,
    ProviderId,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OmwAgentForm {
    pub agent_enabled: bool,
    pub approval_mode: ApprovalMode,
    pub default_provider: Option<String>,
    pub providers: Vec<ProviderRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRow {
    pub id: String,
    pub kind: ProviderKindForm,
    pub model: String,
    pub base_url: String,
    pub key_ref_token: String,
    pub api_key_input: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKindForm {
    OpenAi,
    Anthropic,
    OpenAiCompatible,
    Ollama,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormError {
    InvalidProviderId(String),
    DuplicateProviderId(String),
    DefaultProviderMissing(String),
    BaseUrlRequired(String),
    BaseUrlInvalid(String),
    ApiKeyRequired(String),
    KeyRefInvalid(String),
}

/// Open/closed state for the default-provider dropdown trigger. The
/// list of selectable items is derived from `OmwAgentForm::providers`
/// at render time — we don't cache it here.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DefaultProviderDropdownState {
    pub is_expanded: bool,
    pub highlighted_index: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultProviderHighlightDirection {
    Up,
    Down,
}

#[derive(Debug, Clone)]
pub struct OmwAgentPageState {
    pub saved_config: Config,
    pub form: OmwAgentForm,
    pub pending_secrets: BTreeMap<String, String>,
    pub is_dirty: bool,
    pub last_save_error: Option<String>,
    pub default_provider_dropdown: DefaultProviderDropdownState,
}

#[derive(Debug, Clone)]
pub enum OmwAgentPageAction {
    ToggleEnabled,
    SetApprovalMode(ApprovalMode),
    AddProvider,
    RemoveProvider(usize),
    SetProviderId(usize, String),
    SetProviderKind(usize, ProviderKindForm),
    SetProviderModel(usize, String),
    SetProviderBaseUrl(usize, String),
    SetProviderApiKey(usize, String),
    SetDefaultProviderById(Option<String>),
    ToggleDefaultProviderDropdown,
    CloseDefaultProviderDropdown,
    MoveDefaultProviderHighlight(DefaultProviderHighlightDirection),
    Apply,
    Discard,
}

// ---------------------- Pure converters ----------------------

pub fn form_from_config(cfg: &Config) -> OmwAgentForm {
    let providers: Vec<ProviderRow> = cfg
        .providers
        .iter()
        .map(|(id, pcfg)| ProviderRow {
            id: id.as_str().to_string(),
            kind: kind_from_config(pcfg),
            model: pcfg.default_model().unwrap_or("").to_string(),
            base_url: base_url_from_config(pcfg).unwrap_or_default(),
            key_ref_token: pcfg
                .key_ref()
                .map(|k| k.to_string())
                .unwrap_or_default(),
            api_key_input: String::new(),
        })
        .collect();

    OmwAgentForm {
        agent_enabled: cfg.agent.enabled,
        approval_mode: cfg.approval.mode,
        default_provider: cfg.default_provider.as_ref().map(|p| p.as_str().to_string()),
        providers,
    }
}

fn kind_from_config(pcfg: &ProviderConfig) -> ProviderKindForm {
    match pcfg {
        ProviderConfig::OpenAi { .. } => ProviderKindForm::OpenAi,
        ProviderConfig::Anthropic { .. } => ProviderKindForm::Anthropic,
        ProviderConfig::OpenAiCompatible { .. } => ProviderKindForm::OpenAiCompatible,
        ProviderConfig::Ollama { .. } => ProviderKindForm::Ollama,
    }
}

fn base_url_from_config(pcfg: &ProviderConfig) -> Option<String> {
    match pcfg {
        ProviderConfig::OpenAiCompatible { base_url, .. } => Some(base_url.as_str().to_string()),
        ProviderConfig::Ollama { base_url, .. } => {
            base_url.as_ref().map(|u| u.as_str().to_string())
        }
        ProviderConfig::OpenAi { base_url, .. } => {
            base_url.as_ref().map(|u| u.as_str().to_string())
        }
        _ => None,
    }
}

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

        // Completeness pass — only the row marked as default must be
        // fully filled in. Other rows are drafts; form_to_config skips
        // them at serialization time so they never reach config.toml.
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

/// True iff this row has all kind-required fields populated such that the
/// typed `ProviderConfig` constructor will succeed. Mirrors the completeness
/// logic in `validate_form` for the default row. Non-default rows are
/// drafts; `form_to_config` filters them out on this check so they never
/// land in config.toml.
/// True iff this provider kind requires an API key. Used by
/// `SetProviderKind` to decide whether to clear stale key fields when
/// the user switches a row's kind across the boundary (e.g. OpenAI →
/// Ollama clears; OpenAI → Anthropic preserves).
fn kind_requires_key(k: ProviderKindForm) -> bool {
    matches!(
        k,
        ProviderKindForm::OpenAi
            | ProviderKindForm::Anthropic
            | ProviderKindForm::OpenAiCompatible,
    )
}

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

pub fn form_to_config(
    form: &OmwAgentForm,
    persisted_secrets: &BTreeMap<String, KeyRef>,
) -> Result<Config, Vec<FormError>> {
    validate_form(form)?;

    let mut providers = BTreeMap::new();
    for row in &form.providers {
        if !is_row_complete(row, persisted_secrets) {
            continue;
        }
        let id = ProviderId::from_str(&row.id)
            .map_err(|_| vec![FormError::InvalidProviderId(row.id.clone())])?;
        let model = if row.model.is_empty() {
            None
        } else {
            Some(row.model.clone())
        };
        let key_ref = persisted_secrets
            .get(&row.id)
            .cloned()
            .or_else(|| {
                if row.key_ref_token.is_empty() {
                    None
                } else {
                    KeyRef::from_str(&row.key_ref_token).ok()
                }
            });

        let pcfg = match row.kind {
            ProviderKindForm::OpenAi => ProviderConfig::OpenAi {
                key_ref: key_ref
                    .ok_or_else(|| vec![FormError::ApiKeyRequired(row.id.clone())])?,
                default_model: model,
                base_url: if row.base_url.is_empty() {
                    None
                } else {
                    Some(BaseUrl::from_str(&row.base_url).map_err(|_| {
                        vec![FormError::BaseUrlInvalid(row.id.clone())]
                    })?)
                },
            },
            ProviderKindForm::Anthropic => ProviderConfig::Anthropic {
                key_ref: key_ref
                    .ok_or_else(|| vec![FormError::ApiKeyRequired(row.id.clone())])?,
                default_model: model,
            },
            ProviderKindForm::OpenAiCompatible => ProviderConfig::OpenAiCompatible {
                key_ref: key_ref
                    .ok_or_else(|| vec![FormError::ApiKeyRequired(row.id.clone())])?,
                base_url: BaseUrl::from_str(&row.base_url)
                    .map_err(|_| vec![FormError::BaseUrlInvalid(row.id.clone())])?,
                default_model: model,
            },
            ProviderKindForm::Ollama => {
                let base_url = if row.base_url.is_empty() {
                    None
                } else {
                    Some(BaseUrl::from_str(&row.base_url).map_err(|_| {
                        vec![FormError::BaseUrlInvalid(row.id.clone())]
                    })?)
                };
                ProviderConfig::Ollama {
                    base_url,
                    key_ref,
                    default_model: model,
                }
            }
        };
        providers.insert(id, pcfg);
    }

    let default_provider = form
        .default_provider
        .as_ref()
        .and_then(|s| ProviderId::from_str(s).ok());

    Ok(Config {
        version: Default::default(),
        default_provider,
        providers,
        approval: ApprovalConfig {
            mode: form.approval_mode,
        },
        agent: AgentConfig {
            enabled: form.agent_enabled,
        },
    })
}

pub fn apply_action(state: &mut OmwAgentPageState, action: OmwAgentPageAction) {
    match action {
        OmwAgentPageAction::ToggleEnabled => state.form.agent_enabled = !state.form.agent_enabled,
        OmwAgentPageAction::SetApprovalMode(m) => state.form.approval_mode = m,
        OmwAgentPageAction::AddProvider => state.form.providers.push(ProviderRow {
            id: format!("provider-{}", state.form.providers.len() + 1),
            kind: ProviderKindForm::OpenAi,
            model: String::new(),
            base_url: String::new(),
            key_ref_token: String::new(),
            api_key_input: String::new(),
        }),
        OmwAgentPageAction::RemoveProvider(idx) => {
            if idx < state.form.providers.len() {
                let removed = state.form.providers.remove(idx);
                if state.form.default_provider.as_deref() == Some(&removed.id) {
                    state.form.default_provider = None;
                }
                state.pending_secrets.remove(&removed.id);
            }
        }
        OmwAgentPageAction::SetProviderId(idx, new_id) => {
            if let Some(row) = state.form.providers.get_mut(idx) {
                let old = std::mem::replace(&mut row.id, new_id.clone());
                // If the row's key_ref_token is the canonical form
                // `keychain:omw/<old_id>` (what Apply writes), rebuild it
                // to match the new id so the keychain lookup follows the
                // rename. Non-canonical user-pasted tokens are left alone.
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
        OmwAgentPageAction::SetProviderKind(idx, k) => {
            if let Some(row) = state.form.providers.get_mut(idx) {
                let prev = row.kind;
                row.kind = k;
                // When crossing the key-required boundary (e.g. OpenAI →
                // Ollama), clear stale key fields so validation matches
                // the new kind's requirements instead of carrying ghosts
                // from the previous kind.
                if kind_requires_key(prev) && !kind_requires_key(k) {
                    row.key_ref_token.clear();
                    row.api_key_input.clear();
                    state.pending_secrets.remove(&row.id);
                }
            }
        }
        OmwAgentPageAction::SetProviderModel(idx, s) => {
            if let Some(row) = state.form.providers.get_mut(idx) {
                row.model = s;
            }
        }
        OmwAgentPageAction::SetProviderBaseUrl(idx, s) => {
            if let Some(row) = state.form.providers.get_mut(idx) {
                row.base_url = s;
            }
        }
        OmwAgentPageAction::SetProviderApiKey(idx, s) => {
            if let Some(row) = state.form.providers.get_mut(idx) {
                if s.is_empty() {
                    state.pending_secrets.remove(&row.id);
                } else {
                    state.pending_secrets.insert(row.id.clone(), s.clone());
                }
                row.api_key_input = s;
            }
        }
        OmwAgentPageAction::SetDefaultProviderById(maybe_id) => match maybe_id {
            Some(id) if state.form.providers.iter().any(|r| r.id == id) => {
                state.form.default_provider = Some(id);
            }
            Some(_) => {
                // Unknown id — ignore silently. Reachable only if dropdown
                // state desyncs from form.providers (e.g. row removed
                // between toggle-open and click).
            }
            None => {
                state.form.default_provider = None;
            }
        },
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
            // Total selectable rows = providers + 1 for "(none)".
            let total = state.form.providers.len() + 1;
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
        OmwAgentPageAction::Apply => {
            // Apply is a *side-effecting* action; the page glue (Task 6)
            // wraps this branch to call omw-keychain + writer. The pure
            // reducer leaves the dirty state intact for the caller to
            // resolve.
        }
        OmwAgentPageAction::Discard => {
            state.form = form_from_config(&state.saved_config);
            state.pending_secrets.clear();
            state.is_dirty = false;
            state.last_save_error = None;
            return;
        }
    }
    state.is_dirty = state.form != form_from_config(&state.saved_config);
}

// ---------------- View ----------------
//
// Render strategy (Task 6 minimal): the page is a Monolith that displays the
// current form as plain text labels (heading + flags + provider list) plus
// Apply/Discard buttons. The L3a interaction tests in Task 7 exercise
// `OmwAgentPageView::dispatch` and `apply` directly, so the rendered tree
// is intentionally simple — full inline editing widgets land later.

use crate::appearance::Appearance;
use crate::view_components::{SubmittableTextInput, SubmittableTextInputEvent};
use warpui::{
    elements::{
        ChildView, Container, CrossAxisAlignment, Element, Flex, MainAxisAlignment, MouseStateHandle,
        ParentElement, Text,
    },
    ui_components::{
        button::ButtonVariant,
        components::UiComponent,
    },
    AppContext, Entity, TypedActionView, View, ViewContext, ViewHandle,
};

/// Maximum number of provider rows the editable UI pre-allocates input
/// widgets for. Beyond this, users fall back to editing
/// `~/.config/omw/config.toml` directly. Keeping this static (instead of
/// growing the editor list dynamically) avoids the significantly more
/// complex lifecycle wiring that warpui's view-handle model demands for
/// runtime-spawned children.
const MAX_PROVIDER_SLOTS: usize = 8;

/// Per-provider editor widgets. Created up-front in [`OmwAgentPageView::new`]
/// for `MAX_PROVIDER_SLOTS` rows and rendered conditionally based on the
/// current form's provider count. Each input subscribes to
/// `SubmittableTextInputEvent::Submit` and dispatches the corresponding
/// `Set*` action onto the page's typed action stream — same path the
/// L3a integration tests already exercise.
pub struct ProviderRowEditors {
    pub id_input: ViewHandle<SubmittableTextInput>,
    pub model_input: ViewHandle<SubmittableTextInput>,
    pub base_url_input: ViewHandle<SubmittableTextInput>,
    pub api_key_input: ViewHandle<SubmittableTextInput>,
    pub set_default_button: MouseStateHandle,
    pub remove_button: MouseStateHandle,
    /// One toggle per provider kind: openai, anthropic,
    /// openai-compatible, ollama. Index lines up with
    /// `[ProviderKindForm::OpenAi, Anthropic, OpenAiCompatible, Ollama]`.
    pub kind_buttons: [MouseStateHandle; 4],
}

use super::settings_page::{
    MatchData, PageType, SettingsPageEvent, SettingsPageMeta, SettingsPageViewHandle,
    SettingsWidget, CONTENT_FONT_SIZE,
};
use super::SettingsSection;

/// View struct held by the page. Owns the form state plus mouse-state handles
/// for the Apply/Discard buttons. Click handlers dispatch
/// [`OmwAgentPageAction::Apply`] / [`OmwAgentPageAction::Discard`] which
/// route through [`TypedActionView::handle_action`] back into
/// [`Self::dispatch`].
pub struct OmwAgentPageView {
    pub state: OmwAgentPageState,
    pub apply_button: MouseStateHandle,
    pub discard_button: MouseStateHandle,
    pub add_provider_button: MouseStateHandle,
    /// Toggle button for the global `agent.enabled` flag. Renders as a
    /// single button whose label flips between "Enabled" / "Disabled";
    /// click dispatches [`OmwAgentPageAction::ToggleEnabled`].
    pub agent_enabled_button: MouseStateHandle,
    /// One handle per approval-mode variant (read-only / ask-before-write
    /// / trusted). Indexed in the same order as the iteration in
    /// [`OmwAgentPageWidget::render`] so the click handlers can pick by
    /// position without a separate lookup.
    pub approval_mode_buttons: [MouseStateHandle; 3],
    /// Trigger button for the default-provider dropdown (toggles
    /// expansion). Pre-allocated so hover/click animation state survives
    /// re-renders, matching the convention used by the rest of the page's
    /// buttons.
    pub default_provider_trigger_button: MouseStateHandle,
    /// Click handle for the dropdown's "(none)" menu item.
    pub default_provider_none_item_button: MouseStateHandle,
    /// Click handles for each provider row inside the expanded dropdown
    /// menu. Indexed by row position; rows past `form.providers.len()`
    /// are never rendered.
    pub default_provider_item_buttons: [MouseStateHandle; MAX_PROVIDER_SLOTS],
    /// Per-row editor widgets. Empty when constructed via
    /// [`Self::new_inner`] (used by L3a tests that drive the reducer
    /// directly without rendering); fully populated when constructed
    /// via [`Self::new`] inside a real `ViewContext`.
    pub provider_editors: Vec<ProviderRowEditors>,
    page: PageType<Self>,
}

impl OmwAgentPageView {
    pub fn new(ctx: &mut ViewContext<Self>) -> Self {
        let mut me = Self::new_inner();
        // Pre-allocate one editor set per slot. Each set wires its own
        // Submit subscription that dispatches a Set* action on the page;
        // the same dispatch path already used by `apply_action` /
        // `handle_action`. Events for slots > current row count are
        // simply never rendered, so they're silent until the user adds
        // more providers.
        for slot in 0..MAX_PROVIDER_SLOTS {
            me.provider_editors.push(make_provider_row_editors(slot, ctx));
        }
        // Sync the editor buffers to the loaded form so the user sees
        // the existing values.
        me.refresh_editor_buffers(ctx);
        me
    }

    /// App-context-free constructor. Used by integration tests in
    /// `app/tests/` to mount the view without a full `warpui::App`.
    /// Editor handles stay empty; tests dispatch reducer actions
    /// directly without going through the rendered widgets.
    pub fn new_inner() -> Self {
        // load_or_create_default materializes ~/.config/omw/config.toml on
        // first launch so the file is discoverable. Falls back to an
        // in-memory default on any error so the settings page still opens.
        let cfg = match omw_config::config_path() {
            Ok(p) => omw_config::Config::load_or_create_default(&p).unwrap_or_default(),
            Err(_) => omw_config::Config::default(),
        };
        let form = form_from_config(&cfg);
        Self {
            state: OmwAgentPageState {
                saved_config: cfg,
                form,
                pending_secrets: BTreeMap::new(),
                is_dirty: false,
                last_save_error: None,
                default_provider_dropdown: DefaultProviderDropdownState::default(),
            },
            apply_button: MouseStateHandle::default(),
            discard_button: MouseStateHandle::default(),
            add_provider_button: MouseStateHandle::default(),
            agent_enabled_button: MouseStateHandle::default(),
            approval_mode_buttons: [
                MouseStateHandle::default(),
                MouseStateHandle::default(),
                MouseStateHandle::default(),
            ],
            default_provider_trigger_button: MouseStateHandle::default(),
            default_provider_none_item_button: MouseStateHandle::default(),
            default_provider_item_buttons: std::array::from_fn(|_| {
                MouseStateHandle::default()
            }),
            provider_editors: Vec::new(),
            // is_dual_scrollable=true: long provider lists need to scroll;
            // PageType::wrap_dual_scrollable handles vertical clipping +
            // adds a horizontal scroll only when the window is narrower
            // than MIN_PAGE_WIDTH.
            page: PageType::new_monolith(OmwAgentPageWidget, Some("Agent"), true),
        }
    }

    /// Push the current `form.providers` text values into the editor
    /// buffers. Called on construction and after `Discard` so the UI
    /// reflects the model after a non-typing-driven change.
    pub fn refresh_editor_buffers(&mut self, ctx: &mut ViewContext<Self>) {
        for (i, editors) in self.provider_editors.iter_mut().enumerate() {
            let row = self.state.form.providers.get(i);
            let id_text = row.map(|r| r.id.clone()).unwrap_or_default();
            let model_text = row.map(|r| r.model.clone()).unwrap_or_default();
            let base_url_text = row.map(|r| r.base_url.clone()).unwrap_or_default();
            // Don't show api_key text — it's a secret. Always blank
            // unless the user is mid-edit; pending_secrets carries
            // the in-flight value separately.
            let _ = (&editors.id_input, &editors.model_input, &editors.base_url_input);
            // Buffer updates require nested view context — defer to a
            // helper that keeps the borrow scopes manageable.
            set_input_text(&editors.id_input, &id_text, ctx);
            set_input_text(&editors.model_input, &model_text, ctx);
            set_input_text(&editors.base_url_input, &base_url_text, ctx);
            set_input_text(&editors.api_key_input, "", ctx);
        }
    }

    /// Dispatch a non-Apply action through the pure reducer; Apply has a
    /// dedicated method below because it touches the keychain and disk.
    pub fn dispatch(&mut self, action: OmwAgentPageAction) {
        match action {
            OmwAgentPageAction::Apply => self.apply(),
            other => apply_action(&mut self.state, other),
        }
    }

    /// Side-effecting Apply: writes pending API keys to the OS keychain, then
    /// serialises the form to TOML via `omw_config::save_atomic`. On any
    /// failure, sets `last_save_error` and leaves `saved_config` alone.
    /// Per spec D10, the keychain write happens BEFORE the TOML write.
    pub fn apply(&mut self) {
        // 1. Pre-flight validation.
        if let Err(errs) = validate_form(&self.state.form) {
            self.state.last_save_error = Some(format!("validation failed: {errs:?}"));
            return;
        }

        // 2. Resolve key_refs by writing each pending secret to keychain.
        let mut resolved_key_refs: BTreeMap<String, KeyRef> = BTreeMap::new();
        for (id, secret) in &self.state.pending_secrets {
            let kr = match KeyRef::from_str(&format!("keychain:omw/{id}")) {
                Ok(k) => k,
                Err(e) => {
                    self.state.last_save_error =
                        Some(format!("invalid key_ref for {id}: {e}"));
                    return;
                }
            };
            if let Err(e) = omw_keychain::set(&kr, secret) {
                self.state.last_save_error = Some(format!("keychain set failed: {e}"));
                return;
            }
            resolved_key_refs.insert(id.clone(), kr);
        }

        // 3. Overlay resolved key_refs onto the form before serialising.
        let mut form_with_keys = self.state.form.clone();
        for row in &mut form_with_keys.providers {
            if let Some(kr) = resolved_key_refs.get(&row.id) {
                row.key_ref_token = kr.to_string();
            }
            row.api_key_input.clear();
        }

        // 4. Convert to typed Config.
        let cfg = match form_to_config(&form_with_keys, &resolved_key_refs) {
            Ok(c) => c,
            Err(errs) => {
                self.state.last_save_error = Some(format!("conversion failed: {errs:?}"));
                return;
            }
        };

        // 5. Resolve config path and save atomically.
        let path = match omw_config::config_path() {
            Ok(p) => p,
            Err(e) => {
                self.state.last_save_error = Some(format!("path resolution failed: {e}"));
                return;
            }
        };
        if let Err(e) = omw_config::save_atomic(&path, &cfg) {
            self.state.last_save_error = Some(format!("save failed: {e}"));
            return;
        }

        // 6. Re-derive form from the new saved config.
        self.state.saved_config = cfg.clone();
        self.state.form = form_from_config(&cfg);
        self.state.pending_secrets.clear();
        self.state.is_dirty = false;
        self.state.last_save_error = None;

        // 7. Reset live agent state so the new config takes effect
        //    without an app restart. Each per-pane `# foo` session and
        //    the singleton panel session cache the provider/model/key
        //    they were started with — drop them here so the next
        //    interaction re-provisions against the freshly-saved
        //    config.toml. Best-effort: if the runtime is still warming
        //    up, `stop()` is a no-op and `clear_all_pane_sessions` just
        //    finds an empty map.
        let agent_state = crate::ai_assistant::omw_agent_state::OmwAgentState::shared();
        agent_state.stop();
        agent_state.clear_all_pane_sessions();
    }
}

/// Construct one set of editor widgets for the row at `slot`. Each
/// SubmittableTextInput subscribes to its own Submit event and
/// dispatches the corresponding `Set*` action onto the parent page —
/// the dispatch path is the existing `OmwAgentPageView::handle_action`,
/// which routes back into `dispatch` and eventually `apply_action`,
/// keeping the pure-reducer test surface intact.
fn make_provider_row_editors(
    slot: usize,
    ctx: &mut ViewContext<OmwAgentPageView>,
) -> ProviderRowEditors {
    let id_input = ctx.add_typed_action_view(|ctx| {
        let mut input = SubmittableTextInput::new(ctx);
        input.set_placeholder_text("provider id (e.g. openai-prod)", ctx);
        input
    });
    ctx.subscribe_to_view(&id_input, move |_, _, event, ctx| {
        if let SubmittableTextInputEvent::Submit(s) = event {
            ctx.dispatch_typed_action(&OmwAgentPageAction::SetProviderId(slot, s.clone()));
        }
    });

    let model_input = ctx.add_typed_action_view(|ctx| {
        let mut input = SubmittableTextInput::new(ctx);
        input.set_placeholder_text("model id (e.g. gpt-4o)", ctx);
        input
    });
    ctx.subscribe_to_view(&model_input, move |_, _, event, ctx| {
        if let SubmittableTextInputEvent::Submit(s) = event {
            ctx.dispatch_typed_action(&OmwAgentPageAction::SetProviderModel(slot, s.clone()));
        }
    });

    let base_url_input = ctx.add_typed_action_view(|ctx| {
        let mut input = SubmittableTextInput::new(ctx);
        input.set_placeholder_text("https://api.openai.com/v1 (optional)", ctx);
        input
    });
    ctx.subscribe_to_view(&base_url_input, move |_, _, event, ctx| {
        if let SubmittableTextInputEvent::Submit(s) = event {
            ctx.dispatch_typed_action(&OmwAgentPageAction::SetProviderBaseUrl(slot, s.clone()));
        }
    });

    let api_key_input = ctx.add_typed_action_view(|ctx| {
        let mut input = SubmittableTextInput::new(ctx);
        input.set_placeholder_text("API key (will be saved to keychain on Apply)", ctx);
        input
    });
    ctx.subscribe_to_view(&api_key_input, move |_, _, event, ctx| {
        if let SubmittableTextInputEvent::Submit(s) = event {
            ctx.dispatch_typed_action(&OmwAgentPageAction::SetProviderApiKey(slot, s.clone()));
        }
    });

    ProviderRowEditors {
        id_input,
        model_input,
        base_url_input,
        api_key_input,
        set_default_button: MouseStateHandle::default(),
        remove_button: MouseStateHandle::default(),
        kind_buttons: [
            MouseStateHandle::default(),
            MouseStateHandle::default(),
            MouseStateHandle::default(),
            MouseStateHandle::default(),
        ],
    }
}

/// Set the text content of a SubmittableTextInput's underlying editor.
/// Internally drives `editor.set_buffer_text`. Best-effort — silently
/// no-ops if the view is no longer alive (shouldn't happen in normal
/// use but tolerated to keep refresh cheap).
fn set_input_text(
    input: &ViewHandle<SubmittableTextInput>,
    text: &str,
    ctx: &mut ViewContext<OmwAgentPageView>,
) {
    let editor = input.as_ref(ctx).editor().clone();
    editor.update(ctx, |ed, ctx| {
        ed.set_buffer_text(text, ctx);
    });
}

impl Entity for OmwAgentPageView {
    type Event = SettingsPageEvent;
}

impl View for OmwAgentPageView {
    fn ui_name() -> &'static str {
        "OmwAgentPage"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        self.page.render(self, app)
    }
}

impl TypedActionView for OmwAgentPageView {
    type Action = OmwAgentPageAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        // Route every typed action through the existing `dispatch`
        // method so the pure reducer + side-effecting `apply` keep their
        // single source of truth. `notify()` so the new state is
        // re-rendered on the next frame.
        let needs_buffer_refresh = matches!(
            action,
            OmwAgentPageAction::Discard
                | OmwAgentPageAction::AddProvider
                | OmwAgentPageAction::RemoveProvider(_)
                | OmwAgentPageAction::SetProviderId(_, _)
        );
        self.dispatch(action.clone());
        if needs_buffer_refresh {
            // Re-sync editor buffers from the (possibly mutated) form so
            // typed values keep showing the canonical text. The id case
            // is included because the reducer normalises ids when other
            // mutations happen (default-provider rename, etc.).
            self.refresh_editor_buffers(ctx);
        }
        ctx.notify();
    }
}

impl SettingsPageMeta for OmwAgentPageView {
    fn section() -> SettingsSection {
        SettingsSection::OmwAgent
    }

    fn should_render(&self, _ctx: &AppContext) -> bool {
        true
    }

    fn update_filter(&mut self, query: &str, ctx: &mut ViewContext<Self>) -> MatchData {
        self.page.update_filter(query, ctx)
    }

    fn scroll_to_widget(&mut self, widget_id: &'static str) {
        self.page.scroll_to_widget(widget_id)
    }

    fn clear_highlighted_widget(&mut self) {
        self.page.clear_highlighted_widget();
    }
}

impl From<ViewHandle<OmwAgentPageView>> for SettingsPageViewHandle {
    fn from(view_handle: ViewHandle<OmwAgentPageView>) -> Self {
        SettingsPageViewHandle::OmwAgent(view_handle)
    }
}

struct OmwAgentPageWidget;

impl SettingsWidget for OmwAgentPageWidget {
    type View = OmwAgentPageView;

    fn search_terms(&self) -> &str {
        "agent omw provider api key approval keychain config"
    }

    fn render(
        &self,
        view: &OmwAgentPageView,
        appearance: &Appearance,
        _app: &AppContext,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let active = theme.active_ui_text_color().into_solid();
        let muted = theme.nonactive_ui_text_color().into_solid();
        let form = &view.state.form;

        let mut col = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Start);

        // Agent enabled toggle. Click flips the flag through the same
        // ToggleEnabled reducer action exercised by the L3a tests.
        let agent_enabled_button = appearance
            .ui_builder()
            .button(
                if form.agent_enabled {
                    ButtonVariant::Accent
                } else {
                    ButtonVariant::Secondary
                },
                view.agent_enabled_button.clone(),
            )
            .with_text_label(
                if form.agent_enabled {
                    "Enabled".to_owned()
                } else {
                    "Disabled".to_owned()
                },
            )
            .build()
            .on_click(|ctx, _, _| {
                ctx.dispatch_typed_action(OmwAgentPageAction::ToggleEnabled);
            })
            .finish();
        let mut agent_enabled_row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center);
        agent_enabled_row.add_child(
            Container::new(
                Text::new(
                    "Agent enabled:".to_owned(),
                    appearance.ui_font_family(),
                    CONTENT_FONT_SIZE,
                )
                .with_color(active)
                .finish(),
            )
            .with_margin_right(8.)
            .finish(),
        );
        agent_enabled_row.add_child(agent_enabled_button);
        col.add_child(
            Container::new(agent_enabled_row.finish())
                .with_margin_bottom(8.)
                .finish(),
        );

        // Approval mode selector. Three buttons, one per ApprovalMode
        // variant; the currently-selected one renders Accent and the
        // others Secondary, matching the per-provider kind selector
        // below for visual consistency.
        let approval_modes = [
            (ApprovalMode::ReadOnly, "Read only", &view.approval_mode_buttons[0]),
            (
                ApprovalMode::AskBeforeWrite,
                "Ask before write",
                &view.approval_mode_buttons[1],
            ),
            (ApprovalMode::Trusted, "Trusted", &view.approval_mode_buttons[2]),
        ];
        let mut approval_row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center);
        approval_row.add_child(
            Container::new(
                Text::new(
                    "Approval mode:".to_owned(),
                    appearance.ui_font_family(),
                    CONTENT_FONT_SIZE,
                )
                .with_color(active)
                .finish(),
            )
            .with_margin_right(8.)
            .finish(),
        );
        for (mode, label, handle) in approval_modes {
            let selected = form.approval_mode == mode;
            let button = appearance
                .ui_builder()
                .button(
                    if selected {
                        ButtonVariant::Accent
                    } else {
                        ButtonVariant::Secondary
                    },
                    handle.clone(),
                )
                .with_text_label(label.to_owned())
                .build()
                .on_click(move |ctx, _, _| {
                    ctx.dispatch_typed_action(OmwAgentPageAction::SetApprovalMode(mode));
                })
                .finish();
            approval_row.add_child(
                Container::new(button).with_margin_right(4.).finish(),
            );
        }
        col.add_child(
            Container::new(approval_row.finish())
                .with_margin_bottom(8.)
                .finish(),
        );

        // Default provider dropdown selector. Replaces the prior
        // read-only label; click the trigger to expand a menu of
        // selectable rows + a "(none)" entry.
        let default_label = form
            .default_provider
            .as_deref()
            .unwrap_or("(none)")
            .to_string();
        let trigger_button = appearance
            .ui_builder()
            .button(
                ButtonVariant::Secondary,
                view.default_provider_trigger_button.clone(),
            )
            .with_text_label(format!("{default_label} \u{25BE}"))
            .build()
            .on_click(|ctx, _, _| {
                ctx.dispatch_typed_action(
                    OmwAgentPageAction::ToggleDefaultProviderDropdown,
                );
            })
            .finish();
        let mut default_row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center);
        default_row.add_child(
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
        default_row.add_child(trigger_button);
        col.add_child(
            Container::new(default_row.finish())
                .with_margin_bottom(4.)
                .finish(),
        );

        if view.state.default_provider_dropdown.is_expanded {
            let mut menu = Flex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Start);
            // (none) item — clears default.
            let is_none_active = form.default_provider.is_none();
            let none_item = appearance
                .ui_builder()
                .button(
                    if is_none_active {
                        ButtonVariant::Accent
                    } else {
                        ButtonVariant::Secondary
                    },
                    view.default_provider_none_item_button.clone(),
                )
                .with_text_label("(none)".to_owned())
                .build()
                .on_click(|ctx, _, _| {
                    ctx.dispatch_typed_action(
                        OmwAgentPageAction::SetDefaultProviderById(None),
                    );
                    ctx.dispatch_typed_action(
                        OmwAgentPageAction::CloseDefaultProviderDropdown,
                    );
                })
                .finish();
            menu.add_child(
                Container::new(none_item).with_margin_bottom(2.).finish(),
            );
            // Provider items.
            for (idx, prow) in form.providers.iter().enumerate() {
                if idx >= view.default_provider_item_buttons.len() {
                    break;
                }
                let is_active =
                    form.default_provider.as_deref() == Some(prow.id.as_str());
                let item_button = appearance
                    .ui_builder()
                    .button(
                        if is_active {
                            ButtonVariant::Accent
                        } else {
                            ButtonVariant::Secondary
                        },
                        view.default_provider_item_buttons[idx].clone(),
                    )
                    .with_text_label(prow.id.clone())
                    .build()
                    .on_click({
                        let id = prow.id.clone();
                        move |ctx, _, _| {
                            ctx.dispatch_typed_action(
                                OmwAgentPageAction::SetDefaultProviderById(Some(
                                    id.clone(),
                                )),
                            );
                            ctx.dispatch_typed_action(
                                OmwAgentPageAction::CloseDefaultProviderDropdown,
                            );
                        }
                    })
                    .finish();
                menu.add_child(
                    Container::new(item_button)
                        .with_margin_bottom(2.)
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
            // Reserve the same trailing margin even when collapsed so the
            // page below doesn't jump as the dropdown opens/closes.
            col.add_child(
                Container::new(Flex::column().finish())
                    .with_margin_bottom(12.)
                    .finish(),
            );
        }

        // providers list.
        col.add_child(
            Container::new(
                Text::new(
                    "Providers".to_owned(),
                    appearance.ui_font_family(),
                    CONTENT_FONT_SIZE,
                )
                .with_color(active)
                .finish(),
            )
            .with_margin_bottom(4.)
            .finish(),
        );
        if form.providers.is_empty() {
            col.add_child(
                Container::new(
                    Text::new(
                        "(no providers configured)".to_owned(),
                        appearance.ui_font_family(),
                        CONTENT_FONT_SIZE,
                    )
                    .with_color(muted)
                    .finish(),
                )
                .with_margin_bottom(8.)
                .finish(),
            );
        } else {
            // Per-provider editable rows. Each row exposes id / kind /
            // model / base_url / api_key inputs + Set Default + Remove
            // buttons. Editor handles are pre-allocated up to
            // `MAX_PROVIDER_SLOTS`; we render only the slot indices that
            // the form currently uses.
            for (idx, row) in form.providers.iter().enumerate() {
                if idx >= view.provider_editors.len() {
                    // More providers than slots — fall back to a hint.
                    col.add_child(
                        Container::new(
                            Text::new(
                                format!(
                                    "(provider #{idx} ‘{}’ exceeds editor slot capacity; edit ~/.config/omw/config.toml)",
                                    row.id
                                ),
                                appearance.ui_font_family(),
                                CONTENT_FONT_SIZE,
                            )
                            .with_color(muted)
                            .finish(),
                        )
                        .with_margin_bottom(8.)
                        .finish(),
                    );
                    continue;
                }
                let editors = &view.provider_editors[idx];
                let is_default = form.default_provider.as_deref() == Some(row.id.as_str());

                let kind_str = match row.kind {
                    ProviderKindForm::OpenAi => "openai",
                    ProviderKindForm::Anthropic => "anthropic",
                    ProviderKindForm::OpenAiCompatible => "openai-compat",
                    ProviderKindForm::Ollama => "ollama",
                };
                // Header line: id summary + default marker.
                col.add_child(
                    Container::new(
                        Text::new(
                            format!(
                                "Provider #{idx}: {} [{}]{}",
                                row.id,
                                kind_str,
                                if is_default { " ★ default" } else { "" }
                            ),
                            appearance.ui_font_family(),
                            CONTENT_FONT_SIZE,
                        )
                        .with_color(active)
                        .finish(),
                    )
                    .with_margin_bottom(4.)
                    .finish(),
                );

                // Editable inputs. Each is a SubmittableTextInput; the
                // user types and presses Enter to commit. The label
                // above the input shows what field it controls.
                let labeled_input =
                    |label: &str, input: &ViewHandle<SubmittableTextInput>| -> Box<dyn Element> {
                        let label_el = Container::new(
                            Text::new(
                                label.to_owned(),
                                appearance.ui_font_family(),
                                CONTENT_FONT_SIZE,
                            )
                            .with_color(muted)
                            .finish(),
                        )
                        .with_margin_top(4.)
                        .finish();
                        let input_el = Container::new(ChildView::new(input).finish())
                            .with_margin_bottom(2.)
                            .finish();
                        Flex::column()
                            .with_cross_axis_alignment(CrossAxisAlignment::Start)
                            .with_child(label_el)
                            .with_child(input_el)
                            .finish()
                    };
                col.add_child(labeled_input("    id (press Enter to apply):", &editors.id_input));
                col.add_child(labeled_input("    model:", &editors.model_input));
                col.add_child(labeled_input(
                    "    base_url (optional override):",
                    &editors.base_url_input,
                ));
                col.add_child(labeled_input(
                    "    api key (will be saved to keychain on Apply):",
                    &editors.api_key_input,
                ));

                // Per-row action buttons: Set Default, Remove,
                // and a kind selector row.
                let kinds_with_labels = [
                    (ProviderKindForm::OpenAi, "openai", &editors.kind_buttons[0]),
                    (ProviderKindForm::Anthropic, "anthropic", &editors.kind_buttons[1]),
                    (
                        ProviderKindForm::OpenAiCompatible,
                        "openai-compatible",
                        &editors.kind_buttons[2],
                    ),
                    (ProviderKindForm::Ollama, "ollama", &editors.kind_buttons[3]),
                ];
                let mut kind_row = Flex::row()
                    .with_cross_axis_alignment(CrossAxisAlignment::Center);
                kind_row.add_child(
                    Container::new(
                        Text::new(
                            "    kind:".to_owned(),
                            appearance.ui_font_family(),
                            CONTENT_FONT_SIZE,
                        )
                        .with_color(muted)
                        .finish(),
                    )
                    .with_margin_right(6.)
                    .finish(),
                );
                for (kind, label, handle) in kinds_with_labels {
                    let selected = row.kind == kind;
                    let button = appearance
                        .ui_builder()
                        .button(
                            if selected {
                                ButtonVariant::Accent
                            } else {
                                ButtonVariant::Secondary
                            },
                            handle.clone(),
                        )
                        .with_text_label(label.to_owned())
                        .build()
                        .on_click(move |ctx, _, _| {
                            ctx.dispatch_typed_action(OmwAgentPageAction::SetProviderKind(
                                idx, kind,
                            ));
                        })
                        .finish();
                    kind_row.add_child(
                        Container::new(button).with_margin_right(4.).finish(),
                    );
                }
                col.add_child(Container::new(kind_row.finish()).with_margin_top(4.).finish());

                // Set Default + Remove.
                let mut action_row = Flex::row()
                    .with_main_axis_alignment(MainAxisAlignment::Start)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center);
                let default_button = appearance
                    .ui_builder()
                    .button(
                        if is_default {
                            ButtonVariant::Accent
                        } else {
                            ButtonVariant::Secondary
                        },
                        editors.set_default_button.clone(),
                    )
                    .with_text_label(if is_default {
                        "★ default".to_owned()
                    } else {
                        "Set default".to_owned()
                    })
                    .build()
                    .on_click({
                        let id = row.id.clone();
                        move |ctx, _, _| {
                            ctx.dispatch_typed_action(
                                OmwAgentPageAction::SetDefaultProviderById(Some(id.clone())),
                            );
                        }
                    })
                    .finish();
                action_row.add_child(
                    Container::new(default_button).with_margin_right(6.).finish(),
                );
                let remove_button = appearance
                    .ui_builder()
                    .button(ButtonVariant::Secondary, editors.remove_button.clone())
                    .with_text_label("Remove".to_owned())
                    .build()
                    .on_click(move |ctx, _, _| {
                        ctx.dispatch_typed_action(OmwAgentPageAction::RemoveProvider(idx));
                    })
                    .finish();
                action_row.add_child(remove_button);
                col.add_child(
                    Container::new(action_row.finish())
                        .with_margin_top(4.)
                        .with_margin_bottom(12.)
                        .finish(),
                );
            }
        }

        // Add Provider button.
        let add_button = appearance
            .ui_builder()
            .button(ButtonVariant::Secondary, view.add_provider_button.clone())
            .with_text_label("+ Add provider".to_owned())
            .build()
            .on_click(|ctx, _, _| {
                ctx.dispatch_typed_action(OmwAgentPageAction::AddProvider);
            })
            .finish();
        col.add_child(Container::new(add_button).with_margin_bottom(12.).finish());

        // dirty-state + error indicator.
        col.add_child(
            Container::new(
                Text::new(
                    if view.state.is_dirty {
                        "Unsaved changes".to_owned()
                    } else {
                        "All saved".to_owned()
                    },
                    appearance.ui_font_family(),
                    CONTENT_FONT_SIZE,
                )
                .with_color(muted)
                .finish(),
            )
            .with_margin_top(12.)
            .with_margin_bottom(4.)
            .finish(),
        );
        if let Some(err) = &view.state.last_save_error {
            col.add_child(
                Container::new(
                    Text::new(
                        format!("Error: {err}"),
                        appearance.ui_font_family(),
                        CONTENT_FONT_SIZE,
                    )
                    .with_color(theme.foreground().into_solid())
                    .finish(),
                )
                .with_margin_bottom(8.)
                .finish(),
            );
        }

        // Apply / Discard buttons. on_click closures dispatch typed actions
        // that `OmwAgentPageView::handle_action` routes back into the
        // existing `dispatch` / `apply` path.
        let apply_button = appearance
            .ui_builder()
            .button(ButtonVariant::Accent, view.apply_button.clone())
            .with_text_label("Apply".to_owned())
            .build()
            .on_click(|ctx, _, _| {
                ctx.dispatch_typed_action(OmwAgentPageAction::Apply);
            })
            .finish();
        let discard_button = appearance
            .ui_builder()
            .button(ButtonVariant::Text, view.discard_button.clone())
            .with_text_label("Discard".to_owned())
            .build()
            .on_click(|ctx, _, _| {
                ctx.dispatch_typed_action(OmwAgentPageAction::Discard);
            })
            .finish();

        let buttons = Flex::row()
            .with_child(Container::new(apply_button).with_margin_right(8.).finish())
            .with_child(discard_button);
        col.add_child(Container::new(buttons.finish()).with_margin_top(8.).finish());

        col.finish()
    }
}
