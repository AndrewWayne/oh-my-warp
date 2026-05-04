//! Detect a local Tailscale install + bootstrap `tailscale serve --https`.
//!
//! Wired by Gap 4 of v0.4-thin-polish (Tailscale Serve auto-bootstrap +
//! multi-origin pinning). When the embedded omw-remote daemon comes up on
//! `127.0.0.1:8787`, [`OmwRemoteState::start`] calls [`detect_status`] then
//! (if the node is running) [`serve_https`] to expose the loopback bind under
//! a real `https://<hostname>.<tailnet>.ts.net` URL — so the phone's WS
//! upgrade gets a TLS-terminated path and a stable origin.
//!
//! All shell-out commands use [`std::process::Command`] with explicit args.
//! No string interpolation, no `sh -c`, no shell injection surface.
//!
//! Every shell-out is wrapped in a hard wall-clock timeout (see
//! [`run_with_timeout`]) — `tailscale serve --bg` on a freshly-installed node
//! has been observed to block indefinitely while the LocalAPI provisions
//! HTTPS certs, which previously hung the Warp UI thread waiting on
//! `init_rx.recv()`. On timeout we kill the child via PID and treat the
//! Tailscale path as unavailable, falling back to loopback-only.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::mpsc;
use std::time::Duration;

/// Timeout for `tailscale status --json --self`. Status is normally instant;
/// 3 s leaves enough headroom for a busy LocalAPI without blocking the UI.
const STATUS_TIMEOUT: Duration = Duration::from_secs(3);

/// Timeout for `tailscale serve --bg ...` and `tailscale serve --bg ... off`.
/// Cert provisioning on first use can take a few seconds; 5 s caps the
/// worst-case UI hang and degrades to loopback-only.
const SERVE_TIMEOUT: Duration = Duration::from_secs(5);

/// Run `<bin> <args>` with a wall-clock timeout. On timeout, force-kill the
/// child by PID and return [`std::io::ErrorKind::TimedOut`]. `bin` is the
/// full path to the `tailscale` executable returned by
/// [`find_tailscale_binary`] — using a resolved path (rather than relying on
/// `PATH`) is what makes the call work from a macOS GUI app, whose inherited
/// `PATH` typically excludes Homebrew/Tailscale install dirs.
fn run_with_timeout(bin: &Path, args: &[&str], timeout: Duration) -> std::io::Result<Output> {
    let child = Command::new(bin)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .spawn()?;
    let pid = child.id();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let res = child.wait_with_output();
        let _ = tx.send(res);
    });
    match rx.recv_timeout(timeout) {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(e)) => Err(e),
        Err(_) => {
            // Child handle moved into the waiter thread, so kill via PID.
            #[cfg(windows)]
            {
                let _ = Command::new("taskkill")
                    .args(["/F", "/T", "/PID", &pid.to_string()])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
            #[cfg(not(windows))]
            {
                let _ = Command::new("kill")
                    .args(["-9", &pid.to_string()])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
            Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "tailscale subprocess timed out",
            ))
        }
    }
}

/// Snapshot of the local Tailscale install. The fields are populated
/// best-effort from `tailscale status --json --self`; missing or unparseable
/// output collapses to `installed: false` and all-`None`/false fields.
#[derive(Debug, Clone, Default)]
pub struct TailscaleStatus {
    /// True if the `tailscale` CLI is on PATH.
    pub installed: bool,
    /// True if `tailscale status --json --self` reports `BackendState ==
    /// "Running"`. False if the CLI is installed but the daemon is logged
    /// out / stopped / unreachable.
    pub running: bool,
    /// Local node FQDN with the trailing dot stripped, e.g.
    /// `"laptop.tail-abc12.ts.net"`.
    pub local_hostname: Option<String>,
    /// Tailnet name derived from the FQDN by stripping the leading hostname
    /// label, e.g. `"tail-abc12.ts.net"`.
    pub tailnet: Option<String>,
    /// First IPv4 address Tailscale assigned this node, e.g. `"100.114.215.118"`.
    /// Used as a fallback when `tailscale serve` is unavailable or not enabled
    /// on the tailnet — we can still build a phone-reachable URL by binding
    /// the daemon on 0.0.0.0 and naming this IP in the pair URL.
    pub tailnet_ipv4: Option<String>,
}

/// Failure modes for [`serve_https`] / [`unserve`]. Kept available for a
/// future env-var-gated HTTPS path; not on the v0.4-thin demo flow (we ship
/// option D — pure-JS Ed25519 in the Web Controller, plain HTTP over the
/// tailnet IP — so Tailscale Serve is no longer required).
#[derive(Debug)]
pub enum ServeError {
    /// `tailscale` CLI not on PATH.
    NotInstalled,
    /// `tailscale serve ...` exited non-zero. Carries captured stderr.
    CommandFailed(String),
    /// We couldn't even spawn the child process.
    Spawn(std::io::Error),
    /// `tailscale serve` succeeded but we have no DNSName to build a URL.
    NoLocalHostname,
}

