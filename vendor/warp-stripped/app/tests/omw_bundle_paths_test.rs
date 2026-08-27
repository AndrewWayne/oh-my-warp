// SPDX-License-Identifier: AGPL-3.0-only

#![cfg(feature = "omw_local")]

use warp::ai_assistant::omw_inproc_server::{
    test_platform_executable_name, test_walk_up_for_workspace_helper,
};

#[test]
fn bundled_executable_names_use_the_platform_suffix() {
    assert_eq!(
        test_platform_executable_name("omw-keychain-helper"),
        format!("omw-keychain-helper{}", std::env::consts::EXE_SUFFIX)
    );
    assert_eq!(
        test_platform_executable_name("node"),
        format!("node{}", std::env::consts::EXE_SUFFIX)
    );
}

#[test]
fn workspace_helper_fallback_uses_the_platform_executable_name() {
    let temp = tempfile::tempdir().expect("create temp directory");
    let helper = temp
        .path()
        .join("target")
        .join("debug")
        .join(test_platform_executable_name("omw-keychain-helper"));
    std::fs::create_dir_all(helper.parent().expect("helper parent"))
        .expect("create helper parent");
    std::fs::write(&helper, b"test helper").expect("create fake helper");

    let start = temp.path().join("a").join("b").join("c");
    std::fs::create_dir_all(&start).expect("create nested start path");

    assert_eq!(test_walk_up_for_workspace_helper(&start), Some(helper));
}
