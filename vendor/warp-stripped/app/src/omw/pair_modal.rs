//! Pair-modal content formatting.
//!
//! Wired by Wiring 5, Gap 2. **Scope deferral**: the full reactive Warp dialog
//! widget (with QR canvas + click-Copy + click-Stop) requires deep workspace-
//! view integration (~7 new sites in `workspace/view.rs`: ViewHandle field,
//! init, focus, render conditional, action enum, event handler, state flag),
//! which is too invasive to land in a v0.4-thin-polish session.
//!
//! What this module ships:
//! 1. [`format_pair_modal_text`] — pure formatter that produces the modal
//!    body text exactly per the Gap-2 spec (Status / Pair URL / Tailscale /
//!    Paired devices). Plain ASCII indicators (`[OK]` / `[X]`) per CLAUDE.md
//!    "no emojis without explicit ask". Used both by the toast surface today
//!    and by a future real-modal `View` impl.
//! 2. [`PairModalContent`] — input struct that pairs an [`OmwRemoteStatus`]
//!    snapshot with a [`TailscaleStatus`] snapshot, the two reactive sources
//!    a real modal would subscribe to.
//!
//! What this module does NOT ship (deferred):
//! - The Warp `View` impl + workspace wiring for a presented dialog.
//! - The reactive subscription to `OmwRemoteState::status_rx()` driving an
//!   open dialog's re-render.
//! - The paired-device count source. We display `?` until the daemon-side
//!   `Pairings` redeem hook lands. Tracked under "DEFERRED_WORK" in the Gap 2
//!   commit log.
//!
//! The click-handler in `terminal/view/use_agent_footer/mod.rs` calls
//! [`format_pair_modal_text`] then writes the URL to the clipboard and logs
//! the full block to stderr — meeting the spec goal "URL is surfaced visibly"
//! without the workspace-view surgery.

use super::remote_state::OmwRemoteStatus;
use super::tailscale::TailscaleStatus;

/// Snapshot pair the modal text formatter consumes. The two sources match
/// what a real reactive modal would subscribe to: the omw-remote daemon
/// status and a fresh tailscale probe.
pub struct PairModalContent {
    pub status: OmwRemoteStatus,
    pub tailscale: TailscaleStatus,
    /// `None` means "count not available yet" — rendered as `?`. We deferred
    /// the daemon-side hook that would bump this on each `Pairings::redeem`
    /// success; tracking it as DEFERRED_WORK in the Gap 2 commit message.
    pub paired_device_count: Option<usize>,
}

/// Render the modal body as plain text. Each line in the returned vector is
/// one display row; callers concatenate with newlines (toast surface) or feed
/// each line into a `Text` element (future real modal).
///
/// The output matches the Gap 2 spec layout:
///
/// ```text
/// Remote Control
/// ─────────────────────────────────────────
/// Status: Running on 127.0.0.1:8787
///
/// Pair URL:
///   https://<hostname>.<tailnet>.ts.net/pair?t=...
///   [Copy]   [Show QR]
///
/// Tailscale: [OK] <hostname>.<tailnet>.ts.net
///    (or: [X] Tailscale not detected — Install from tailscale.com)
///
/// Paired devices: 0
///
/// [Stop Daemon]   [Close]
/// ```
///
/// The `[Copy]` / `[Show QR]` / `[Stop Daemon]` / `[Close]` rows are kept as
/// labels here; the toast surface ignores them (the click-handler auto-copies
/// the URL on toast emission), and a real modal will render them as buttons.
pub fn format_pair_modal_text(content: &PairModalContent) -> Vec<String> {
    let mut lines = Vec::with_capacity(16);

    lines.push("Remote Control".to_string());
    lines.push("-".repeat(41));

    // Status line
    match &content.status {
        OmwRemoteStatus::Running { .. } => {
            lines.push("Status: Running on 127.0.0.1:8787".to_string());
        }
        OmwRemoteStatus::Starting => {
            lines.push("Status: Starting...".to_string());
        }
        OmwRemoteStatus::Stopped => {
            lines.push("Status: Stopped".to_string());
        }
        OmwRemoteStatus::Failed { error } => {
            lines.push(format!("Status: Failed - {error}"));
        }
    }
    lines.push(String::new());

    // Pair URL line
    lines.push("Pair URL:".to_string());
    match &content.status {
        OmwRemoteStatus::Running { pair_url, .. } => {
            lines.push(format!("  {pair_url}"));
        }
        _ => {
            lines.push("  (daemon not running)".to_string());
        }
    }
    lines.push("  [Copy]   [Show QR]".to_string());
    lines.push(String::new());

    // Tailscale status line. `local_hostname` is the FQDN (e.g.
    // `laptop.tail-abc12.ts.net`) — see `tailscale.rs` doc comment — so we
    // surface it directly without re-joining tailnet.
    if content.tailscale.installed && content.tailscale.running {
        if let Some(fqdn) = &content.tailscale.local_hostname {
            lines.push(format!("Tailscale: [OK] {fqdn}"));
        } else {
            lines.push("Tailscale: [OK] (no DNS name reported)".to_string());
        }
    } else if content.tailscale.installed {
        lines.push("Tailscale: [X] installed but not running".to_string());
    } else {
        lines.push("Tailscale: [X] not detected - Install from tailscale.com".to_string());
    }
    lines.push(String::new());

    // Paired-device count (stubbed: see DEFERRED_WORK note in module docs)
    let count_label = match content.paired_device_count {
        Some(n) => n.to_string(),
        None => "?".to_string(),
    };
    lines.push(format!("Paired devices: {count_label}"));
    lines.push(String::new());

    lines.push("[Stop Daemon]   [Close]".to_string());

    lines
}

