//! `omw provider {list,add,remove}` implementations.
//!
//! Invariant I-1 (no plaintext keys leak): the `--key` value is captured
//! only into local bindings that we never write to stdout/stderr. The on-disk
//! representation is always a `keychain:` ref produced from the provider id.

use std::io::{Read, Write};
use std::path::Path;
use std::str::FromStr;

use anyhow::{anyhow, bail, Context};
use clap::Args;
use toml_edit::{value, DocumentMut, Item, Table};

use omw_config::{config_path, BaseUrl, Config, KeyRef, ProviderConfig, ProviderId};
use omw_keychain::KeychainError;

use crate::ProviderKindArg;

// ---------------- list ----------------

pub(crate) fn list(stdout: &mut dyn Write, _stderr: &mut dyn Write) -> anyhow::Result<()> {
    let path = config_path()?;
    let cfg = Config::load_from(&path)
        .with_context(|| format!("loading config from {}", path.display()))?;

    if cfg.providers.is_empty() {
        writeln!(stdout, "Configured providers:")?;
        writeln!(stdout, "  (no providers configured)")?;
        return Ok(());
    }

    writeln!(stdout, "Configured providers:")?;
    let default_id = cfg
        .default_provider
        .as_ref()
        .map(|p| p.as_str().to_string());
    for (id, prov) in cfg.providers.iter() {
        let kind = kind_of(prov);
        let default_model = default_model_of(prov).unwrap_or("(unset)");
        let key_status = key_status(prov);
        let is_default = default_id.as_deref() == Some(id.as_str());
        let suffix = if is_default { " (default)" } else { "" };
        writeln!(
            stdout,
            "  {id} (kind={kind}) default-model={default_model} key={key_status}{suffix}",
            id = id.as_str(),
            kind = kind,
            default_model = default_model,
            key_status = key_status,
            suffix = suffix
        )?;
    }
    Ok(())
}

fn kind_of(p: &ProviderConfig) -> &'static str {
    match p {
        ProviderConfig::OpenAi { .. } => "openai",
        ProviderConfig::Anthropic { .. } => "anthropic",
        ProviderConfig::OpenAiCompatible { .. } => "openai-compatible",
        ProviderConfig::Ollama { .. } => "ollama",
    }
}

fn default_model_of(p: &ProviderConfig) -> Option<&str> {
    match p {
        ProviderConfig::OpenAi { default_model, .. }
        | ProviderConfig::Anthropic { default_model, .. }
        | ProviderConfig::OpenAiCompatible { default_model, .. }
        | ProviderConfig::Ollama { default_model, .. } => default_model.as_deref(),
    }
}

fn key_ref_of(p: &ProviderConfig) -> Option<&KeyRef> {
    match p {
        ProviderConfig::OpenAi { key_ref, .. }
        | ProviderConfig::Anthropic { key_ref, .. }
        | ProviderConfig::OpenAiCompatible { key_ref, .. } => Some(key_ref),
        ProviderConfig::Ollama { key_ref, .. } => key_ref.as_ref(),
    }
}

fn key_status(p: &ProviderConfig) -> &'static str {
    match (p, key_ref_of(p)) {
        (ProviderConfig::Ollama { .. }, None) => "(no key)",
        (_, None) => "(no key)",
        (_, Some(kr)) => match omw_keychain::get(kr) {
            Ok(_) => "stored",
            Err(KeychainError::NotFound) => "missing",
            Err(_) => "missing",
        },
    }
}

// ---------------- add ----------------

#[derive(Args, Debug)]
pub struct AddArgs {
    /// Provider id (`[A-Za-z0-9_-]+`).
    pub id: String,
    /// Provider kind. Required in non-interactive mode.
    #[arg(long)]
    pub kind: Option<ProviderKindArg>,
    /// API key (mutually exclusive with `--from-stdin`).
    #[arg(long, conflicts_with = "from_stdin")]
    pub key: Option<String>,
    /// Read the API key from a single line on stdin.
    #[arg(long)]
    pub from_stdin: bool,
    /// Base URL (required for `openai-compatible`).
    #[arg(long)]
    pub base_url: Option<String>,
    /// Default model for this provider.
    #[arg(long)]
    pub default_model: Option<String>,
    /// Don't prompt; rely entirely on flags.
    #[arg(long)]
    pub non_interactive: bool,
    /// Overwrite an existing provider with this id.
    #[arg(long)]
    pub force: bool,
    /// Set this provider as the new `default_provider`.
    #[arg(long)]
    pub make_default: bool,
}

