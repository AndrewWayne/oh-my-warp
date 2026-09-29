// SPDX-License-Identifier: AGPL-3.0-only
//
// omw-authored file in the in-tree Warp fork (warpdotdev/warp), part of the
// AGPL-3.0 derivative work. See specs/fork-strategy.md §3.
//
// Copyright (C) 2026 Shenhao Miao and the omw contributors
// Copyright (C) 2020-2026 Denver Technologies, Inc.
//
// This program is free software: you can redistribute it and/or modify it
// under the terms of the GNU Affero General Public License, version 3, as
// published by the Free Software Foundation. See the LICENSE file at the
// repository root for the full text.

//! The in-process agent server keeps the documented 127.0.0.1:8788 when it
//! is free, and falls back to an OS-assigned port when another local
//! service holds it (that used to break every `# ` prompt).

#![cfg(feature = "omw_local")]

use warp::test_exports::bind_loopback;

// One test, run in order: parallel tests would race for 8788.
#[tokio::test(flavor = "current_thread")]
async fn bind_loopback_prefers_8788_and_falls_back_when_taken() {
    // Only check the preferred path if nothing else on this machine
    // already owns 8788.
    let free = std::net::TcpListener::bind("127.0.0.1:8788").is_ok();
    if free {
        let (listener, url) = bind_loopback().await.expect("bind_loopback");
        assert_eq!(listener.local_addr().unwrap().port(), 8788);
        assert_eq!(url, "http://127.0.0.1:8788");
    }

    // Hold 8788. If something else already owns it, this bind fails and
    // the port is taken all the same.
    let _occupier = std::net::TcpListener::bind("127.0.0.1:8788");
    let (listener, url) = bind_loopback().await.expect("bind_loopback");
    let addr = listener.local_addr().unwrap();
    assert!(addr.ip().is_loopback(), "{addr}");
    assert_ne!(addr.port(), 8788);
    assert_eq!(url, format!("http://{addr}"));
}
