//! TOML schema for `omw-config`.
//!
//! Per `specs/threat-model.md` §3.5, `BaseUrl` enforces an `http`/`https`
//! scheme check (the v0.1 user decision on §7's open question). Stricter
//! private-IP blocking would break the local-Ollama default and was deferred.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::str::FromStr;

use crate::key_ref::KeyRef;

/// Top-level config.
///
/// `#[serde(default)]` on the type lets an empty TOML file deserialize to
/// `Config::default()`. The top level intentionally does NOT use
/// `deny_unknown_fields` — v0.2 added `[approval]` and `[agent]` as
/// first-class typed blocks; `[routing]` and `[audit]` remain forward-compat
/// reservations. We want a binary to ignore unknown top-level tables
/// gracefully rather than refuse to start.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(default)]
pub struct Config {
    pub version: SchemaVersion,
    pub default_provider: Option<ProviderId>,
    pub providers: BTreeMap<ProviderId, ProviderConfig>,
    pub approval: ApprovalConfig,
    pub agent: AgentConfig,
}

// ---------------- SchemaVersion ----------------

/// Schema version. v0.1 = `1`. Reserving the field early so v0.2/v0.3 can
/// migrate cleanly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct SchemaVersion(pub u32);

impl Default for SchemaVersion {
    fn default() -> Self {
        Self(1)
    }
}

impl SchemaVersion {
    pub const SUPPORTED: &'static [u32] = &[1];
}

impl<'de> serde::Deserialize<'de> for SchemaVersion {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = u32::deserialize(d)?;
        if Self::SUPPORTED.contains(&v) {
            Ok(SchemaVersion(v))
        } else {
            Err(serde::de::Error::custom(format!(
                "unsupported config schema version `{v}`; supported: {:?}",
                Self::SUPPORTED
            )))
        }
    }
}

// ---------------- ProviderId ----------------

/// A user-chosen provider id. Validated as `[A-Za-z0-9_-]+` to avoid the
/// `[providers.foo.bar]` nested-table footgun in TOML.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProviderId(String);

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum ProviderIdParseError {
    #[error("provider id must not be empty")]
    Empty,
    #[error("provider id `{0}` contains invalid characters; allowed: A-Z a-z 0-9 _ -")]
    InvalidChars(String),
}

impl ProviderId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for ProviderId {
    type Err = ProviderIdParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() {
            return Err(ProviderIdParseError::Empty);
        }
        if !s
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err(ProviderIdParseError::InvalidChars(s.to_string()));
        }
        Ok(ProviderId(s.to_string()))
    }
}

impl TryFrom<String> for ProviderId {
    type Error = ProviderIdParseError;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse()
    }
}

impl std::fmt::Display for ProviderId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl serde::Serialize for ProviderId {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for ProviderId {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

// ---------------- BaseUrl ----------------

/// A base URL constrained to `http` or `https`. Per `specs/threat-model.md`
/// §3.5, this rejects `file://`, `data://`, `javascript://`, `ftp://`, etc.
/// Private IP literals are NOT blocked — Ollama runs on `127.0.0.1` and
/// blocking it by default would break the v0.1 zero-config Ollama UX.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaseUrl(url::Url);

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum BaseUrlParseError {
    #[error("invalid URL: {0}")]
    Invalid(String),
    #[error("unsupported URL scheme `{0}`; expected `http` or `https`")]
    UnsupportedScheme(String),
}

impl BaseUrl {
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
    pub fn into_url(self) -> url::Url {
        self.0
    }
    pub fn url(&self) -> &url::Url {
        &self.0
    }
}

impl FromStr for BaseUrl {
    type Err = BaseUrlParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let url = url::Url::parse(s).map_err(|e| BaseUrlParseError::Invalid(e.to_string()))?;
        match url.scheme() {
            "http" | "https" => Ok(BaseUrl(url)),
            other => Err(BaseUrlParseError::UnsupportedScheme(other.to_string())),
        }
    }
}

impl TryFrom<String> for BaseUrl {
    type Error = BaseUrlParseError;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse()
    }
}

