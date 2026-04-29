//! `omw-cli` — `omw` umbrella CLI.
//!
//! v0.1 surface: `omw provider {list,add,remove}` and `omw config {path,show}`.
//! See `crates/omw-cli/tests/README.md` for the public-API contract.

use std::io::Write;

use clap::{Parser, Subcommand, ValueEnum};

mod commands;

/// Library entry point. `args` does NOT include argv[0]. Returns the exit
/// code the binary wrapper would `exit()` with. Never touches process stdio
/// directly.
pub fn run(args: &[String], stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    // Clap's `try_parse_from` expects argv-with-program-name. Prepend a
    // placeholder so usage/error strings read correctly.
    let mut argv: Vec<String> = Vec::with_capacity(args.len() + 1);
    argv.push("omw".to_string());
    argv.extend(args.iter().cloned());

    let cli = match Cli::try_parse_from(argv) {
        Ok(cli) => cli,
        Err(e) => {
            // Clap renders help/version on stdout; errors on stderr. We route
            // both to the appropriate sink we were given. Use `e.exit_code()`
            // so `--help` exits 0 and parse failures exit non-zero.
            let rendered = e.render().to_string();
            let code = e.exit_code();
            let _ = if code == 0 {
                stdout.write_all(rendered.as_bytes())
            } else {
                stderr.write_all(rendered.as_bytes())
            };
            return code;
        }
    };

    let result = match cli.command {
        Command::Provider(p) => match p.command {
            ProviderCommand::List => commands::provider::list(stdout, stderr),
            ProviderCommand::Add(args) => commands::provider::add(args, stdout, stderr),
            ProviderCommand::Remove(args) => commands::provider::remove(args, stdout, stderr),
        },
        Command::Config(c) => match c.command {
            ConfigCommand::Path => commands::config::path(stdout, stderr),
            ConfigCommand::Show => commands::config::show(stdout, stderr),
        },
    };

    match result {
        Ok(()) => 0,
        Err(e) => {
            let _ = writeln!(stderr, "error: {e:#}");
            1
        }
    }
}

#[derive(Parser, Debug)]
#[command(
    name = "omw",
    about = "omw umbrella CLI",
    disable_help_subcommand = true
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Manage provider entries
    Provider(ProviderArgs),
    /// Inspect omw configuration
    Config(ConfigArgs),
}

#[derive(clap::Args, Debug)]
struct ProviderArgs {
    #[command(subcommand)]
    command: ProviderCommand,
}

#[derive(Subcommand, Debug)]
enum ProviderCommand {
    /// List configured providers
    List,
    /// Add a provider entry to the config and store its key in the keychain
    Add(commands::provider::AddArgs),
    /// Remove a provider entry from config and clear its keychain entry
    Remove(commands::provider::RemoveArgs),
}

#[derive(clap::Args, Debug)]
struct ConfigArgs {
    #[command(subcommand)]
    command: ConfigCommand,
}

#[derive(Subcommand, Debug)]
enum ConfigCommand {
    /// Print the resolved config path
    Path,
    /// Print the current config contents (without secret values)
    Show,
}

/// Provider kinds exposed at the CLI surface. Mirrors `ProviderConfig`'s
/// `serde(rename = ...)` variants — kebab-case as on the wire.
#[derive(Copy, Clone, Debug, ValueEnum)]
pub(crate) enum ProviderKindArg {
    Openai,
    Anthropic,
    #[value(name = "openai-compatible")]
    OpenaiCompatible,
    Ollama,
}

impl ProviderKindArg {
    pub(crate) fn as_kebab(self) -> &'static str {
        match self {
            Self::Openai => "openai",
            Self::Anthropic => "anthropic",
            Self::OpenaiCompatible => "openai-compatible",
            Self::Ollama => "ollama",
        }
    }
}