impl std::fmt::Display for ServeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServeError::NotInstalled => write!(f, "tailscale CLI not installed"),
            ServeError::CommandFailed(msg) => write!(f, "tailscale serve failed: {msg}"),
            ServeError::Spawn(e) => write!(f, "spawning tailscale: {e}"),
            ServeError::NoLocalHostname => write!(f, "tailscale node has no DNSName yet"),
        }
    }
}

impl std::error::Error for ServeError {}

/// Locate the `tailscale` executable. Returns `Some(full_path)` if found.
///
/// Search order:
///   1. `$PATH` (covers terminal-launched and well-configured environments)
///   2. Well-known platform-conventional install locations
///
/// The fallback list matters because macOS GUI apps launched via Finder /
/// Launchpad inherit a minimal `PATH` (`/usr/bin:/bin:/usr/sbin:/sbin`) that
/// excludes Homebrew. Without this fallback, a user with Tailscale installed
/// via `brew install tailscale` would see "Tailscale not detected" in a
/// GUI-launched build and the pair URL would degrade to loopback-only.
fn find_tailscale_binary() -> Option<PathBuf> {
    let exe_name = if cfg!(windows) {
        "tailscale.exe"
    } else {
        "tailscale"
    };

    // 1. PATH lookup.
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join(exe_name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    // 2. Well-known install locations.
    #[cfg(target_os = "macos")]
    let candidates: &[&str] = &[
        "/opt/homebrew/bin/tailscale",                          // Homebrew (Apple Silicon)
        "/usr/local/bin/tailscale",                             // Homebrew (Intel) / manual
        "/Applications/Tailscale.app/Contents/MacOS/Tailscale", // Mac App Store
    ];
    #[cfg(target_os = "linux")]
    let candidates: &[&str] = &["/usr/bin/tailscale", "/usr/local/bin/tailscale"];
    #[cfg(target_os = "windows")]
    let candidates: &[&str] = &[r"C:\Program Files\Tailscale\tailscale.exe"];
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    let candidates: &[&str] = &[];

    candidates
        .iter()
        .map(PathBuf::from)
        .find(|p| p.is_file())
}

/// Probe the local Tailscale install. Cheap to call: shells out at most once.
/// On any error (CLI missing, daemon down, unparseable JSON) returns a
/// best-effort [`TailscaleStatus`] with whatever fields we could establish.
pub fn detect_status() -> TailscaleStatus {
    let bin = match find_tailscale_binary() {
        Some(p) => p,
        None => return TailscaleStatus::default(),
    };

    let output = match run_with_timeout(&bin, &["status", "--json", "--self"], STATUS_TIMEOUT) {
        Ok(o) => o,
        Err(_) => {
            // Includes io::Error::TimedOut from run_with_timeout — the LocalAPI
            // can hang on a freshly-installed node; we degrade to "installed
            // but not running" so OmwRemoteState falls back to loopback-only.
            return TailscaleStatus {
                installed: true,
                ..TailscaleStatus::default()
            };
        }
    };
    if !output.status.success() {
        return TailscaleStatus {
            installed: true,
            ..TailscaleStatus::default()
        };
    }
    let stdout = match std::str::from_utf8(&output.stdout) {
        Ok(s) => s,
        Err(_) => {
            return TailscaleStatus {
                installed: true,
                ..TailscaleStatus::default()
            };
        }
    };
    parse_status_json(stdout)
}

/// Parse the JSON shape produced by `tailscale status --json --self`. Public
/// for the in-file unit tests; not re-exported from `omw::mod`.
fn parse_status_json(s: &str) -> TailscaleStatus {
    let v: serde_json::Value = match serde_json::from_str(s) {
        Ok(v) => v,
        Err(_) => {
            return TailscaleStatus {
                installed: true,
                ..TailscaleStatus::default()
            };
        }
    };

    let backend = v
        .get("BackendState")
        .and_then(|x| x.as_str())
        .unwrap_or("");
    let running = backend == "Running";

    // DNSName is the FQDN of this node, with a trailing dot. Strip it.
    let raw_dns = v
        .get("Self")
        .and_then(|s| s.get("DNSName"))
        .and_then(|x| x.as_str())
        .unwrap_or("");
    let trimmed = raw_dns.trim_end_matches('.').to_string();
    let local_hostname = if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    };

    // Tailnet = DNSName with the first label stripped. e.g.
    // "laptop.tail-abc12.ts.net" -> "tail-abc12.ts.net".
    let tailnet = local_hostname
        .as_deref()
        .and_then(|h| h.split_once('.').map(|(_, rest)| rest.to_string()));

    // First IPv4 from Self.TailscaleIPs (preferred) or top-level TailscaleIPs.
    let tailnet_ipv4 = v
        .get("Self")
        .and_then(|s| s.get("TailscaleIPs"))
        .or_else(|| v.get("TailscaleIPs"))
        .and_then(|ips| ips.as_array())
        .and_then(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str())
                .find(|s| !s.contains(':'))
                .map(|s| s.to_string())
        });

    TailscaleStatus {
        installed: true,
        running,
        local_hostname,
        tailnet,
        tailnet_ipv4,
    }
}

