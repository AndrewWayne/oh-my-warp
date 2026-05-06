//! Error types for `omw-config`.
//!
//! `ValidationError` carries structured `ValidationIssue` records (field path +
//! message) so future tooling such as `omw config check` can render them without
//! re-parsing strings.

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("could not resolve config path: {0}")]
    PathResolution(String),

    #[error("failed to read config at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to parse TOML at {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: Box<toml::de::Error>,
    },

    #[error(transparent)]
    Validation(#[from] ValidationError),

    #[error("watcher error: {0}")]
    Watcher(Box<notify::Error>),

    #[error("could not parse {path:?} with toml_edit: {source}")]
    TomlEdit {
        path: std::path::PathBuf,
        #[source]
        source: toml_edit::TomlError,
    },
}

impl From<notify::Error> for ConfigError {
    fn from(e: notify::Error) -> Self {
        ConfigError::Watcher(Box::new(e))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationIssue {
    pub field_path: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    pub issues: Vec<ValidationIssue>,
}

impl ValidationError {
    pub fn single(field_path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            issues: vec![ValidationIssue {
                field_path: field_path.into(),
                message: message.into(),
            }],
        }
    }
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "config validation failed with {} issue(s):",
            self.issues.len()
        )?;
        for issue in &self.issues {
            writeln!(f, "  - {}: {}", issue.field_path, issue.message)?;
        }
        Ok(())
    }
}

impl std::error::Error for ValidationError {}
