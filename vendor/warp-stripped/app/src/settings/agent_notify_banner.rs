use crate::banner::BannerState;
use settings::{RespectUserSyncSetting, SupportedPlatforms, SyncToCloud};
use warp_core::define_settings_group;

// Not a setting per se, but a record of a user action persisted like one.
//
// On startup omw offers a one-click banner to wire Claude Code / Codex
// completion notifications (via the bundled `omw notify-setup`). If the user
// permanently dismisses it ("Don't show again"), we remember that here so we
// never nag again.
define_settings_group!(AgentNotifyBannerSettings, settings: [
    agent_notify_banner_state: AgentNotifyBannerState {
        type: BannerState,
        default: BannerState::NotDismissed,
        supported_platforms: SupportedPlatforms::ALL,
        sync_to_cloud: SyncToCloud::Globally(RespectUserSyncSetting::Yes),
        private: true,
    },
]);