pub(crate) fn add(
    args: AddArgs,
    stdout: &mut dyn Write,
    _stderr: &mut dyn Write,
) -> anyhow::Result<()> {
    // Step 1: validate id (cheap, do first to fail before any IO).
    let pid = ProviderId::from_str(&args.id)
        .map_err(|e| anyhow!("invalid provider id `{}`: {e}", args.id))?;

    // Step 2: kind is required in non-interactive (every test passes it).
    let kind = match args.kind {
        Some(k) => k,
        None => {
            if args.non_interactive {
                bail!("--kind is required in --non-interactive mode");
            }
            bail!("interactive `provider add` is not implemented in v0.1; pass --non-interactive and --kind");
        }
    };

    // Step 3: openai-compatible requires --base-url. Validate before touching
    // disk or the keychain.
    let base_url_str = args.base_url.as_deref();
    if matches!(kind, ProviderKindArg::OpenaiCompatible) && base_url_str.is_none() {
        bail!("--base-url is required for kind=openai-compatible");
    }
    let parsed_base_url = match base_url_str {
        Some(s) => {
            Some(BaseUrl::from_str(s).map_err(|e| anyhow!("invalid --base-url `{s}`: {e}"))?)
        }
        None => None,
    };
    // Ollama default base_url is http://127.0.0.1:11434 if none provided —
    // but only when running interactively per the spec; the tests pass an
    // explicit --base-url for ollama. For non-interactive we only persist what
    // the user gave us. (Loader treats Ollama base_url as Optional anyway.)

    // Step 4: load the config doc with toml_edit (preserves comments). If the
    // file is missing, start from a fresh doc with `version = 1`.
    let cfg_path = config_path()?;
    let mut doc = read_doc_or_empty(&cfg_path)?;

    // Step 5: existence check — fail fast before any keychain mutation.
    let providers_already_has_id = providers_table(&doc)
        .map(|t| t.contains_key(pid.as_str()))
        .unwrap_or(false);
    if providers_already_has_id && !args.force {
        bail!(
            "provider `{}` already exists; pass --force to overwrite",
            pid.as_str()
        );
    }

    // Step 6: collect the secret. Bind tightly; never written to stdio.
    let key_value: Option<String> = if args.from_stdin {
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        let line = buf.trim_end_matches(['\n', '\r']).to_string();
        Some(line)
    } else {
        args.key.clone()
    };

    let key_required = !matches!(kind, ProviderKindArg::Ollama);
    if key_required && key_value.is_none() {
        if args.non_interactive {
            bail!("--key (or --from-stdin) is required in --non-interactive mode");
        }
        bail!("interactive key prompt is not implemented in v0.1; pass --key or --from-stdin");
    }

    // Step 7: write the secret to the keychain (only when we have one).
    let key_ref_for_id = KeyRef::Keychain {
        name: format!("omw/{}", pid.as_str()),
    };
    if let Some(secret) = &key_value {
        // overwrite is fine — `set` semantics already replace.
        omw_keychain::set(&key_ref_for_id, secret).context("storing key in keychain")?;
    }

    // Step 8: ensure top-level `version = 1`.
    if !doc.as_table().contains_key("version") {
        doc["version"] = value(1_i64);
    }

    // Step 9: build the new provider table and install it.
    let mut new_table = Table::new();
    new_table["kind"] = value(kind.as_kebab());
    if key_value.is_some() {
        new_table["key_ref"] = value(key_ref_for_id.to_string());
    }
    if let Some(b) = parsed_base_url.as_ref() {
        new_table["base_url"] = value(b.as_str());
    }
    if let Some(model) = args.default_model.as_ref() {
        new_table["default_model"] = value(model.as_str());
    }
    new_table.set_implicit(false);

    let providers = ensure_providers_table(&mut doc);
    providers.insert(pid.as_str(), Item::Table(new_table));

    // Step 10: handle default_provider.
    let had_default_provider = doc
        .as_table()
        .get("default_provider")
        .and_then(|i| i.as_str())
        .is_some();
    let providers_count = providers_table(&doc).map(|t| t.len()).unwrap_or(0);
    let auto_default = args.non_interactive && !had_default_provider && providers_count == 1;
    if args.make_default || auto_default {
        doc["default_provider"] = value(pid.as_str());
    }

    // Step 11: write back atomically-ish. Create parent dir if needed.
    write_doc(&cfg_path, &doc)?;

    // Step 12: confirmation. Use the kebab kind name; never echo the secret.
    writeln!(
        stdout,
        "Added provider `{}` (kind={})",
        pid.as_str(),
        kind.as_kebab()
    )?;
    Ok(())
}