impl std::fmt::Display for BaseUrl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0.as_str())
    }
}

impl serde::Serialize for BaseUrl {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(self.0.as_str())
    }
}

impl<'de> serde::Deserialize<'de> for BaseUrl {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

// ---------------- ProviderConfig ----------------

/// One configured provider. Internally tagged on `kind` — required fields per
/// variant are enforced by the type system, not a runtime validator.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum ProviderConfig {
    #[serde(rename = "openai")]
    OpenAi {
        key_ref: KeyRef,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default_model: Option<String>,
        /// Optional override for the OpenAI API endpoint. Lets users
        /// route through Azure OpenAI deployments, regional CDN
        /// fronts, or a local intercepting proxy without having to
        /// switch the provider variant to `openai-compatible`. When
        /// `None` (the default), pi-ai uses `https://api.openai.com/v1`.
        /// The kernel reads this in `session.ts::buildModel` and
        /// passes it to `manualOpenAICompletions` for unknown model
        /// ids — registered models in pi-ai's registry still use
        /// their hard-coded endpoint.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        base_url: Option<BaseUrl>,
    },
    #[serde(rename = "anthropic")]
    Anthropic {
        key_ref: KeyRef,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default_model: Option<String>,
    },
    #[serde(rename = "openai-compatible")]
    OpenAiCompatible {
        key_ref: KeyRef,
        base_url: BaseUrl,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default_model: Option<String>,
    },
    #[serde(rename = "ollama")]
    Ollama {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        base_url: Option<BaseUrl>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        key_ref: Option<KeyRef>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default_model: Option<String>,
    },
}

impl ProviderConfig {
    /// The kebab-case discriminator string ("openai", "anthropic",
    /// "openai-compatible", "ollama") matching the `kind = ...` TOML field.
    pub fn kind_str(&self) -> &'static str {
        match self {
            Self::OpenAi { .. } => "openai",
            Self::Anthropic { .. } => "anthropic",
            Self::OpenAiCompatible { .. } => "openai-compatible",
            Self::Ollama { .. } => "ollama",
        }
    }

    /// Borrow the configured `KeyRef` if present. `None` for Ollama with no
    /// `key_ref` field (Ollama is the only kind where the key is optional).
    pub fn key_ref(&self) -> Option<&KeyRef> {
        match self {
            Self::OpenAi { key_ref, .. }
            | Self::Anthropic { key_ref, .. }
            | Self::OpenAiCompatible { key_ref, .. } => Some(key_ref),
            Self::Ollama { key_ref, .. } => key_ref.as_ref(),
        }
    }

    /// Borrow the `default_model` field if set.
    pub fn default_model(&self) -> Option<&str> {
        match self {
            Self::OpenAi { default_model, .. }
            | Self::Anthropic { default_model, .. }
            | Self::OpenAiCompatible { default_model, .. }
            | Self::Ollama { default_model, .. } => default_model.as_deref(),
        }
    }
}

// ---------------- Approval ----------------

/// Mirrors `omw_policy::ApprovalMode` and `apps/omw-agent/src/policy.ts:11`.
/// The snake_case wire form is what the kernel sees in `session/create.policy.mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalMode {
    ReadOnly,
    AskBeforeWrite,
    Trusted,
}

impl Default for ApprovalMode {
    fn default() -> Self {
        Self::AskBeforeWrite
    }
}

/// `[approval]` block. Reserved as a forward-compat block in v0.1; first-class in v0.2.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ApprovalConfig {
    pub mode: ApprovalMode,
}

// ---------------- Agent ----------------