/// Convenience: join the lines from [`format_pair_modal_text`] with `\n`.
/// This is what the click-handler logs to stderr.
pub fn format_pair_modal_text_block(content: &PairModalContent) -> String {
    format_pair_modal_text(content).join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts_running() -> TailscaleStatus {
        TailscaleStatus {
            installed: true,
            running: true,
            local_hostname: Some("laptop.tail-abc.ts.net".to_string()),
            tailnet: Some("tail-abc.ts.net".to_string()),
            tailnet_ipv4: Some("100.64.0.1".to_string()),
        }
    }

    fn ts_not_installed() -> TailscaleStatus {
        TailscaleStatus {
            installed: false,
            running: false,
            local_hostname: None,
            tailnet: None,
            tailnet_ipv4: None,
        }
    }

    /// When the daemon is running and Tailscale is up, the modal text shows
    /// the pair URL, an `[OK]` Tailscale line with the full DNS name, and a
    /// `?` paired-device count (stubbed).
    #[test]
    fn modal_text_running_with_tailscale() {
        let content = PairModalContent {
            status: OmwRemoteStatus::Running {
                pair_url: "https://laptop.tail-abc.ts.net/pair?t=tok".to_string(),
                tailscale_serving: true,
            },
            tailscale: ts_running(),
            paired_device_count: None,
        };
        let block = format_pair_modal_text_block(&content);
        assert!(block.contains("Status: Running on 127.0.0.1:8787"));
        assert!(block.contains("https://laptop.tail-abc.ts.net/pair?t=tok"));
        assert!(block.contains("Tailscale: [OK] laptop.tail-abc.ts.net"));
        assert!(block.contains("Paired devices: ?"));
        assert!(block.contains("[Stop Daemon]"));
    }

    /// When Tailscale isn't installed, the line directs the user to install
    /// it. The `[OK]` indicator is replaced with `[X]`.
    #[test]
    fn modal_text_running_without_tailscale() {
        let content = PairModalContent {
            status: OmwRemoteStatus::Running {
                pair_url: "http://127.0.0.1:8787/pair?t=tok".to_string(),
                tailscale_serving: false,
            },
            tailscale: ts_not_installed(),
            paired_device_count: Some(0),
        };
        let block = format_pair_modal_text_block(&content);
        assert!(block.contains("http://127.0.0.1:8787/pair?t=tok"));
        assert!(block.contains("Tailscale: [X] not detected"));
        assert!(block.contains("Install from tailscale.com"));
        assert!(block.contains("Paired devices: 0"));
    }

    /// A `Failed` status reports the error verbatim and shows a stub URL line
    /// rather than crashing the formatter.
    #[test]
    fn modal_text_failed_status() {
        let content = PairModalContent {
            status: OmwRemoteStatus::Failed {
                error: "bind: address already in use".to_string(),
            },
            tailscale: ts_not_installed(),
            paired_device_count: None,
        };
        let block = format_pair_modal_text_block(&content);
        assert!(block.contains("Status: Failed - bind: address already in use"));
        assert!(block.contains("(daemon not running)"));
    }

    /// No emoji codepoints land in the output. CLAUDE.md §5 prohibits emoji
    /// in product surfaces; this regression test guards the modal text body.
    #[test]
    fn modal_text_has_no_emoji_codepoints() {
        let content = PairModalContent {
            status: OmwRemoteStatus::Running {
                pair_url: "http://127.0.0.1:8787/pair?t=tok".to_string(),
                tailscale_serving: true,
            },
            tailscale: ts_running(),
            paired_device_count: Some(2),
        };
        let block = format_pair_modal_text_block(&content);
        for ch in block.chars() {
            // Reject the broad emoji ranges. We don't need a full UNICODE
            // pictograph audit — just the common blocks Warp's UI strings
            // would accidentally pull in.
            assert!(
                !matches!(
                    ch as u32,
                    0x2600..=0x27BF      // Miscellaneous Symbols + Dingbats
                    | 0x1F300..=0x1FAFF  // Emoticons / Symbols & Pictographs / Supplemental Symbols
                    | 0x2700..=0x27BF    // Dingbats
                ),
                "found emoji-range codepoint U+{:04X} in modal text",
                ch as u32
            );
        }
    }
}
