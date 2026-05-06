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

#[derive(Debug, Clone)]
pub struct OmwAgentPageState {
    pub saved_config: Config,
    pub form: OmwAgentForm,
    pub pending_secrets: BTreeMap<String, String>,
    pub is_dirty: bool,
    pub last_save_error: Option<String>,
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
    SetDefault(usize),
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
        _ => None,
    }
}

pub fn validate_form(form: &OmwAgentForm) -> Result<(), Vec<FormError>> {
    let mut errs = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for row in &form.providers {
        if ProviderId::from_str(&row.id).is_err() {
            errs.push(FormError::InvalidProviderId(row.id.clone()));
            continue;
        }
        if !seen.insert(row.id.clone()) {
            errs.push(FormError::DuplicateProviderId(row.id.clone()));
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
            ProviderKindForm::OpenAi | ProviderKindForm::Anthropic => {
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
        if !row.key_ref_token.is_empty() && KeyRef::from_str(&row.key_ref_token).is_err() {
            errs.push(FormError::KeyRefInvalid(row.id.clone()));
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

pub fn form_to_config(
    form: &OmwAgentForm,
    persisted_secrets: &BTreeMap<String, KeyRef>,
) -> Result<Config, Vec<FormError>> {
    validate_form(form)?;

    let mut providers = BTreeMap::new();
    for row in &form.providers {
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
                row.kind = k;
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
        OmwAgentPageAction::SetDefault(idx) => {
            if let Some(row) = state.form.providers.get(idx) {
                state.form.default_provider = Some(row.id.clone());
            }
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
use warpui::{
    elements::{
        Container, CrossAxisAlignment, Element, Flex, MouseStateHandle, ParentElement, Text,
    },
    ui_components::{
        button::ButtonVariant,
        components::UiComponent,
    },
    AppContext, Entity, View, ViewContext, ViewHandle,
};

use super::settings_page::{
    MatchData, PageType, SettingsPageEvent, SettingsPageMeta, SettingsPageViewHandle,
    SettingsWidget, CONTENT_FONT_SIZE,
};
use super::SettingsSection;

/// Action enum for clicks dispatched from the rendered page. Kept separate
/// from `OmwAgentPageAction` so the click handlers can carry extra context
/// if needed; for now they map 1:1 to dispatch calls.
#[derive(Clone, Debug, PartialEq)]
pub enum OmwAgentPageViewAction {
    Apply,
    Discard,
}

/// View struct held by the page. Owns the form state plus mouse-state handles
/// for the Apply/Discard buttons. Click dispatch wiring is minimal in this
/// commit; Task 7 exercises `dispatch` directly.
pub struct OmwAgentPageView {
    pub state: OmwAgentPageState,
    pub apply_button: MouseStateHandle,
    pub discard_button: MouseStateHandle,
    page: PageType<Self>,
}

impl OmwAgentPageView {
    pub fn new(_ctx: &mut ViewContext<Self>) -> Self {
        Self::new_inner()
    }

    /// App-context-free constructor. Used by integration tests in
    /// `app/tests/` to mount the view without a full `warpui::App`.
    pub fn new_inner() -> Self {
        let cfg = omw_config::Config::load().unwrap_or_default();
        let form = form_from_config(&cfg);
        Self {
            state: OmwAgentPageState {
                saved_config: cfg,
                form,
                pending_secrets: BTreeMap::new(),
                is_dirty: false,
                last_save_error: None,
            },
            apply_button: MouseStateHandle::default(),
            discard_button: MouseStateHandle::default(),
            page: PageType::new_monolith(OmwAgentPageWidget, Some("Agent"), false),
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
    }
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

        // agent_enabled flag.
        col.add_child(
            Container::new(
                Text::new(
                    format!("Agent enabled: {}", form.agent_enabled),
                    appearance.ui_font_family(),
                    CONTENT_FONT_SIZE,
                )
                .with_color(active)
                .finish(),
            )
            .with_margin_bottom(8.)
            .finish(),
        );

        // approval mode.
        col.add_child(
            Container::new(
                Text::new(
                    format!("Approval mode: {:?}", form.approval_mode),
                    appearance.ui_font_family(),
                    CONTENT_FONT_SIZE,
                )
                .with_color(active)
                .finish(),
            )
            .with_margin_bottom(8.)
            .finish(),
        );

        // default provider.
        col.add_child(
            Container::new(
                Text::new(
                    format!(
                        "Default provider: {}",
                        form.default_provider.as_deref().unwrap_or("(none)")
                    ),
                    appearance.ui_font_family(),
                    CONTENT_FONT_SIZE,
                )
                .with_color(active)
                .finish(),
            )
            .with_margin_bottom(12.)
            .finish(),
        );

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
            for row in &form.providers {
                let kind_str = match row.kind {
                    ProviderKindForm::OpenAi => "openai",
                    ProviderKindForm::Anthropic => "anthropic",
                    ProviderKindForm::OpenAiCompatible => "openai-compat",
                    ProviderKindForm::Ollama => "ollama",
                };
                let model = if row.model.is_empty() {
                    "(default)"
                } else {
                    row.model.as_str()
                };
                col.add_child(
                    Container::new(
                        Text::new(
                            format!("- {} [{}] model={}", row.id, kind_str, model),
                            appearance.ui_font_family(),
                            CONTENT_FONT_SIZE,
                        )
                        .with_color(active)
                        .finish(),
                    )
                    .with_margin_bottom(4.)
                    .finish(),
                );
            }
        }

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

        // Apply / Discard buttons. Click handlers are intentionally noop for
        // now — Task 7's tests construct the view directly and call
        // `dispatch`/`apply`. Wiring full action dispatch through
        // `SettingsAction` is deferred per the plan.
        let apply_button = appearance
            .ui_builder()
            .button(ButtonVariant::Accent, view.apply_button.clone())
            .with_text_label("Apply".to_owned())
            .build()
            .finish();
        let discard_button = appearance
            .ui_builder()
            .button(ButtonVariant::Text, view.discard_button.clone())
            .with_text_label("Discard".to_owned())
            .build()
            .finish();

        let buttons = Flex::row()
            .with_child(Container::new(apply_button).with_margin_right(8.).finish())
            .with_child(discard_button);
        col.add_child(Container::new(buttons.finish()).with_margin_top(8.).finish());

        col.finish()
    }
}
