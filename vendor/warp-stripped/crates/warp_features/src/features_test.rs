use super::*;

#[test]
#[ignore = "CORE-3768 - need to clean up PREVIEW_FLAGS, but this is a temporary fix for the cluttered changelog"]
fn test_all_preview_flags_have_a_description() {
    for flag in PREVIEW_FLAGS {
        assert!(
            flag.flag_description()
                .is_some_and(|description| !description.is_empty()),
            "Missing description for preview-enabled flag {flag:?}"
        );
    }
}

/// Regression guard for the v0.0.6/v0.0.7/v0.0.8 autoupdate-dead bug.
///
/// `OMW_LOCAL_FLAGS` MUST contain `FeatureFlag::Autoupdate` — `bin/oss.rs` wires
/// this slice into `ChannelState::additional_features` under
/// `#[cfg(feature = "omw_local")]`. Without `Autoupdate` here, the poll loop at
/// `autoupdate/mod.rs::AutoupdateState::register` never starts AND the
/// `workspace:check_for_updates` command-palette binding at `workspace/mod.rs`
/// never registers, leaving omw users with no path to update.
#[test]
fn omw_local_flags_enables_autoupdate() {
    assert!(
        OMW_LOCAL_FLAGS.contains(&FeatureFlag::Autoupdate),
        "OMW_LOCAL_FLAGS must enable Autoupdate so omw builds receive updates. \
         If you removed it, bin/oss.rs no longer wires Autoupdate into \
         ChannelState::additional_features and the poll loop never starts."
    );
}
