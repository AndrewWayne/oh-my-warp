//! omw-specific autoupdate glue: parses the `omw-local-preview-v<x.y.z>` tag
//! format, fetches release metadata from the GitHub Releases API, and verifies
//! a downloaded DMG against its published SHA-256 sidecar.
//!
//! This module is only compiled under the `omw_local` feature. T3 keeps the
//! surface self-contained; T4 wires these symbols into the state machine.

use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use anyhow::{anyhow, Result};
use channel_versions::VersionInfo;
use lazy_static::lazy_static;
use regex::Regex;
use serde::Deserialize;
use sha2::{Digest, Sha256};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const GITHUB_ACCEPT: &str = "application/vnd.github+json";
const GITHUB_API_VERSION: &str = "2022-11-28";
const USER_AGENT_PREFIX: &str = "omw-warp-oss";
/// Cap the prefix of any non-success response body that we log so a wayward
/// HTML error page doesn't balloon the log.
const LOG_BODY_PREFIX_LEN: usize = 200;

lazy_static! {
    /// Validates the GitHub release tag (the JSON `tag_name` field).
    static ref TAG_REGEX: Regex =
        Regex::new(r"^omw-local-preview-v\d+\.\d+\.\d+$").expect("tag regex is valid");

    /// Matches the arm64 darwin DMG asset name. The version is part of the
    /// filename (curl validation 2026-05-13). Strict semver triple in the
    /// version segment (matches `TAG_REGEX`) so we don't accept asset names
    /// that don't correspond to a publishable tag.
    static ref DMG_ASSET_REGEX: Regex =
        Regex::new(r"^omw-warp-oss-v\d+\.\d+\.\d+-aarch64-apple-darwin\.dmg$")
            .expect("dmg asset regex is valid");

    /// Matches the corresponding `.sha256` sidecar uploaded alongside the DMG.
    static ref SHA_ASSET_REGEX: Regex =
        Regex::new(r"^omw-warp-oss-v\d+\.\d+\.\d+-aarch64-apple-darwin\.dmg\.sha256$")
            .expect("sha asset regex is valid");

    /// Stash for asset URLs returned by `omw_fetch_latest_release` so the
    /// downstream download/verify code paths (in `mac.rs`) can read them
    /// without threading new parameters through five layers of upstream code.
    static ref PENDING_ASSETS: Mutex<Option<OmwAssetUrls>> = Mutex::new(None);
}

/// Called by `fetch_version` after a successful fetch. Replaces any previous
/// stash — the latest fetch always wins (poll cycles are serialized).
pub(super) fn set_pending_assets(urls: OmwAssetUrls) {
    *PENDING_ASSETS.lock().unwrap_or_else(|e| e.into_inner()) = Some(urls);
}

/// Read the DMG URL from the most recent fetch. Returns `None` if no fetch has
/// stashed anything yet (e.g. first poll never succeeded).
pub(super) fn current_dmg_url() -> Option<String> {
    PENDING_ASSETS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .map(|u| u.dmg_url.clone())
}

/// Read the SHA-256 sidecar URL from the most recent fetch.
pub(super) fn current_sha_url() -> Option<String> {
    PENDING_ASSETS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .map(|u| u.sha_url.clone())
}

/// URLs for the two release assets we care about on a given GitHub release.
/// Empty strings (via `Default`) indicate "no update available".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct OmwAssetUrls {
    pub dmg_url: String,
    pub sha_url: String,
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    #[serde(default)]
    assets: Vec<GitHubAsset>,
}

#[derive(Debug, Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

/// Inline semver parser. Accepts `omw-local-preview-v<x.y.z>` or bare `<x.y.z>`.
/// Pre-release suffixes, non-numeric components, and wrong arity all return `None`.
pub(super) fn parse_omw_semver(tag: &str) -> Option<(u32, u32, u32)> {
    const PREFIX: &str = "omw-local-preview-v";
    let body = tag.strip_prefix(PREFIX).unwrap_or(tag);

    if body.is_empty() {
        return None;
    }

    // Reject anything beyond strict `<u32>.<u32>.<u32>`. A pre-release suffix
    // like `0.0.5-alpha` makes the third component non-numeric, so the parse
    // below will fail naturally.
    let parts: Vec<&str> = body.split('.').collect();
    if parts.len() != 3 {
        return None;
    }

    let major = parts[0].parse::<u32>().ok()?;
    let minor = parts[1].parse::<u32>().ok()?;
    let patch = parts[2].parse::<u32>().ok()?;

    Some((major, minor, patch))
}