/// `tailscale serve --bg <port>` and return the URL the node is now reachable
/// at (`https://<DNSName>` — Tailscale's modern `serve` implies HTTPS on the
/// tailnet hostname's port 443).
///
/// Works on Tailscale 1.62+. On 1.96+ the older `--https=PORT <target>` form
/// was removed; using it returns a usage error. We use the modern form.
///
/// Common failure mode: `Serve is not enabled on your tailnet.` — the user
/// must enable Serve in their Tailscale admin console first
/// (https://login.tailscale.com/f/serve). [`ServeError::CommandFailed`]
/// surfaces the underlying stderr so callers can detect this and fall back to
/// a tailnet-IP-based URL instead.
pub fn serve_https(port: u16) -> Result<String, ServeError> {
    let bin = find_tailscale_binary().ok_or(ServeError::NotInstalled)?;
    let port_str = port.to_string();

    let output = run_with_timeout(&bin, &["serve", "--bg", &port_str], SERVE_TIMEOUT)
        .map_err(ServeError::Spawn)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        return Err(ServeError::CommandFailed(stderr));
    }

    let status = detect_status();
    let host = status.local_hostname.ok_or(ServeError::NoLocalHostname)?;
    Ok(format!("https://{host}"))
}

/// `tailscale serve --bg <port> off`. Best-effort teardown; callers run this
/// on shutdown but can also invoke it before re-binding. On Tailscale 1.96+
/// the modern path is `tailscale serve clear`, but the legacy `<target> off`
/// form is still accepted as a no-op when nothing is mapped — so we use the
/// per-port form here to avoid clobbering unrelated Serve configs the user
/// may have set up by hand.
pub fn unserve(port: u16) -> Result<(), ServeError> {
    let bin = find_tailscale_binary().ok_or(ServeError::NotInstalled)?;
    let port_str = port.to_string();
    let output = run_with_timeout(&bin, &["serve", "--bg", &port_str, "off"], SERVE_TIMEOUT)
        .map_err(ServeError::Spawn)?;
    if !output.status.success() {
        // Don't propagate — failure here is expected when Serve was never
        // enabled or the port was never mapped. Caller already treats this
        // best-effort.
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        return Err(ServeError::CommandFailed(stderr));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Representative shape captured from a real `tailscale status --json
    /// --self` payload (irrelevant fields elided). The trailing dot on
    /// `DNSName` is real Tailscale behavior; the parser must strip it.
    const SAMPLE_JSON: &str = r#"{
        "BackendState": "Running",
        "Self": {
            "ID": "node1234",
            "PublicKey": "nodekey:abcd",
            "DNSName": "laptop.tail-abc12.ts.net.",
            "Online": true
        }
    }"#;

    #[test]
    fn detect_status_parses_real_json() {
        let s = parse_status_json(SAMPLE_JSON);
        assert!(s.installed, "JSON parsed -> CLI is installed");
        assert!(s.running, "BackendState=Running -> running");
        assert_eq!(
            s.local_hostname.as_deref(),
            Some("laptop.tail-abc12.ts.net"),
            "trailing dot must be stripped"
        );
        assert_eq!(
            s.tailnet.as_deref(),
            Some("tail-abc12.ts.net"),
            "tailnet = hostname minus first label"
        );
    }

    #[test]
    fn detect_status_handles_logged_out_node() {
        // Backend reports something other than Running; DNSName empty.
        let json = r#"{"BackendState": "NeedsLogin", "Self": {"DNSName": ""}}"#;
        let s = parse_status_json(json);
        assert!(s.installed);
        assert!(!s.running);
        assert!(s.local_hostname.is_none());
        assert!(s.tailnet.is_none());
    }

    #[test]
    fn detect_status_handles_unparseable_json() {
        let s = parse_status_json("not valid json");
        // We treat "we found the CLI but couldn't parse" as installed=true,
        // everything else default. The orchestration layer falls back to
        // loopback-only.
        assert!(s.installed);
        assert!(!s.running);
        assert!(s.local_hostname.is_none());
        assert!(s.tailnet.is_none());
    }

    #[test]
    fn detect_status_when_not_installed() {
        // Simulate the not-installed branch directly. We can't easily
        // intercept `which` without a proc-macro shim, but the public
        // [`detect_status`] returns `TailscaleStatus::default()` on the
        // not-on-PATH branch — and the default has installed=false.
        let s = TailscaleStatus::default();
        assert!(!s.installed);
        assert!(!s.running);
        assert!(s.local_hostname.is_none());
        assert!(s.tailnet.is_none());
    }

}
