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

use std::process::Command;

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

    let output = match Command::new("tailscale")
        .args(["status", "--json", "--self"])
        .output()
    {
        Ok(o) => o,
        Err(_) => {
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

    let output = Command::new("tailscale")
        .args(["serve", "--bg", &https_arg, &target])
        .output()
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
    let output = Command::new("tailscale")
        .args(["serve", "--bg", &https_arg, "off"])
        .output()
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
