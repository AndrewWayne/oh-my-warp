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

/// Regression guard for the v0.0.6/v0.0.7/v0.0.8 inert-autoupdate bug.
///
/// Mirrors what `bin/oss.rs::main` does for the additional-features wiring:
/// take a fresh `ChannelState` and call `with_additional_features(OMW_LOCAL_FLAGS)`.
/// If anyone drops that call from the binary or removes `Autoupdate` from
/// `OMW_LOCAL_FLAGS`, this test fails. Operates on the local state instance
/// rather than the global via `ChannelState::set`, so the test is parallel-safe.
#[cfg(feature = "omw_local")]
#[test]
fn omw_local_channel_state_enables_autoupdate() {
    let state = ChannelState::init().with_additional_features(OMW_LOCAL_FLAGS);
    assert!(
        state
            .additional_features_set()
            .contains(&FeatureFlag::Autoupdate),
        "omw_local builds must wire Autoupdate into ChannelState::additional_features \
         via OMW_LOCAL_FLAGS; without it, the poll loop at autoupdate/mod.rs::register \
         and the workspace:check_for_updates binding never start."
    );
}
