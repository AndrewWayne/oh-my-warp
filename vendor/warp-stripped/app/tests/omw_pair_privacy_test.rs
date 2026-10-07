// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 Shenhao Miao and the omw contributors
// Copyright (C) 2020-2026 Denver Technologies, Inc.
// This file is part of the AGPL-3.0 derivative work. See LICENSE.

#![cfg(all(feature = "omw_local", target_os = "macos"))]

use warp::test_exports::OmwRemoteStatus;

#[test]
fn mac_remote_status_debug_keeps_pairing_token_out_of_logs() {
    let pair_url = "http://127.0.0.1:8787/pair?t=qa-pair-secret";
    let status = OmwRemoteStatus::Running {
        pair_url: pair_url.into(),
        tailscale_serving: false,
    };
    let debug = format!("{status:?}");
    assert!(!debug.contains("qa-pair-secret"), "{debug}");
    assert!(!debug.contains("http://"), "{debug}");
    assert!(debug.contains("Running"));
    assert!(debug.contains("tailscale_serving: false"));
    if let OmwRemoteStatus::Running { pair_url: retained, .. } = status {
        assert_eq!(retained, pair_url, "pairing UI must still receive the real URL");
    }
}
