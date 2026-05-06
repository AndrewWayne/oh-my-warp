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
