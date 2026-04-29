//! `omw ask <prompt>` — spawn `omw-agent ask <prompt> [flags]` and forward
//! stdio + exit code.
//!
//! Thin wrapper around `agent_runner::run_one_turn`. All provider HTTP,
//! keychain resolution, and usage telemetry live in the TS half
//! (`apps/omw-agent/src/cli.ts`); the Rust half locates the agent binary,
//! spawns it, streams stdio, and best-effort persists usage.

use std::io::Write;

use clap::Args;

use crate::commands::agent_runner::{self, AgentOpts};

#[derive(Args, Debug)]
pub struct AskArgs {
    /// Prompt to send to the model.
    pub prompt: String,
    /// Provider id (overrides `default_provider`).
    #[arg(long)]
    pub provider: Option<String>,
    /// Model name (overrides the provider's `default_model`).
    #[arg(long)]
    pub model: Option<String>,
    /// Maximum tokens to generate.
    #[arg(long)]
    pub max_tokens: Option<u32>,
    /// Sampling temperature.
    #[arg(long)]
    pub temperature: Option<f32>,
}

/// Run the handler. Returns the exit code that the binary wrapper would
/// `exit()` with.
pub(crate) fn run(args: AskArgs, stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    let opts = AgentOpts {
        agent_bin: None,
        provider: args.provider,
        model: args.model,
        max_tokens: args.max_tokens,
        temperature: args.temperature.map(|t| t as f64),
        cwd: None,
    };
    agent_runner::run_one_turn(&args.prompt, &opts, stdout, stderr)
}