/// `[agent]` block. Master enable/disable for the inline agent panel.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct AgentConfig {
    pub enabled: bool,
    /// Path to a user-managed AGENTS.md. When set and readable, its
    /// contents are copied to the canonical AGENTS.md location on every
    /// session create (see `crate::sync_agents_md`). Empty / unset →
    /// the canonical file is used as-is (or no system prompt if that
    /// file is also missing).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agents_md_path: Option<PathBuf>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            agents_md_path: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pid(s: &str) -> ProviderId {
        s.parse().unwrap()
    }

    // -------- ProviderId --------

    #[test]
    fn provider_id_accepts_alphanum_and_dash_underscore() {
        assert!("openai-prod".parse::<ProviderId>().is_ok());
        assert!("oz_local".parse::<ProviderId>().is_ok());
        assert!("a1".parse::<ProviderId>().is_ok());
    }

    #[test]
    fn provider_id_rejects_dots() {
        // Otherwise [providers.foo.bar] becomes a nested table by accident.
        let err = "foo.bar".parse::<ProviderId>().unwrap_err();
        assert!(matches!(err, ProviderIdParseError::InvalidChars(_)));
    }

    #[test]
    fn provider_id_rejects_empty_and_whitespace() {
        assert!(matches!(
            "".parse::<ProviderId>().unwrap_err(),
            ProviderIdParseError::Empty
        ));
        assert!(matches!(
            "foo bar".parse::<ProviderId>().unwrap_err(),
            ProviderIdParseError::InvalidChars(_)
        ));
        assert!(matches!(
            "foo/bar".parse::<ProviderId>().unwrap_err(),
            ProviderIdParseError::InvalidChars(_)
        ));
    }

    // -------- BaseUrl --------

    #[test]
    fn base_url_accepts_http_and_https() {
        assert!("https://api.openai.com".parse::<BaseUrl>().is_ok());
        assert!("http://127.0.0.1:11434".parse::<BaseUrl>().is_ok());
        // Private IPs are intentionally allowed — Ollama runs on 127.0.0.1.
        assert!("http://10.0.0.5:8080".parse::<BaseUrl>().is_ok());
    }

    #[test]
    fn base_url_rejects_file_scheme() {
        let err = "file:///etc/passwd".parse::<BaseUrl>().unwrap_err();
        assert_eq!(err, BaseUrlParseError::UnsupportedScheme("file".into()));
    }

    #[test]
    fn base_url_rejects_data_scheme() {
        let err = "data:text/plain,hi".parse::<BaseUrl>().unwrap_err();
        assert_eq!(err, BaseUrlParseError::UnsupportedScheme("data".into()));
    }

    #[test]
    fn base_url_rejects_javascript_scheme() {
        let err = "javascript:alert(1)".parse::<BaseUrl>().unwrap_err();
        assert_eq!(
            err,
            BaseUrlParseError::UnsupportedScheme("javascript".into())
        );
    }

    #[test]
    fn base_url_rejects_ftp_scheme() {
        let err = "ftp://example.com".parse::<BaseUrl>().unwrap_err();
        assert_eq!(err, BaseUrlParseError::UnsupportedScheme("ftp".into()));
    }

    #[test]
    fn base_url_rejects_garbage() {
        let err = "not a url".parse::<BaseUrl>().unwrap_err();
        assert!(matches!(err, BaseUrlParseError::Invalid(_)));
    }

    // -------- SchemaVersion --------

    #[test]
    fn schema_version_default_is_one() {
        assert_eq!(SchemaVersion::default(), SchemaVersion(1));
    }

    #[test]
    fn schema_version_rejects_unknown() {
        let r: Result<SchemaVersion, _> = toml::from_str("999");
        assert!(r.is_err());
    }

    // -------- Config round-trip per variant --------

    #[test]
    fn round_trips_openai_provider() {
        let toml = r#"
default_provider = "openai-prod"

[providers.openai-prod]
kind = "openai"
key_ref = "keychain:omw/openai"
default_model = "gpt-4o"
"#;
        let cfg: Config = toml::from_str(toml).unwrap();
        let serialized = toml::to_string(&cfg).unwrap();
        let round: Config = toml::from_str(&serialized).unwrap();
        assert_eq!(cfg, round);
        assert_eq!(cfg.default_provider, Some(pid("openai-prod")));
    }

    #[test]
    fn round_trips_anthropic_provider() {
        let toml = r#"
[providers.anthro]
kind = "anthropic"
key_ref = "keychain:omw/anthropic"
default_model = "claude-sonnet-4-6"
"#;
        let cfg: Config = toml::from_str(toml).unwrap();
        let round: Config = toml::from_str(&toml::to_string(&cfg).unwrap()).unwrap();
        assert_eq!(cfg, round);
    }

    #[test]
    fn round_trips_openai_compatible_provider() {
        let toml = r#"
[providers.azure]
kind = "openai-compatible"
key_ref = "keychain:omw/azure"
base_url = "https://my-resource.openai.azure.com/"
default_model = "gpt-4o"
"#;
        let cfg: Config = toml::from_str(toml).unwrap();
        let round: Config = toml::from_str(&toml::to_string(&cfg).unwrap()).unwrap();
        assert_eq!(cfg, round);
    }

    #[test]
    fn round_trips_ollama_with_minimal_fields() {
        let toml = r#"
[providers.ollama-local]
kind = "ollama"
"#;
        let cfg: Config = toml::from_str(toml).unwrap();
        let round: Config = toml::from_str(&toml::to_string(&cfg).unwrap()).unwrap();
        assert_eq!(cfg, round);
    }

    #[test]
    fn round_trips_ollama_with_all_fields() {
        let toml = r#"
[providers.ollama-local]
kind = "ollama"
base_url = "http://127.0.0.1:11434/"
key_ref = "keychain:omw/ollama"
default_model = "llama3.1:8b"
"#;
        let cfg: Config = toml::from_str(toml).unwrap();
        let round: Config = toml::from_str(&toml::to_string(&cfg).unwrap()).unwrap();
        assert_eq!(cfg, round);
    }

    // -------- Schema correctness --------

    #[test]
    fn empty_toml_yields_default_config() {
        let cfg: Config = toml::from_str("").unwrap();
        assert_eq!(cfg, Config::default());
        assert_eq!(cfg.version, SchemaVersion(1));
    }

    #[test]
    fn missing_kind_is_rejected() {
        let toml = r#"
[providers.x]
key_ref = "keychain:omw/x"
"#;
        assert!(toml::from_str::<Config>(toml).is_err());
    }

    #[test]
    fn unknown_kind_is_rejected() {
        let toml = r#"
[providers.x]
kind = "fictional"
"#;
        assert!(toml::from_str::<Config>(toml).is_err());
    }

    #[test]
    fn openai_compatible_without_base_url_is_rejected_structurally() {
        let toml = r#"
[providers.x]
kind = "openai-compatible"
key_ref = "keychain:omw/x"
"#;
        // base_url is required by the enum variant; serde rejects, no
        // post-validation needed.
        assert!(toml::from_str::<Config>(toml).is_err());
    }

    #[test]
    fn openai_without_key_ref_is_rejected_structurally() {
        let toml = r#"
[providers.x]
kind = "openai"
"#;
        assert!(toml::from_str::<Config>(toml).is_err());
    }

    #[test]
    fn deny_unknown_fields_inside_variant() {
        let toml = r#"
[providers.x]
kind = "openai"
key_ref = "keychain:omw/x"
mystery = "field"
"#;
        // deny_unknown_fields on the enum applies per-variant.
        assert!(toml::from_str::<Config>(toml).is_err());
    }

    #[test]
    fn unknown_top_level_table_is_tolerated_for_forward_compat() {
        // v0.2 added [approval] and [agent] (now first-class typed). [routing]
        // remains a forward-compat reservation. A binary must not crash on
        // unknown top-level tables.
        let toml = r#"
version = 1

[providers.openai-prod]
kind = "openai"
key_ref = "keychain:omw/openai"

[routing]
default = "openai-prod"

[approval]
mode = "ask_before_write"
"#;
        let cfg: Config = toml::from_str(toml).expect("unknown top-level tables must be tolerated");
        assert!(cfg.providers.contains_key(&pid("openai-prod")));
    }

    #[test]
    fn rejects_plaintext_key_in_provider_block_i1() {
        // I-1 enforced by KeyRef's deserializer; the error bubbles up here.
        let toml = r#"
[providers.openai-prod]
kind = "openai"
key_ref = "sk-prod-secret"
"#;
        let err = toml::from_str::<Config>(toml).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("scheme") || msg.contains("keychain"),
            "expected KeyRef scheme rejection, got: {msg}"
        );
    }

    #[test]
    fn rejects_file_scheme_base_url_in_provider_block() {
        let toml = r#"
[providers.evil]
kind = "openai-compatible"
key_ref = "keychain:omw/evil"
base_url = "file:///etc/passwd"
"#;
        let err = toml::from_str::<Config>(toml).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("scheme"),
            "expected scheme rejection, got: {msg}"
        );
    }

    #[test]
    fn approval_mode_default_is_ask_before_write() {
        assert_eq!(ApprovalMode::default(), ApprovalMode::AskBeforeWrite);
    }

    #[test]
    fn approval_mode_serializes_kebab_case() {
        let v = serde_json::to_string(&ApprovalMode::AskBeforeWrite).unwrap();
        assert_eq!(v, "\"ask_before_write\"");
        let r: ApprovalMode = serde_json::from_str("\"read_only\"").unwrap();
        assert_eq!(r, ApprovalMode::ReadOnly);
        let t: ApprovalMode = serde_json::from_str("\"trusted\"").unwrap();
        assert_eq!(t, ApprovalMode::Trusted);
    }

    #[test]
    fn approval_block_round_trips() {
        let toml = r#"
[approval]
mode = "trusted"
"#;
        let cfg: Config = toml::from_str(toml).unwrap();
        assert_eq!(cfg.approval.mode, ApprovalMode::Trusted);
        let s = toml::to_string(&cfg).unwrap();
        let round: Config = toml::from_str(&s).unwrap();
        assert_eq!(round.approval.mode, ApprovalMode::Trusted);
    }

    #[test]
    fn approval_block_default_when_missing() {
        let cfg: Config = toml::from_str("").unwrap();
        assert_eq!(cfg.approval.mode, ApprovalMode::AskBeforeWrite);
    }

    #[test]
    fn agent_block_default_enabled_true() {
        let cfg: Config = toml::from_str("").unwrap();
        assert!(cfg.agent.enabled);
    }

    #[test]
    fn agent_block_round_trips() {
        let toml = r#"
[agent]
enabled = false
"#;
        let cfg: Config = toml::from_str(toml).unwrap();
        assert!(!cfg.agent.enabled);
        let s = toml::to_string(&cfg).unwrap();
        let round: Config = toml::from_str(&s).unwrap();
        assert!(!round.agent.enabled);
    }

    #[test]
    fn agent_block_round_trips_agents_md_path() {
        let toml = r#"
[agent]
enabled = true
agents_md_path = "/Users/me/dotfiles/AGENTS.md"
"#;
        let cfg: Config = toml::from_str(toml).unwrap();
        assert_eq!(
            cfg.agent.agents_md_path,
            Some(PathBuf::from("/Users/me/dotfiles/AGENTS.md"))
        );
        let s = toml::to_string(&cfg).unwrap();
        let round: Config = toml::from_str(&s).unwrap();
        assert_eq!(round.agent.agents_md_path, cfg.agent.agents_md_path);
    }

    #[test]
    fn agents_md_path_omitted_when_unset() {
        let cfg = Config::default();
        let s = toml::to_string(&cfg).unwrap();
        assert!(
            !s.contains("agents_md_path"),
            "default config must not serialize agents_md_path key"
        );
    }
}
