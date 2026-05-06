//! `omw-config` — TOML configuration loader, schema validator, and file
//! watcher for omw.
//!
//! See [PRD §8.2](../../../PRD.md#82-components) for component scope and
//! [`specs/threat-model.md`](../../../specs/threat-model.md) §3.4–§3.5 for the
//! invariants this crate enforces (most importantly **I-1**: no plaintext keys
//! on disk).
//!
//! # Example
//!
//! ```no_run
//! use omw_config::Config;
//!
//! let cfg = Config::load()?;
//! // load() already validates; calling again is a no-op equivalent.
//! cfg.validate()?;
//! # Ok::<(), omw_config::ConfigError>(())
//! ```

mod error;
mod key_ref;
mod schema;
mod watcher;
mod writer;

pub use error::{ConfigError, ValidationError, ValidationIssue};
pub use key_ref::{KeyRef, KeyRefParseError};
pub use schema::{
    AgentConfig, ApprovalConfig, ApprovalMode, BaseUrl, BaseUrlParseError, Config,
    ProviderConfig, ProviderId, ProviderIdParseError, SchemaVersion,
};
pub use watcher::{watch, ConfigUpdate, WatchHandle};
pub use writer::save_atomic;

use std::path::{Path, PathBuf};

/// Resolve the path to omw's TOML config file.
///
/// Resolution order: `OMW_CONFIG` env var (if non-empty) → `XDG_CONFIG_HOME` /
/// `omw/config.toml` → `$HOME/.config/omw/config.toml` (`USERPROFILE` on
/// Windows). The PRD specifies the XDG path explicitly even on macOS, so this
/// does not use the OS "Application Support" directory.
pub fn config_path() -> Result<PathBuf, ConfigError> {
    if let Some(p) = std::env::var_os("OMW_CONFIG") {
        if !p.is_empty() {
            return Ok(PathBuf::from(p));
        }
    }
    let base = if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            PathBuf::from(xdg)
        } else {
            home_dir()?.join(".config")
        }
    } else {
        home_dir()?.join(".config")
    };
    Ok(base.join("omw").join("config.toml"))
}

fn home_dir() -> Result<PathBuf, ConfigError> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| ConfigError::PathResolution("neither HOME nor USERPROFILE is set".into()))
}

impl Config {
    /// Load + validate from the default config path. A missing file returns
    /// `Ok(Config::default())` — a fresh install isn't an error. Any other I/O
    /// or parse error surfaces.
    pub fn load() -> Result<Self, ConfigError> {
        Self::load_from(&config_path()?)
    }

    /// Load + validate from an explicit path. Same missing-file rule as
    /// [`Config::load`].
    pub fn load_from(path: &Path) -> Result<Self, ConfigError> {
        let raw = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(source) => {
                return Err(ConfigError::Io {
                    path: path.to_path_buf(),
                    source,
                })
            }
        };
        let cfg: Config = toml::from_str(&raw).map_err(|source| ConfigError::Parse {
            path: path.to_path_buf(),
            source: Box::new(source),
        })?;
        cfg.validate()?;
        Ok(cfg)
    }

    /// Cross-field validation. Per-field correctness (URL schemes, key-ref
    /// shape, provider-id grammar, required-fields-per-variant) is enforced
    /// by serde at parse time; this method covers checks that span fields.
    pub fn validate(&self) -> Result<(), ValidationError> {
        let mut issues = Vec::new();
        if let Some(default) = &self.default_provider {
            if !self.providers.contains_key(default) {
                issues.push(ValidationIssue {
                    field_path: "default_provider".into(),
                    message: format!(
                        "references provider id `{default}` which is not configured under [providers.*]"
                    ),
                });
            }
        }
        if issues.is_empty() {
            Ok(())
        } else {
            Err(ValidationError { issues })
        }
    }
}

impl Config {
    /// Save this config to `path` via the round-trip writer. See [`save_atomic`].
    pub fn save_atomic(&self, path: &std::path::Path) -> Result<(), ConfigError> {
        crate::writer::save_atomic(path, self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn missing_file_yields_default_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("does-not-exist.toml");
        let cfg = Config::load_from(&path).unwrap();
        assert_eq!(cfg, Config::default());
    }

    #[test]
    fn malformed_toml_returns_parse_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.toml");
        std::fs::write(&path, "this is = =not valid toml").unwrap();
        let err = Config::load_from(&path).unwrap_err();
        assert!(matches!(err, ConfigError::Parse { .. }), "got: {err:?}");
    }

    #[test]
    fn validates_default_provider_exists() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ghost.toml");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(
            f,
            r#"default_provider = "ghost"

[providers.real]
kind = "ollama"
"#
        )
        .unwrap();
        drop(f);
        let err = Config::load_from(&path).unwrap_err();
        match err {
            ConfigError::Validation(v) => {
                assert_eq!(v.issues.len(), 1);
                assert_eq!(v.issues[0].field_path, "default_provider");
                assert!(v.issues[0].message.contains("ghost"));
            }
            other => panic!("expected validation error, got {other:?}"),
        }
    }

    #[test]
    fn validates_default_provider_when_present() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ok.toml");
        std::fs::write(
            &path,
            r#"default_provider = "ollama-local"

[providers.ollama-local]
kind = "ollama"
"#,
        )
        .unwrap();
        Config::load_from(&path).expect("valid config should load");
    }

    /// Single env-var-resolution test — `std::env` is process-global, so all
    /// env-touching assertions are serialised inside one test.
    #[test]
    fn config_path_resolution() {
        let restore_omw = std::env::var_os("OMW_CONFIG");
        let restore_xdg = std::env::var_os("XDG_CONFIG_HOME");

        // OMW_CONFIG takes precedence over everything.
        std::env::set_var("OMW_CONFIG", "/custom/path/config.toml");
        assert_eq!(
            config_path().unwrap(),
            std::path::PathBuf::from("/custom/path/config.toml")
        );

        // XDG_CONFIG_HOME used when OMW_CONFIG is unset.
        std::env::remove_var("OMW_CONFIG");
        std::env::set_var("XDG_CONFIG_HOME", "/tmp/xdg-config");
        assert_eq!(
            config_path().unwrap(),
            std::path::PathBuf::from("/tmp/xdg-config/omw/config.toml")
        );

        // Empty OMW_CONFIG is treated as unset.
        std::env::set_var("OMW_CONFIG", "");
        assert_eq!(
            config_path().unwrap(),
            std::path::PathBuf::from("/tmp/xdg-config/omw/config.toml")
        );

        match restore_omw {
            Some(v) => std::env::set_var("OMW_CONFIG", v),
            None => std::env::remove_var("OMW_CONFIG"),
        }
        match restore_xdg {
            Some(v) => std::env::set_var("XDG_CONFIG_HOME", v),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
    }
}
