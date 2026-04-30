//! `omw remote {start,status,stop}` — Phase F.
//!
//! See `crates/omw-cli/tests/remote_status.rs` and `remote_start_stop.rs`
//! for the public-API contract. Executor fills in `unimplemented!()` bodies.

use std::io::Write;

use clap::Args;

#[derive(Args, Debug)]
pub struct StartArgs {
    /// Address to bind. Default: `127.0.0.1:8787`.
    #[arg(long, default_value = "127.0.0.1:8787")]
    pub listen: String,
    /// Skip Tailscale wiring (default for v0.4-thin).
    #[arg(long, default_value_t = false)]
    pub no_tailscale: bool,
    /// Hidden test hook: when the named env var is set, the server shuts down.
    #[arg(long, hide = true)]
    pub shutdown_signal: Option<String>,
}

#[derive(Args, Debug)]
pub struct StopArgs {
    /// Also revoke every paired device (`devices.revoked_at = now()`).
    #[arg(long, default_value_t = false)]
    pub all: bool,
}

pub(crate) fn start(
    _args: StartArgs,
    _stdout: &mut dyn Write,
    _stderr: &mut dyn Write,
) -> anyhow::Result<()> {
    unimplemented!("Phase F: omw remote start")
}

pub(crate) fn status(_stdout: &mut dyn Write, _stderr: &mut dyn Write) -> anyhow::Result<()> {
    unimplemented!("Phase F: omw remote status")
}

pub(crate) fn stop(
    _args: StopArgs,
    _stdout: &mut dyn Write,
    _stderr: &mut dyn Write,
) -> anyhow::Result<()> {
    unimplemented!("Phase F: omw remote stop")
}
