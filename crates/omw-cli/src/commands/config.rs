//! `omw config {path,show}` implementations.

use std::io::Write;

use anyhow::Context;

use omw_config::{config_path, Config, ProviderConfig};

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
        writeln!(
            stdout,
            "  {} (kind={}) key={}",
            id.as_str(),
            prov.kind_str(),
            key_status(prov),
        )?;
    }
    Ok(())
}

fn key_status(p: &ProviderConfig) -> &'static str {
    match p.key_ref() {
        None => "(no key)",
        Some(kr) => match omw_keychain::get(kr) {
            Ok(_) => "stored",
            Err(_) => "missing",
        },
    }
}