/// Fetches `{releases_base_url}/latest` and returns the version and asset
/// URLs for the arm64 darwin DMG, if a usable release exists.
///
/// Behavior on the various failure modes is documented in the spec; in short:
/// 200 with valid assets → update info; 404, asset-missing, or tag-mismatch →
/// `Ok((VersionInfo::new(current_tag), Default))` meaning "no update"; 403 with
/// rate-limit metadata, 5xx, or transport errors → `Err`.
pub(super) async fn omw_fetch_latest_release(
    client: &http_client::Client,
    releases_base_url: &str,
    current_tag: &str,
) -> Result<(VersionInfo, OmwAssetUrls)> {
    let url = format!("{releases_base_url}/latest");
    let user_agent = format!("{USER_AGENT_PREFIX}/{current_tag}");

    log::info!("omw autoupdate: fetching latest release from {url}");

    let response = client
        .get(url.as_str())
        .header("User-Agent", user_agent.as_str())
        .header("Accept", GITHUB_ACCEPT)
        .header("X-GitHub-Api-Version", GITHUB_API_VERSION)
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await
        .map_err(|e| anyhow!("omw autoupdate: HTTP transport error fetching {url}: {e}"))?;

    let status = response.status();
    match status.as_u16() {
        200 => {
            // fall through to body parsing
        }
        403 | 429 => {
            // GitHub's docs: primary rate-limit may come back as either 403
            // (with X-RateLimit-Reset) or 429 (with Retry-After). Treat both
            // the same — the caller just needs to know "back off, try later".
            let reset = response
                .headers()
                .get("X-RateLimit-Reset")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_owned())
                .unwrap_or_else(|| "unknown".to_string());
            return Err(anyhow!(
                "omw autoupdate: GitHub API rate limit ({status}). reset_epoch={reset}"
            ));
        }
        404 => {
            log::info!("omw autoupdate: no releases found at {url} (404)");
            return Ok((
                VersionInfo::new(current_tag.to_string()),
                OmwAssetUrls::default(),
            ));
        }
        _ => {
            return Err(anyhow!(
                "omw autoupdate: unexpected status {status} fetching {url}"
            ));
        }
    }

    let body = response
        .text()
        .await
        .map_err(|e| anyhow!("omw autoupdate: error reading response body from {url}: {e}"))?;

    let release: GitHubRelease = match serde_json::from_str(&body) {
        Ok(release) => release,
        Err(e) => {
            let prefix: String = body.chars().take(LOG_BODY_PREFIX_LEN).collect();
            return Err(anyhow!(
                "omw autoupdate: failed to parse GitHub release JSON: {e}. body prefix: {prefix:?}"
            ));
        }
    };

    if !TAG_REGEX.is_match(&release.tag_name) {
        log::info!(
            "omw autoupdate: latest tag {:?} does not match omw-local-preview-v<x.y.z>; treating as no update",
            release.tag_name
        );
        return Ok((
            VersionInfo::new(current_tag.to_string()),
            OmwAssetUrls::default(),
        ));
    }

    let dmg_asset = release
        .assets
        .iter()
        .find(|asset| DMG_ASSET_REGEX.is_match(&asset.name));
    let sha_asset = release
        .assets
        .iter()
        .find(|asset| SHA_ASSET_REGEX.is_match(&asset.name));

    let (dmg_asset, sha_asset) = match (dmg_asset, sha_asset) {
        (Some(dmg), Some(sha)) => (dmg, sha),
        _ => {
            log::info!(
                "release {} present but required arm64 darwin asset(s) not yet uploaded",
                release.tag_name
            );
            return Ok((
                VersionInfo::new(current_tag.to_string()),
                OmwAssetUrls::default(),
            ));
        }
    };

    Ok((
        VersionInfo::new(release.tag_name),
        OmwAssetUrls {
            dmg_url: dmg_asset.browser_download_url.clone(),
            sha_url: sha_asset.browser_download_url.clone(),
        },
    ))
}

