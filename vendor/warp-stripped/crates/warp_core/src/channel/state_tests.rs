use super::derive_http_origin_from_ws_url;
#[cfg(feature = "omw_local")]
use super::ChannelState;
#[cfg(feature = "omw_local")]
use crate::features::{FeatureFlag, OMW_LOCAL_FLAGS};

#[test]
fn wss_becomes_https_and_strips_path() {
    let got = derive_http_origin_from_ws_url("wss://rtc.app.warp.dev/graphql/v2");
    assert_eq!(got.as_deref(), Some("https://rtc.app.warp.dev"));
}

#[test]
fn ws_becomes_http_and_preserves_port() {
    let got = derive_http_origin_from_ws_url("ws://localhost:8080/graphql/v2");
    assert_eq!(got.as_deref(), Some("http://localhost:8080"));
}

#[test]
fn unparseable_input_returns_none() {
    assert!(derive_http_origin_from_ws_url("not a url").is_none());
    assert!(derive_http_origin_from_ws_url("https://app.warp.dev").is_none());
}

/// Regression guard for the explicit omw_local update surface.
///
/// Exercises the `OMW_LOCAL_FLAGS` half of the wiring used by `bin/oss.rs`:
/// take a fresh `ChannelState` and call `with_additional_features(OMW_LOCAL_FLAGS)`.
/// If anyone removes `Autoupdate` from that slice, this test fails. Operates on
/// the local state instance rather than the global via `ChannelState::set`, so
/// the test is parallel-safe. The flag keeps the command/apply update bindings
/// available; it does not enable background polling, which is controlled by
/// `AppExecutionMode::can_autoupdate`.
#[cfg(feature = "omw_local")]
#[test]
fn omw_local_channel_state_enables_autoupdate() {
    let state = ChannelState::init().with_additional_features(OMW_LOCAL_FLAGS);
    assert!(
        state
            .additional_features_set()
            .contains(&FeatureFlag::Autoupdate),
        "omw_local builds must wire Autoupdate into ChannelState::additional_features \
         via OMW_LOCAL_FLAGS; without it, the workspace:check_for_updates and \
         update-apply bindings never register."
    );
}
