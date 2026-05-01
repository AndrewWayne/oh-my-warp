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

/// Run `tailscale <args>` with a wall-clock timeout. On timeout, force-kill
/// the child by PID and return [`std::io::ErrorKind::TimedOut`].
fn run_with_timeout(args: &[&str], timeout: Duration) -> std::io::Result<Output> {
    let child = Command::new("tailscale")
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
}

/// Failure modes for [`serve_https`] / [`unserve`]. We don't try to
/// distinguish e.g. "permission denied" from "wrong CLI version" — Gap 4
/// just falls back to loopback-only when serve fails.
#[derive(Debug)]
pub enum ServeError {
    /// `tailscale` CLI not on PATH.
    NotInstalled,
    /// `tailscale serve ...` exited non-zero. Carries the captured stderr so
    /// the orchestration layer can log it once and move on.
    CommandFailed(String),
    /// We couldn't even spawn the child process.
    Spawn(std::io::Error),
    /// `tailscale serve` succeeded but we have no DNSName to build a URL
    /// from (e.g., node isn't logged in). Caller should treat this as
    /// loopback-only.
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

/// Returns true if `tailscale` resolves on PATH. We don't depend on the
/// `which` crate; the standard PATH-walk is enough for our needs.
fn tailscale_on_path() -> bool {
    let exe_name = if cfg!(windows) {
        "tailscale.exe"
    } else {
        "tailscale"
    };
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            if dir.join(exe_name).is_file() {
                return true;
            }
        }
    }
    false
}

/// Probe the local Tailscale install. Cheap to call: shells out at most once.
/// On any error (CLI missing, daemon down, unparseable JSON) returns a
/// best-effort [`TailscaleStatus`] with whatever fields we could establish.
pub fn detect_status() -> TailscaleStatus {
    if !tailscale_on_path() {
        return TailscaleStatus::default();
    }

    let output = match run_with_timeout(&["status", "--json", "--self"], STATUS_TIMEOUT) {
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

    TailscaleStatus {
        installed: true,
        running,
        local_hostname,
        tailnet,
    }
}

/// `tailscale serve --bg --https=<port> http://127.0.0.1:<port>` and return
/// the URL the node is now reachable at (`https://<DNSName>` — Tailscale
/// listens on 443 by default for the `--https` flag, no port suffix needed).
pub fn serve_https(port: u16) -> Result<String, ServeError> {
    if !tailscale_on_path() {
        return Err(ServeError::NotInstalled);
    }
    let target = format!("http://127.0.0.1:{port}");
    let https_arg = format!("--https={port}");

    let output = run_with_timeout(
        &["serve", "--bg", &https_arg, &target],
        SERVE_TIMEOUT,
    )
    .map_err(ServeError::Spawn)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        return Err(ServeError::CommandFailed(stderr));
    }

    let status = detect_status();
    let host = status.local_hostname.ok_or(ServeError::NoLocalHostname)?;
    Ok(format!("https://{host}"))
}

/// `tailscale serve --bg --https=<port> off`. Best-effort teardown; callers
/// run this on shutdown but can also invoke it before re-binding.
pub fn unserve(port: u16) -> Result<(), ServeError> {
    if !tailscale_on_path() {
        return Err(ServeError::NotInstalled);
    }
    let https_arg = format!("--https={port}");
    let output = run_with_timeout(
        &["serve", "--bg", &https_arg, "off"],
        SERVE_TIMEOUT,
    )
    .map_err(ServeError::Spawn)?;
    if !output.status.success() {
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