// ---------------- remove ----------------

#[derive(Args, Debug)]
pub struct RemoveArgs {
    /// Provider id to remove.
    pub id: String,
    /// Skip the interactive confirmation prompt.
    #[arg(long)]
    pub yes: bool,
}

pub(crate) fn remove(
    args: RemoveArgs,
    stdout: &mut dyn Write,
    _stderr: &mut dyn Write,
) -> anyhow::Result<()> {
    if !args.yes {
        bail!("refusing to remove without --yes");
    }
    let pid = ProviderId::from_str(&args.id)
        .map_err(|e| anyhow!("invalid provider id `{}`: {e}", args.id))?;

    let cfg_path = config_path()?;
    let mut doc = match read_doc_or_empty_existing(&cfg_path)? {
        Some(d) => d,
        None => bail!(
            "provider `{}` is not configured (no config file)",
            pid.as_str()
        ),
    };

    let exists = providers_table(&doc)
        .map(|t| t.contains_key(pid.as_str()))
        .unwrap_or(false);
    if !exists {
        bail!("provider `{}` is not configured", pid.as_str());
    }

    // Remove the [providers.<id>] table.
    if let Some(providers) = providers_table_mut(&mut doc) {
        providers.remove(pid.as_str());
    }

    // Clear default_provider if it pointed at us.
    let cleared_default = doc
        .as_table()
        .get("default_provider")
        .and_then(|i| i.as_str())
        .map(|s| s == pid.as_str())
        .unwrap_or(false);
    if cleared_default {
        doc.as_table_mut().remove("default_provider");
    }

    // Remove the keychain entry; NotFound is benign.
    let kr = KeyRef::Keychain {
        name: format!("omw/{}", pid.as_str()),
    };
    match omw_keychain::delete(&kr) {
        Ok(()) | Err(KeychainError::NotFound) => {}
        Err(e) => return Err(anyhow!("clearing keychain entry: {e}")),
    }

    write_doc(&cfg_path, &doc)?;

    writeln!(stdout, "Removed provider `{}`", pid.as_str())?;
    Ok(())
}

// ---------------- helpers ----------------

fn providers_table(doc: &DocumentMut) -> Option<&Table> {
    doc.as_table().get("providers").and_then(|i| i.as_table())
}

fn providers_table_mut(doc: &mut DocumentMut) -> Option<&mut Table> {
    doc.as_table_mut()
        .get_mut("providers")
        .and_then(|i| i.as_table_mut())
}

fn ensure_providers_table(doc: &mut DocumentMut) -> &mut Table {
    if !doc.as_table().contains_key("providers") {
        let mut t = Table::new();
        t.set_implicit(true);
        doc.insert("providers", Item::Table(t));
    }
    let item = doc
        .as_table_mut()
        .get_mut("providers")
        .expect("just inserted");
    let table = item
        .as_table_mut()
        .expect("`providers` should be a TOML table");
    table.set_implicit(true);
    table
}

fn read_doc_or_empty(path: &Path) -> anyhow::Result<DocumentMut> {
    match std::fs::read_to_string(path) {
        Ok(s) => s
            .parse::<DocumentMut>()
            .with_context(|| format!("parsing config at {}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(DocumentMut::new()),
        Err(e) => Err(anyhow!("reading config at {}: {e}", path.display())),
    }
}

fn read_doc_or_empty_existing(path: &Path) -> anyhow::Result<Option<DocumentMut>> {
    match std::fs::read_to_string(path) {
        Ok(s) => {
            Ok(Some(s.parse::<DocumentMut>().with_context(|| {
                format!("parsing config at {}", path.display())
            })?))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(anyhow!("reading config at {}: {e}", path.display())),
    }
}

fn write_doc(path: &Path, doc: &DocumentMut) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating parent dir {}", parent.display()))?;
        }
    }
    std::fs::write(path, doc.to_string())
        .with_context(|| format!("writing config at {}", path.display()))?;
    Ok(())
}