/// Verifies that `dmg_path` matches the SHA-256 published at `sha256_url`.
/// Streams the DMG so a 250MB file doesn't end up in memory. On mismatch the
/// function both logs an error and returns `Err` so the state machine sees it
/// AND the production log shows it.
pub(super) async fn verify_sha256(
    dmg_path: &Path,
    sha256_url: &str,
    client: &http_client::Client,
) -> Result<()> {
    let current_tag = warp_core::channel::ChannelState::app_version().unwrap_or_default();
    let user_agent = format!("{USER_AGENT_PREFIX}/{current_tag}");

    let response = client
        .get(sha256_url)
        .header("User-Agent", user_agent.as_str())
        .header("X-GitHub-Api-Version", GITHUB_API_VERSION)
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await
        .map_err(|e| anyhow!("omw autoupdate: error fetching {sha256_url}: {e}"))?;

    let status = response.status();
    if !status.is_success() {
        return Err(anyhow!(
            "omw autoupdate: non-success status {status} fetching SHA-256 from {sha256_url}"
        ));
    }

    let body = response.text().await.map_err(|e| {
        anyhow!("omw autoupdate: error reading SHA-256 body from {sha256_url}: {e}")
    })?;

    // `shasum -a 256` produces `<64-hex>  <filename>`. Grab the first token.
    let expected = body
        .split_whitespace()
        .next()
        .ok_or_else(|| anyhow!("omw autoupdate: empty SHA-256 body from {sha256_url}"))?
        .to_ascii_lowercase();

    if expected.len() != 64 || !expected.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(anyhow!(
            "omw autoupdate: malformed SHA-256 token from {sha256_url}: {expected:?}"
        ));
    }

    // Stream the DMG into the hasher so a ~250MB file doesn't get buffered.
    let actual = compute_sha256_streaming(dmg_path).await?;

    if expected != actual {
        log::error!(
            "SHA-256 mismatch for {dmg_path:?}: expected={expected} actual={actual}"
        );
        return Err(anyhow!(
            "SHA-256 mismatch: expected {expected} actual {actual}"
        ));
    }

    Ok(())
}

/// Streams `path` into a SHA-256 hasher and returns the lowercase hex digest.
async fn compute_sha256_streaming(path: &Path) -> Result<String> {
    use futures_lite::AsyncReadExt;

    let mut file = async_fs::File::open(path)
        .await
        .map_err(|e| anyhow!("omw autoupdate: cannot open {path:?} for hashing: {e}"))?;

    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];

    loop {
        let n = file
            .read(&mut buf)
            .await
            .map_err(|e| anyhow!("omw autoupdate: read error hashing {path:?}: {e}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }

    Ok(hex::encode(hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_omw_semver_strips_prefix() {
        assert_eq!(
            parse_omw_semver("omw-local-preview-v0.0.5"),
            Some((0, 0, 5))
        );
    }

    #[test]
    fn parse_omw_semver_bare_semver() {
        assert_eq!(parse_omw_semver("0.0.5"), Some((0, 0, 5)));
    }

    #[test]
    fn parse_omw_semver_ordering() {
        let bigger = parse_omw_semver("0.0.10").unwrap();
        let smaller = parse_omw_semver("0.0.9").unwrap();
        assert!(bigger > smaller, "(0,0,10) must compare > (0,0,9)");
    }

    #[test]
    fn parse_omw_semver_leading_zero() {
        assert_eq!(parse_omw_semver("0.0.005"), Some((0, 0, 5)));
    }

    #[test]
    fn parse_omw_semver_rejects_pre_release() {
        assert_eq!(parse_omw_semver("0.0.5-alpha"), None);
    }

    #[test]
    fn parse_omw_semver_rejects_empty() {
        assert_eq!(parse_omw_semver(""), None);
    }

    #[test]
    fn parse_omw_semver_rejects_non_numeric() {
        assert_eq!(parse_omw_semver("vX.Y.Z"), None);
    }

    #[test]
    fn parse_omw_semver_rejects_too_few_components() {
        assert_eq!(parse_omw_semver("0.5"), None);
    }

    #[test]
    fn parse_omw_semver_rejects_too_many_components() {
        assert_eq!(parse_omw_semver("0.0.5.1"), None);
    }

    #[test]
    fn parse_omw_semver_handles_double_digit_minor() {
        assert_eq!(parse_omw_semver("0.99.0"), Some((0, 99, 0)));
    }
}
