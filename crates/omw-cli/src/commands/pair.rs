//! `omw pair {qr,list,revoke}` — Phase F.
//!
//! See `crates/omw-cli/tests/pair_qr.rs`, `pair_list.rs`, `pair_revoke.rs`
//! for the public-API contract. Executor fills in `unimplemented!()` bodies.

use std::io::Write;

use clap::{Args, ValueEnum};

#[derive(Copy, Clone, Debug, ValueEnum)]
pub(crate) enum QrOutputArg {
    Terminal,
    Png,
    Svg,
}

#[derive(Args, Debug)]
pub struct QrArgs {
    /// TTL for the pair token in minutes (default: 10).
    #[arg(long, default_value_t = 10)]
    pub ttl: u64,
    /// QR code output format. Default: terminal-rendered ASCII.
    #[arg(long, value_enum, default_value_t = QrOutputArg::Terminal)]
    pub out: QrOutputArg,
    /// Override the base URL. Default: `https://127.0.0.1:8787`.
    #[arg(long)]
    pub base_url: Option<String>,
}

#[derive(Args, Debug)]
pub struct RevokeArgs {
    /// Device id to revoke.
    pub device_id: String,
}

pub(crate) fn qr(
    _args: QrArgs,
    _stdout: &mut dyn Write,
    _stderr: &mut dyn Write,
) -> anyhow::Result<()> {
    unimplemented!("Phase F: omw pair qr")
}

pub(crate) fn list(_stdout: &mut dyn Write, _stderr: &mut dyn Write) -> anyhow::Result<()> {
    unimplemented!("Phase F: omw pair list")
}

pub(crate) fn revoke(
    _args: RevokeArgs,
    _stdout: &mut dyn Write,
    _stderr: &mut dyn Write,
) -> anyhow::Result<()> {
    unimplemented!("Phase F: omw pair revoke")
}
