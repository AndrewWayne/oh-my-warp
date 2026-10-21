use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::{
    fs,
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};
use toml_edit::{value, Array, DocumentMut};

#[derive(ClapArgs, Debug)]
pub(crate) struct Args {
    /// Preview the title-only change without modifying Codex configuration
    #[arg(long)]
    dry_run: bool,
}

fn config_path() -> Result<PathBuf> {
    let home = std::env::var_os("CODEX_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".codex")))
        .context("neither CODEX_HOME nor HOME is set")?;
    Ok(home.join("config.toml"))
}

fn title_configuration(original: &str) -> Result<Option<String>> {
    let mut document = original
        .parse::<DocumentMut>()
        .map_err(|_| anyhow::anyhow!("invalid Codex TOML configuration"))?;
    if document
        .get("tui")
        .is_some_and(|tui| tui.as_table_like().is_none())
    {
        bail!("Codex tui configuration must be a table");
    }
    let items = match document
        .get("tui")
        .and_then(|tui| tui.get("terminal_title"))
    {
        Some(item) => item
            .as_array()
            .context("Codex terminal_title must be an array")?
            .iter()
            .map(|item| {
                item.as_str()
                    .map(str::to_owned)
                    .context("Codex title items must be strings")
            })
            .collect::<Result<Vec<_>>>()?,
        None => vec![
            "activity".into(),
            "thread-name".into(),
            "project-name".into(),
        ],
    };
    if items
        .first()
        .is_some_and(|item| item == "session-id" || item == "thread-id")
        && items.get(1).is_some_and(|item| item == "app-name")
    {
        return Ok(None);
    }
    let mut title = Array::new();
    title.push("session-id");
    title.push("app-name");
    for item in items {
        title.push(item);
    }
    document["tui"]["terminal_title"] = value(title);
    Ok(Some(document.to_string()))
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn enable_at(path: &Path, dry_run: bool, output: &mut dyn Write) -> Result<()> {
    // Preserve an existing config symlink and edit its target atomically.
    let path = match fs::symlink_metadata(path) {
        Ok(_) => fs::canonicalize(path).context("cannot resolve existing Codex configuration")?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => path.to_owned(),
        Err(error) => return Err(error.into()),
    };
    let original = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.into()),
    };
    let Some(updated) = title_configuration(&original)? else {
        writeln!(output, "Codex native thread titles are already enabled.")?;
        return Ok(());
    };
    writeln!(output, "Config: {}", path.display())?;
    if dry_run {
        writeln!(
            output,
            "Preview: enable native thread titles; retain existing title items and other settings."
        )?;
        return Ok(());
    }
    fs::create_dir_all(path.parent().context("configuration path has no parent")?)?;
    let stamp = format!(
        "{}-{}",
        std::process::id(),
        chrono::Utc::now()
            .timestamp_nanos_opt()
            .context("invalid timestamp")?
    );
    if path.exists() {
        let backup = path.with_extension(format!("toml.omw-links-{stamp}.bak"));
        write_private(&backup, original.as_bytes())?;
        writeln!(output, "Backup: {}", backup.display())?;
    }
    let temporary = path.with_extension(format!("toml.omw-links-{stamp}.tmp"));
    let result = (|| {
        write_private(&temporary, updated.as_bytes())?;
        let current = fs::read_to_string(&path).or_else(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                Ok(String::new())
            } else {
                Err(error)
            }
        })?;
        if current != original {
            bail!("Codex configuration changed during setup; retry");
        }
        fs::rename(&temporary, &path)?;
        writeln!(
            output,
            "Enabled for new Codex launches. oh my warp hides the added title prefix."
        )?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub(crate) fn enable(args: Args, output: &mut dyn Write) -> Result<()> {
    enable_at(&config_path()?, args.dry_run, output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn dangling_configuration_symlink_is_preserved_without_writes() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("missing.toml");
        let path = directory.path().join("config.toml");
        std::os::unix::fs::symlink(&target, &path).unwrap();
        assert!(enable_at(&path, false, &mut Vec::new()).is_err());
        assert_eq!(fs::read_link(&path).unwrap(), target);
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn setup_preserves_settings_and_symlink_and_is_idempotent() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("native.toml");
        let path = directory.path().join("config.toml");
        let original = "# retain comment\nmodel = 'test-model' # keep\n[features]\nworktrees = true\n[tui]\nnotifications = false\nterminal_title = ['git-branch', 'model']\n";
        fs::write(&target, original).unwrap();
        std::os::unix::fs::symlink(&target, &path).unwrap();
        enable_at(&path, false, &mut Vec::new()).unwrap();
        let updated = fs::read_to_string(&target).unwrap();
        assert!(updated.contains("model = 'test-model' # keep"));
        assert!(updated.contains("notifications = false"));
        assert!(updated.contains("worktrees = true"));
        assert!(fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_symlink());
        let parsed: DocumentMut = updated.parse().unwrap();
        let items: Vec<_> = parsed["tui"]["terminal_title"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item.as_str().unwrap())
            .collect();
        assert_eq!(items, ["session-id", "app-name", "git-branch", "model"]);
        let backup = fs::read_dir(directory.path())
            .unwrap()
            .filter_map(|entry| entry.ok())
            .find(|entry| entry.path().extension().is_some_and(|ext| ext == "bak"))
            .unwrap()
            .path();
        assert_eq!(fs::read_to_string(&backup).unwrap(), original);
        assert_eq!(
            fs::metadata(&backup).unwrap().permissions().mode() & 0o777,
            0o600
        );
        enable_at(&path, false, &mut Vec::new()).unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), updated);
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 3);
    }

    #[test]
    fn preview_and_invalid_configuration_do_not_write() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        enable_at(&path, true, &mut Vec::new()).unwrap();
        assert!(!path.exists());
        for original in [
            "model = [",
            "tui = 1",
            "[tui]\nterminal_title = 1",
            "[tui]\nterminal_title = [42]",
        ] {
            fs::write(&path, original).unwrap();
            assert!(enable_at(&path, false, &mut Vec::new()).is_err());
            assert_eq!(fs::read_to_string(&path).unwrap(), original);
            assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
        }
        let secret = "example-private-value-that-must-not-be-printed";
        fs::write(&path, format!("api_key = \"{secret}")).unwrap();
        let error = enable_at(&path, false, &mut Vec::new()).unwrap_err();
        assert!(!format!("{error:#}").contains(secret));
    }
}
