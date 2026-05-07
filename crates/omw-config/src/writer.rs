//! Round-trip TOML writer using `toml_edit` to preserve user-authored
//! comments, key order, and unknown tables. See spec §1.2.

use std::path::Path;

use toml_edit::{value, DocumentMut, Item};

use crate::error::ConfigError;
use crate::schema::{ApprovalMode, Config, ProviderConfig};

/// Atomically save `cfg` to `path`, preserving user comments / unknown
/// tables on the existing file. Writes to `<path>.tmp` then renames. If
/// `path` does not exist, starts from an empty document.
pub fn save_atomic(path: &Path, cfg: &Config) -> Result<(), ConfigError> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|source| ConfigError::Io {
                path: path.to_path_buf(),
                source,
            })?;
        }
    }

    let existing = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(source) => {
            return Err(ConfigError::Io {
                path: path.to_path_buf(),
                source,
            })
        }
    };

    let mut doc: DocumentMut = if existing.trim().is_empty() {
        DocumentMut::new()
    } else {
        existing.parse().map_err(|source: toml_edit::TomlError| ConfigError::TomlEdit {
            path: path.to_path_buf(),
            source,
        })?
    };

    apply_managed_fields(&mut doc, cfg);

    let serialized = doc.to_string();

    let tmp = {
        let mut tmp_name = path.file_name()
            .ok_or_else(|| ConfigError::Io {
                path: path.to_path_buf(),
                source: std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "config path must have a file name",
                ),
            })?
            .to_os_string();
        tmp_name.push(".tmp");
        path.with_file_name(tmp_name)
    };
    std::fs::write(&tmp, serialized.as_bytes()).map_err(|source| ConfigError::Io {
        path: tmp.clone(),
        source,
    })?;
    std::fs::rename(&tmp, path).map_err(|source| ConfigError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(())
}

fn apply_managed_fields(doc: &mut DocumentMut, cfg: &Config) {
    // version
    doc["version"] = value(cfg.version.0 as i64);

    // default_provider
    match &cfg.default_provider {
        Some(id) => doc["default_provider"] = value(id.as_str()),
        None => {
            doc.remove("default_provider");
        }
    }

    // [approval]
    let approval = doc["approval"].or_insert(toml_edit::table());
    approval["mode"] = value(approval_mode_str(cfg.approval.mode));

    // [agent]
    let agent = doc["agent"].or_insert(toml_edit::table());
    agent["enabled"] = value(cfg.agent.enabled);

    // [providers]
    let providers = doc["providers"].or_insert(toml_edit::table());
    let providers_tbl = providers
        .as_table_mut()
        .expect("providers must be a table");
    providers_tbl.set_implicit(true);

    // Track which provider IDs we wrote so we can prune removed ones.
    let want_ids: std::collections::HashSet<String> = cfg
        .providers
        .keys()
        .map(|id| id.as_str().to_string())
        .collect();

    let existing_ids: Vec<String> = providers_tbl
        .iter()
        .map(|(k, _)| k.to_string())
        .collect();
    for id in existing_ids {
        if !want_ids.contains(&id) {
            providers_tbl.remove(&id);
        }
    }

    for (id, pcfg) in &cfg.providers {
        let entry = providers_tbl
            .entry(id.as_str())
            .or_insert(Item::Table(toml_edit::Table::new()));
        let table = entry.as_table_mut().expect("provider must be table");
        write_provider_into_table(pcfg, table);
    }
}

fn approval_mode_str(m: ApprovalMode) -> &'static str {
    match m {
        ApprovalMode::ReadOnly => "read_only",
        ApprovalMode::AskBeforeWrite => "ask_before_write",
        ApprovalMode::Trusted => "trusted",
    }
}

fn write_provider_into_table(pcfg: &ProviderConfig, table: &mut toml_edit::Table) {
    table["kind"] = value(pcfg.kind_str());
    match pcfg {
        ProviderConfig::OpenAi { key_ref, default_model, base_url } => {
            table["key_ref"] = value(key_ref.to_string());
            update_optional(table, "default_model", default_model.as_deref());
            update_optional(table, "base_url", base_url.as_ref().map(|u| u.as_str()));
        }
        ProviderConfig::Anthropic { key_ref, default_model } => {
            table["key_ref"] = value(key_ref.to_string());
            update_optional(table, "default_model", default_model.as_deref());
            table.remove("base_url");
        }
        ProviderConfig::OpenAiCompatible { key_ref, base_url, default_model } => {
            table["key_ref"] = value(key_ref.to_string());
            table["base_url"] = value(base_url.as_str());
            update_optional(table, "default_model", default_model.as_deref());
        }
        ProviderConfig::Ollama { base_url, key_ref, default_model } => {
            let key_ref_str = key_ref.as_ref().map(|k| k.to_string());
            update_optional(table, "key_ref", key_ref_str.as_deref());
            update_optional(table, "base_url", base_url.as_ref().map(|u| u.as_str()));
            update_optional(table, "default_model", default_model.as_deref());
        }
    }
}

fn update_optional(table: &mut toml_edit::Table, key: &str, val: Option<&str>) {
    match val {
        Some(s) => table[key] = value(s),
        None => {
            table.remove(key);
        }
    }
}
