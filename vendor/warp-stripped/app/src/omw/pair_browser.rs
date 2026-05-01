//! Open the user's default browser to a local HTML page that displays the
//! pair URL + a scannable QR code.
//!
//! Wired by Gap 2 follow-up: the original toast surface (clipboard + stderr)
//! left users running warp-oss.exe from Explorer with no visible URL or QR.
//! This module is the minimum-viable replacement for the deferred reactive
//! `View<>`-backed Warp dialog: we render the QR SVG via [`crate::omw::qr`],
//! wrap it in a small self-contained HTML page, write it under
//! `std::env::temp_dir()`, and shell out to the platform's default opener.
//!
//! Why a temp HTML file instead of a `data:` URL: `data:` URLs ~5-15 KB are
//! supported by every modern browser, but the file approach gives the user a
//! stable URL to revisit and works around any data-URL size quirks.
//!
//! The full reactive in-app dialog (Gap 2 §2.2 in the continuation doc) is
//! still the right long-term shape; this module deletes itself once that
//! lands.

use std::io;
use std::path::PathBuf;
use std::process::Command;

use super::qr::render_pair_url_svg;

/// Build the pair-page HTML for the given URL and the optional Tailscale
/// hostname (for the status line). The QR code is embedded inline as SVG.
fn build_html(pair_url: &str, tailscale_hostname: Option<&str>) -> String {
    let qr_svg = match render_pair_url_svg(pair_url) {
        Ok(s) => s,
        Err(_) => String::from(
            "<p style=\"color:#b00;\">QR render failed — copy the URL below and paste it into your phone's browser.</p>",
        ),
    };
    let tailscale_line = match tailscale_hostname {
        Some(h) => format!(
            "<p class=\"meta\">Tailscale: <code>{h}</code> &middot; Phone must be on the same tailnet.</p>"
        ),
        None => String::from(
            "<p class=\"meta\" style=\"color:#b80;\">Tailscale not detected — only reachable from this machine. Install Tailscale to pair from a phone.</p>",
        ),
    };
    let escaped_url = pair_url
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;");
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>omw — Pair Phone</title>
<style>
  body {{ font-family: -apple-system, system-ui, "Segoe UI", sans-serif; max-width: 640px; margin: 40px auto; padding: 0 20px; color: #222; }}
  h1 {{ margin-top: 0; }}
  .qr {{ display: inline-block; padding: 16px; background: #fff; border: 1px solid #ddd; border-radius: 8px; }}
  .qr svg {{ display: block; width: 280px; height: 280px; }}
  .url-box {{ margin: 16px 0 8px; padding: 12px 14px; background: #f5f5f5; border-radius: 6px; font-family: ui-monospace, "SF Mono", Consolas, monospace; word-break: break-all; user-select: all; }}
  button {{ padding: 8px 16px; cursor: pointer; font-size: 14px; }}
  .meta {{ color: #555; font-size: 13px; }}
  .row {{ display: flex; gap: 12px; align-items: center; flex-wrap: wrap; }}
</style>
</head>
<body>
<h1>omw Remote Control — pair your phone</h1>
<p>Scan this QR with your phone, or paste the URL below into your phone's browser.</p>
<div class="qr">{qr_svg}</div>
{tailscale_line}
<h3>Pair URL</h3>
<div class="url-box" id="url">{escaped_url}</div>
<div class="row">
  <button id="copy">Copy URL</button>
  <span id="copy-status" class="meta"></span>
</div>
<script>
  document.getElementById('copy').addEventListener('click', async () => {{
    const url = document.getElementById('url').textContent;
    try {{
      await navigator.clipboard.writeText(url);
      document.getElementById('copy-status').textContent = 'Copied!';
    }} catch (e) {{
      document.getElementById('copy-status').textContent = 'Copy failed — select the URL above manually.';
    }}
  }});
</script>
</body>
</html>
"#
    )
}

/// Write the page under `<tmp>/omw-pair.html` and return the path.
fn write_pair_html(pair_url: &str, tailscale_hostname: Option<&str>) -> io::Result<PathBuf> {
    let path = std::env::temp_dir().join("omw-pair.html");
    let html = build_html(pair_url, tailscale_hostname);
    std::fs::write(&path, html)?;
    Ok(path)
}

/// Spawn the platform-default opener for the given file path.
fn open_with_default_app(path: &PathBuf) -> io::Result<()> {
    #[cfg(windows)]
    {
        // `cmd /c start "" <path>` is the canonical Windows "open with default
        // app" invocation. The empty `""` is the window title; without it
        // `start` interprets the path as a window title when it has spaces.
        let p = path.to_string_lossy().to_string();
        Command::new("cmd")
            .args(["/c", "start", "", &p])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Command::new("xdg-open")
            .arg(path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;
    }
    Ok(())
}

/// Render an HTML pair page for `pair_url` (with optional tailnet hostname for
/// the status line) and open it in the user's default browser. Returns the
/// path of the file that was written so callers can log it.
pub fn open_pair_page(pair_url: &str, tailscale_hostname: Option<&str>) -> io::Result<PathBuf> {
    let path = write_pair_html(pair_url, tailscale_hostname)?;
    open_with_default_app(&path)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// HTML must contain the URL (escaped) and the SVG tag from the QR helper.
    #[test]
    fn build_html_embeds_url_and_qr() {
        let html = build_html("https://laptop.tail-abc.ts.net/pair?t=xyz&n=1", Some("laptop.tail-abc.ts.net"));
        assert!(html.contains("<svg"), "expected QR SVG in HTML");
        assert!(
            html.contains("https://laptop.tail-abc.ts.net/pair?t=xyz&amp;n=1"),
            "expected escaped URL in HTML"
        );
        assert!(html.contains("Pair URL"), "expected URL section heading");
    }

    /// When no Tailscale hostname is supplied, the page must explain that the
    /// URL is loopback-only.
    #[test]
    fn build_html_warns_on_no_tailscale() {
        let html = build_html("http://127.0.0.1:8787/pair?t=xyz", None);
        assert!(
            html.contains("Tailscale not detected"),
            "expected loopback-only warning"
        );
    }

    /// Writing the file should succeed and produce a non-empty file at the
    /// returned path under the system temp dir.
    #[test]
    fn write_pair_html_creates_file() {
        let path = write_pair_html("http://127.0.0.1:8787/pair?t=tt", None)
            .expect("write_pair_html must succeed under temp dir");
        let bytes = std::fs::read(&path).expect("file must exist after write");
        assert!(bytes.len() > 200, "html file should be non-trivial");
        // Cleanup is left to the OS — it lives under the temp dir which is
        // periodically reaped.
    }
}
