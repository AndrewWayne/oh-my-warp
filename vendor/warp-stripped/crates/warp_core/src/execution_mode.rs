use std::sync::OnceLock;

use crate::channel::ChannelState;
use warpui::{Entity, ModelContext, SingletonEntity};

// Global execution mode, for logic that runs outside the UI framework.
static GLOBAL_EXECUTION_MODE: OnceLock<ExecutionMode> = OnceLock::new();

/// Execution mode that Warp is running under.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionMode {
    /// Warp is running as a normal desktop app.
    App,
    /// Warp is running as a CLI.
    Sdk,
}

impl ExecutionMode {
    /// Returns the client ID to report to the server.
    /// This must stay in sync with the util/client.go constants on the server.
    pub fn client_id(&self) -> &'static str {
        match self {
            ExecutionMode::App => "warp-app",
            ExecutionMode::Sdk => "warp-cli",
        }
    }
}

/// Model tracking the mode that Warp is running in.
///
/// This gates functionality that's disabled when Warp is running in SDK mode.
#[derive(Clone, Debug)]
pub struct AppExecutionMode {
    mode: ExecutionMode,
    is_sandboxed: bool,
}

impl AppExecutionMode {
    /// Create an `AppExecutionMode` model with the execution mode set.
    pub fn new(mode: ExecutionMode, is_sandboxed: bool, _ctx: &mut ModelContext<Self>) -> Self {
        let _ = GLOBAL_EXECUTION_MODE.set(mode);
        Self { mode, is_sandboxed }
    }

    /// True if running as the full desktop app.
    fn is_app(&self) -> bool {
        matches!(self.mode, ExecutionMode::App)
    }

    /// Whether Active AI features are allowed in this execution mode.
    ///
    /// Active AI should only run in the desktop app, where there's a user
    /// to engage with it.
    pub fn allows_active_ai(&self) -> bool {
        self.is_app() && ChannelState::official_cloud_services_enabled()
    }

    /// Whether the app can sync user preferences to the cloud. This does not gate
    /// modifying preferences locally.
    pub fn can_sync_preferences(&self) -> bool {
        self.is_app() && ChannelState::official_cloud_services_enabled()
    }

    /// Whether the app can save and restore sessions.
    pub fn can_save_session(&self) -> bool {
        self.is_app()
    }

    /// Whether the app can *automatically* update. This does not prevent manual updates.
    ///
    /// omw_local builds autoupdate against GitHub Releases (see `autoupdate::oss`), not the
    /// official cloud services — so they bypass the cloud-services gate here.
    pub fn can_autoupdate(&self) -> bool {
        self.is_app()
            && (cfg!(feature = "omw_local") || ChannelState::official_cloud_services_enabled())
    }

    /// Whether the app can automatically start MCP servers from the previous session.
    pub fn can_autostart_mcp_servers(&self) -> bool {
        self.is_app() && ChannelState::official_cloud_services_enabled()
    }

    /// Whether the app can sync agent conversations (tasks and cloud conversation metadata).
    /// In CLI mode, we don't need this data since there's no user viewing it.
    pub fn can_fetch_agent_runs_for_management(&self) -> bool {
        self.is_app() && ChannelState::official_cloud_services_enabled()
    }

    /// Whether telemetry should be sent synchronously at shutdown.
    /// In CLI mode, we synchronously send events at shutdown because there's a higher likelihood
    /// that they will be lost otherwise.
    pub fn send_telemetry_at_shutdown(&self) -> bool {
        matches!(self.mode, ExecutionMode::Sdk)
    }

    /// If true, the app is running autonomously, without a user present.
    /// Wherever possible, prefer more targeted capability checks like
    /// [`Self::can_autostart_mcp_servers`].
    pub fn is_autonomous(&self) -> bool {
        matches!(self.mode, ExecutionMode::Sdk)
    }

    /// Returns the client ID to report to the server.
    pub fn client_id(&self) -> &'static str {
        self.mode.client_id()
    }

    /// If true, Warp is running in a sandbox like a Docker container or VM, rather than directly
    /// on a user machine.
    pub fn is_sandboxed(&self) -> bool {
        self.is_sandboxed
    }
}

impl Entity for AppExecutionMode {
    type Event = ();
}

impl SingletonEntity for AppExecutionMode {}

/// Returns the current global client ID string ("warp-app" or "warp-cli").
/// This is set when AppExecutionMode is constructed during application start.
/// Returns None if the execution mode has not been set yet.
pub fn current_client_id() -> Option<&'static str> {
    GLOBAL_EXECUTION_MODE.get().map(|mode| mode.client_id())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression guard for the v0.0.6/v0.0.7 inert-poll bug (PR #63).
    ///
    /// Under `omw_local`, `ChannelState::official_cloud_services_enabled()` is
    /// intentionally false (that's what strips cloud surfaces). Before PR #63
    /// `can_autoupdate()` required it to be true, so the gate at
    /// `autoupdate/mod.rs::AutoupdateState::register` never fired. omw builds
    /// have their own GitHub-Releases autoupdate path (`autoupdate::oss`) that
    /// doesn't go through the cloud, so the gate has to bypass for omw.
    #[cfg(feature = "omw_local")]
    #[test]
    fn omw_local_app_can_autoupdate() {
        let mode = AppExecutionMode {
            mode: ExecutionMode::App,
            is_sandboxed: false,
        };
        assert!(
            mode.can_autoupdate(),
            "omw_local App-mode must allow autoupdate even when official cloud \
             services are disabled — it polls GitHub Releases, not the cloud."
        );
    }

    /// SDK mode never autoupdates regardless of channel — there's no user
    /// present to relaunch.
    #[test]
    fn sdk_mode_cannot_autoupdate() {
        let mode = AppExecutionMode {
            mode: ExecutionMode::Sdk,
            is_sandboxed: false,
        };
        assert!(!mode.can_autoupdate());
    }
}
