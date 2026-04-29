//! `omw config {path,show}` implementations.

use std::io::Write;

use anyhow::Context;

use omw_config::{config_path, Config, ProviderConfig};
use omw_keychain::KeychainError;

pub(crate) fn path(stdout: &mut dyn Write, _stderr: &mut dyn Write) -> anyhow::Result<()> {
    let p = config_path()?;
    writeln!(stdout, "{}", p.display())?;
    Ok(())
}

pub(crate) fn show(stdout: &mut dyn Write, _stderr: &mut dyn Write) -> anyhow::Result<()> {
    let p = config_path()?;
    let cfg =
        Config::load_from(&p).with_context(|| format!("loading config from {}", p.display()))?;
    writeln!(stdout, "version = 1")?;
    if let Some(default) = &cfg.default_provider {
        writeln!(stdout, "default_provider = {}", default.as_str())?;
    }
    if cfg.providers.is_empty() {
        writeln!(stdout, "(no providers configured)")?;
        return Ok(());
    }
    writeln!(stdout, "providers:")?;
    for (id, prov) in cfg.providers.iter() {
        let kind = match prov {
            ProviderConfig::OpenAi { .. } => "openai",
            ProviderConfig::Anthropic { .. } => "anthropic",
            ProviderConfig::OpenAiCompatible { .. } => "openai-compatible",
            ProviderConfig::Ollama { .. } => "ollama",
        };
        let key_status = key_status(prov);
        writeln!(
            stdout,
            "  {} (kind={}) key={}",
            id.as_str(),
            kind,
            key_status
        )?;
    }
    Ok(())
}

fn key_status(p: &ProviderConfig) -> &'static str {
    let kr = match p {
        ProviderConfig::OpenAi { key_ref, .. }
        | ProviderConfig::Anthropic { key_ref, .. }
        | ProviderConfig::OpenAiCompatible { key_ref, .. } => Some(key_ref),
        ProviderConfig::Ollama { key_ref, .. } => key_ref.as_ref(),
    };
    match kr {
        None => "(no key)",
        Some(kr) => match omw_keychain::get(kr) {
            Ok(_) => "stored",
            Err(KeychainError::NotFound) => "missing",
            Err(_) => "missing",
        },
    }
}
