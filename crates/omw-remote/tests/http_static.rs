//! Tests for the embedded Web Controller bundle served by `omw-remote`.
//!
//! Wiring sub-phase 3: the daemon serves `apps/web-controller/dist/`
//! directly via a `Router::fallback`, so any path that doesn't match an
//! `/api/v1/*` or `/ws/v1/*` route gets the SPA bundle (with SPA fallback
//! to `index.html` for unknown routes so client-side routing works).

#[path = "http_common/mod.rs"]
mod http_common;

use http_common::{http_request, spawn_server};

#[tokio::test]
async fn get_root_returns_index_html() {
    let f = spawn_server().await;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let (status, body) = http_request(f.addr, "GET", "/", vec![], &[]).await;
    assert_eq!(status, 200, "expected 200 for /; got {status}");
    let body_str = std::str::from_utf8(&body).expect("index.html is utf-8");
    assert!(
        body_str.contains(r#"<div id="root">"#),
        "expected Vite root div in body; got: {body_str}"
    );
    assert!(
        body_str.contains("<title>omw"),
        "expected omw title in body; got: {body_str}"
    );
}

#[tokio::test]
async fn get_unknown_path_returns_index_html_for_spa() {
    let f = spawn_server().await;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // react-router client-side routes — when the user reloads, the daemon
    // must hand back index.html so JS can re-handle the path.
    for path in ["/pair", "/terminal/foo/bar", "/some-deep-route/x/y"] {
        let (status, body) = http_request(f.addr, "GET", path, vec![], &[]).await;
        assert_eq!(status, 200, "expected 200 for {path}; got {status}");
        let body_str = std::str::from_utf8(&body).expect("index.html is utf-8");
        assert!(
            body_str.contains(r#"<div id="root">"#),
            "SPA fallback for {path} should serve index.html; got: {body_str}"
        );
    }
}

#[tokio::test]
async fn get_assets_path_serves_real_file() {
    let f = spawn_server().await;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // The webmanifest is a real file in dist/ — it must be served as-is,
    // not rewritten to index.html.
    let (status, body) = http_request(f.addr, "GET", "/manifest.webmanifest", vec![], &[]).await;
    assert_eq!(status, 200, "expected 200 for manifest; got {status}");
    let body_str = std::str::from_utf8(&body).expect("manifest is utf-8");
    assert!(
        body_str.contains("\"name\""),
        "expected JSON manifest body; got: {body_str}"
    );
    assert!(
        body_str.contains("omw"),
        "manifest should mention omw; got: {body_str}"
    );
}
