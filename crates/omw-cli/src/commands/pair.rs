//! `omw pair {qr,list,revoke}` — Phase F.
//!
//! See `crates/omw-cli/tests/pair_qr.rs`, `pair_list.rs`, `pair_revoke.rs`
//! for the public-API contract.

use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use clap::{Args, ValueEnum};
use omw_remote::{open_db, HostKey, Pairings};
use qrcode::render::unicode::Dense1x2;
use qrcode::QrCode;

use crate::db::data_dir;

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

fn remote_db_path() -> Result<PathBuf> {
    Ok(data_dir()?.join("omw-remote.sqlite3"))
}

fn host_key_path() -> Result<PathBuf> {
    Ok(data_dir()?.join("host_key.bin"))
}

fn ensure_data_dir() -> Result<PathBuf> {
    let dir = data_dir()?;
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("creating data dir {}", dir.display()))?;
    Ok(dir)
}

pub(crate) fn qr(args: QrArgs, stdout: &mut dyn Write, _stderr: &mut dyn Write) -> Result<()> {
    ensure_data_dir()?;

    // Load (or create) the host key. Not strictly needed for token issue, but
    // the start path also wants it; ensuring it exists here is defensive and
    // matches what the spec implies for "first-run pair qr".
    let _ = HostKey::load_or_create(&host_key_path()?).with_context(|| "loading host key")?;

    let db_path = remote_db_path()?;
    let conn = open_db(&db_path)
        .with_context(|| format!("opening omw-remote db at {}", db_path.display()))?;
    let pairings = Pairings::new(conn);

    let token = pairings
        .issue(Duration::from_secs(args.ttl * 60))
        .map_err(|e| anyhow!("issue pair token: {e}"))?;

    let base_url = args
        .base_url
        .clone()
        .unwrap_or_else(|| "https://127.0.0.1:8787".to_string());
    let url = format!(
        "{}/pair?t={}",
        base_url.trim_end_matches('/'),
        token.to_base32()
    );

    writeln!(stdout, "{}", url)?;

    match args.out {
        QrOutputArg::Terminal => {
            let code = QrCode::new(url.as_bytes()).with_context(|| "encoding QR code")?;
            let rendered = code
                .render::<Dense1x2>()
                .dark_color(Dense1x2::Light)
                .light_color(Dense1x2::Dark)
                .build();
            writeln!(stdout, "{}", rendered)?;
        }
        QrOutputArg::Png | QrOutputArg::Svg => {
            // Not exercised by Phase F tests; fall through with a brief note.
            writeln!(stdout, "(QR rendering for png/svg not yet implemented; URL above is sufficient for pairing.)")?;
        }
    }

    Ok(())
}

pub(crate) fn list(stdout: &mut dyn Write, _stderr: &mut dyn Write) -> Result<()> {
    ensure_data_dir()?;
    let db_path = remote_db_path()?;
    let conn = open_db(&db_path)
        .with_context(|| format!("opening omw-remote db at {}", db_path.display()))?;

    let mut stmt = conn
        .prepare(
            "SELECT id, name, paired_at, last_seen, revoked_at \
             FROM devices ORDER BY paired_at DESC",
        )
        .with_context(|| "preparing devices query")?;

    let rows = stmt
        .query_map([], |row| {
            let id: String = row.get(0)?;
            let name: String = row.get(1)?;
            let paired_at: String = row.get(2)?;
            let last_seen: Option<String> = row.get(3)?;
            let revoked_at: Option<String> = row.get(4)?;
            Ok((id, name, paired_at, last_seen, revoked_at))
        })
        .with_context(|| "querying devices")?;

    let collected: Vec<_> = rows
        .collect::<rusqlite::Result<Vec<_>>>()
        .with_context(|| "reading devices rows")?;

    if collected.is_empty() {
        writeln!(stdout, "no paired devices")?;
        return Ok(());
    }

    // Plain table layout: id  name  paired_at  last_seen  revoked_at
    writeln!(
        stdout,
        "{:<20} {:<24} {:<26} {:<26} REVOKED_AT",
        "ID", "NAME", "PAIRED_AT", "LAST_SEEN"
    )?;
    for (id, name, paired_at, last_seen, revoked_at) in collected {
        let last_seen = last_seen.unwrap_or_else(|| "-".to_string());
        let revoked_at = revoked_at.unwrap_or_else(|| "-".to_string());
        writeln!(
            stdout,
            "{:<20} {:<24} {:<26} {:<26} {}",
            id, name, paired_at, last_seen, revoked_at,
        )?;
    }

    Ok(())
}

pub(crate) fn revoke(
    args: RevokeArgs,
    stdout: &mut dyn Write,
    _stderr: &mut dyn Write,
) -> Result<()> {
    ensure_data_dir()?;
    let db_path = remote_db_path()?;
    let conn = open_db(&db_path)
        .with_context(|| format!("opening omw-remote db at {}", db_path.display()))?;

    let now = Utc::now().to_rfc3339();
    let changed = conn
        .execute(
            "UPDATE devices SET revoked_at = ?1 \
             WHERE id = ?2 AND revoked_at IS NULL",
            rusqlite::params![now, &args.device_id],
        )
        .with_context(|| "executing revoke update")?;

    if changed == 0 {
        // Differentiate "already revoked" from "unknown id". The unknown
        // case must produce a 'not found' error per the test contract.
        let exists: bool = conn
            .query_row(
                "SELECT 1 FROM devices WHERE id = ?1",
                rusqlite::params![&args.device_id],
                |_| Ok(true),
            )
            .unwrap_or(false);
        if !exists {
            return Err(anyhow!("device {} not found", args.device_id));
        }
        // Already revoked — still report success-ish. But to keep contract
        // simple, treat this as success.
        writeln!(stdout, "device {} is already revoked", args.device_id)?;
        return Ok(());
    }

    writeln!(stdout, "revoked device {}", args.device_id)?;
    Ok(())
}
