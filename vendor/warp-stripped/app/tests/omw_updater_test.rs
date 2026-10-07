// SPDX-License-Identifier: AGPL-3.0-only
//
// omw-authored tests in the in-tree upstream fork, part of the AGPL-3.0
// derivative work. See specs/fork-strategy.md section 3.
// Copyright (C) 2026 omw contributors
// Copyright (C) 2020-2026 Denver Technologies, Inc.

#![cfg(feature = "omw_local")]

// Exercise the unchanged updater module without mounting the application UI.
#[path = "../src/autoupdate/omw_oss.rs"]
mod omw_oss;

use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread::JoinHandle;

const CURRENT: &str = "omw-local-preview-v0.0.11";
const LATEST: &str = "omw-local-preview-v0.0.13";

fn response(status: u16, body: String) -> (String, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut request = vec![0; 8192];
        let bytes = stream.read(&mut request).unwrap();
        let request = String::from_utf8_lossy(&request[..bytes]);
        assert!(request.starts_with("GET /"));
        assert!(request
            .to_ascii_lowercase()
            .contains("user-agent: omw-warp-oss/"));
        write!(
            stream,
            "HTTP/1.1 {status} QA\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
    });
    (url, handle)
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

#[test]
fn release_metadata_selects_assets_and_preserves_pending_urls() {
    let body = serde_json::json!({"tag_name":LATEST, "assets":[
        {"name":"omw-warp-oss-v0.0.13-aarch64-apple-darwin.dmg", "browser_download_url":"https://example.test/release.dmg"},
        {"name":"omw-warp-oss-v0.0.13-aarch64-apple-darwin.dmg.sha256", "browser_download_url":"https://example.test/release.dmg.sha256"},
    ]}).to_string();
    let (url, server) = response(200, body);
    let (version, assets) = runtime()
        .block_on(omw_oss::omw_fetch_latest_release(
            &http_client::Client::new_for_test(),
            &url,
            CURRENT,
        ))
        .unwrap();
    server.join().unwrap();
    assert_eq!(version.version, LATEST);
    assert!(omw_oss::parse_omw_semver(LATEST) > omw_oss::parse_omw_semver(CURRENT));
    omw_oss::set_pending_assets(assets);
    assert_eq!(
        omw_oss::current_dmg_url().as_deref(),
        Some("https://example.test/release.dmg")
    );
    assert_eq!(
        omw_oss::current_sha_url().as_deref(),
        Some("https://example.test/release.dmg.sha256")
    );
}

#[test]
fn absent_or_unusable_releases_keep_current_version() {
    for (status, body) in [
        (404, String::new()),
        (
            200,
            serde_json::json!({"tag_name":LATEST, "assets":[]}).to_string(),
        ),
        (
            200,
            serde_json::json!({"tag_name":"v9.9.9", "assets":[]}).to_string(),
        ),
    ] {
        let (url, server) = response(status, body);
        let (version, assets) = runtime()
            .block_on(omw_oss::omw_fetch_latest_release(
                &http_client::Client::new_for_test(),
                &url,
                CURRENT,
            ))
            .unwrap();
        server.join().unwrap();
        assert_eq!(version.version, CURRENT);
        assert_eq!(assets, omw_oss::OmwAssetUrls::default());
    }
}

#[test]
fn rate_limit_server_error_and_invalid_json_are_reported() {
    for (status, body) in [(403, ""), (429, ""), (500, ""), (200, "invalid JSON")] {
        let (url, server) = response(status, body.to_string());
        assert!(runtime()
            .block_on(omw_oss::omw_fetch_latest_release(
                &http_client::Client::new_for_test(),
                &url,
                CURRENT
            ))
            .is_err());
        server.join().unwrap();
    }
}

#[test]
fn checksum_accepts_matching_bytes_and_rejects_tampering_or_invalid_sidecars() {
    use sha2::{Digest, Sha256};

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fixture.dmg");
    std::fs::write(&path, b"QA package bytes").unwrap();
    let digest = hex::encode(Sha256::digest(b"QA package bytes"));
    for (status, body, valid) in [
        (200, format!("{digest}  fixture.dmg\n"), true),
        (
            200,
            format!("{}  fixture.dmg\n", digest.to_ascii_uppercase()),
            true,
        ),
        (200, "0".repeat(64), false),
        (200, "not-a-checksum".to_string(), false),
        (200, String::new(), false),
        (404, String::new(), false),
    ] {
        let (url, server) = response(status, body);
        let result = runtime().block_on(omw_oss::verify_sha256(
            &path,
            &url,
            &http_client::Client::new_for_test(),
        ));
        server.join().unwrap();
        assert_eq!(result.is_ok(), valid);
    }
}
