//! `omw agent` — line-oriented REPL that spawns `omw-agent ask <line>`
//! per non-empty / non-meta line.
//!
//! Reads stdin via a locked `BufRead`; for each line:
//!   - empty → continue (silent skip).
//!   - `/exit` or `/quit` → break, exit 0.
//!   - otherwise → `agent_runner::run_one_turn`, continue regardless of
//!     the per-turn return code (failed turns do NOT terminate the REPL).
//!
//! EOF → break, exit 0.

use std::io::{BufRead, Write};
use std::path::PathBuf;

use clap::Args;

use crate::commands::agent_runner::{self, AgentOpts};

#[derive(Args, Debug)]
pub struct AgentArgs {
    /// Working directory used when spawning the agent for each turn.
    /// This flag is parent-side only and is NOT forwarded into the child.
    #[arg(long)]
    pub cwd: Option<PathBuf>,
    /// Provider id (overrides `default_provider`). Propagated to every turn.
    #[arg(long)]
    pub provider: Option<String>,
    /// Model name (overrides the provider's `default_model`). Propagated
    /// to every turn.
    #[arg(long)]
    pub model: Option<String>,
}

/// Run the REPL. Returns the exit code (always 0 in the documented happy
/// paths: EOF, `/exit`, `/quit`).
pub(crate) fn run(args: AgentArgs, stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    let opts = AgentOpts {
        agent_bin: None,
        provider: args.provider,
        model: args.model,
        max_tokens: None,
        temperature: None,
        cwd: args.cwd,
    };

    let stdin = std::io::stdin();
    let handle = stdin.lock();
    for line in handle.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed == "/exit" || trimmed == "/quit" {
            break;
        }
        let _ = agent_runner::run_one_turn(trimmed, &opts, stdout, stderr);
    }
    0
}
