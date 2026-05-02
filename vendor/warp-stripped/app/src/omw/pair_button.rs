//! Phone-button label/tooltip helper used by the two footer surfaces that
//! host an omw Phone button: the agent input footer (CLI-agent block toolbar)
//! and the Warpify footer (subshell/SSH block toolbar). Pre v0.4-thin this
//! lived inside the agent input footer module and the Warpify footer used
//! hardcoded strings — that worked because both were keyed on a single
//! process-global `OmwRemoteStatus`. v0.4-thin makes the labels per-pane:
//! "Stop sharing" only for the pane whose `EntityId` is in the share map.
//!
//! Pure function with no warpui dependencies, so it stays trivially testable.
//!
//! `omw_local`-only.

#![cfg(feature = "omw_local")]

use super::OmwRemoteStatus;

pub const LABEL_SHARE: &str = "Share with phone";
pub const LABEL_STARTING: &str = "Starting...";
pub const LABEL_STOP_SHARING: &str = "Stop sharing";
pub const LABEL_RETRY: &str = "Retry pairing";

pub const TOOLTIP_INITIAL: &str = "Share with phone";
pub const TOOLTIP_STARTING: &str = "Starting...";
pub const TOOLTIP_SHARE_THIS: &str = "Share this pane with phone";
pub const TOOLTIP_STOP_SHARING: &str = "Stop sharing this pane";
pub const TOOLTIP_FAILED: &str = "Pairing failed — click to retry";

/// Map (daemon status, this-pane-shared) to the (label, tooltip) pair displayed
/// on a Phone button.
///
/// Per-pane semantics (design v0.4-thin §3.1):
/// - daemon down              → "Share with phone" (start daemon + share THIS pane + QR)
/// - daemon Starting          → "Starting…" (disabled label)
/// - daemon Running, !shared  → "Share with phone" (silent share, phone already paired)
/// - daemon Running, shared   → "Stop sharing" (unshare THIS pane only)
/// - daemon Failed            → "Retry pairing"
pub fn pair_button_text(
    status: &OmwRemoteStatus,
    is_shared: bool,
) -> (&'static str, &'static str) {
    match status {
        OmwRemoteStatus::Stopped => (LABEL_SHARE, TOOLTIP_INITIAL),
        OmwRemoteStatus::Starting => (LABEL_STARTING, TOOLTIP_STARTING),
        OmwRemoteStatus::Running { .. } if is_shared => {
            (LABEL_STOP_SHARING, TOOLTIP_STOP_SHARING)
        }
        OmwRemoteStatus::Running { .. } => (LABEL_SHARE, TOOLTIP_SHARE_THIS),
        OmwRemoteStatus::Failed { .. } => (LABEL_RETRY, TOOLTIP_FAILED),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn share_label_when_stopped_regardless_of_pane_state() {
        // (daemon down, !shared) is the only logically reachable input here —
        // a pane can't be shared when the daemon isn't running — but the
        // table is exhaustive over the bool input for safety.
        assert_eq!(
            pair_button_text(&OmwRemoteStatus::Stopped, false),
            (LABEL_SHARE, TOOLTIP_INITIAL)
        );
        assert_eq!(
            pair_button_text(&OmwRemoteStatus::Stopped, true),
            (LABEL_SHARE, TOOLTIP_INITIAL)
        );
    }

    #[test]
    fn share_label_when_running_but_pane_not_shared() {
        // The bug v0.4-thin fixes: pane B's button used to read "Stop pairing"
        // once any pane had been shared. Now it reads "Share with phone" until
        // pane B itself is in the share map.
        let status = OmwRemoteStatus::Running {
            pair_url: "http://127.0.0.1:8787/pair?t=x".to_string(),
            tailscale_serving: false,
        };
        assert_eq!(
            pair_button_text(&status, false),
            (LABEL_SHARE, TOOLTIP_SHARE_THIS)
        );
    }

    #[test]
    fn stop_sharing_label_when_running_and_pane_shared() {
        let status = OmwRemoteStatus::Running {
            pair_url: "http://127.0.0.1:8787/pair?t=x".to_string(),
            tailscale_serving: false,
        };
        assert_eq!(
            pair_button_text(&status, true),
            (LABEL_STOP_SHARING, TOOLTIP_STOP_SHARING)
        );
    }

    #[test]
    fn retry_label_when_failed() {
        let status = OmwRemoteStatus::Failed {
            error: "boom".to_string(),
        };
        assert_eq!(pair_button_text(&status, false), (LABEL_RETRY, TOOLTIP_FAILED));
    }
}
