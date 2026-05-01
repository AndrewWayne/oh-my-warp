//! Embedded Web Controller bundle + static-asset / SPA-fallback handler.
//!
//! When the `embedded-web-controller` feature is on (default), `build.rs`
//! ensures `apps/web-controller/dist/` exists and `include_dir!` bakes it
//! into the binary. The `serve_static` handler is wired as the router
//! fallback in `server::make_router`, so any request that doesn't match an
//! `/api/v1/*` or `/ws/v1/*` route is served from the bundle.
//!
//! The fallback also implements the standard SPA pattern: unknown paths
//! return `index.html` so client-side react-router routes (`/pair`,
//! `/terminal/...`) keep working when the user reloads the page.
//!
//! With the feature off, `serve_static` returns 503 with a build hint.

#[cfg(feature = "embedded-web-controller")]
use axum::http::header;
use axum::http::{StatusCode, Uri};
use axum::response::IntoResponse;

#[cfg(feature = "embedded-web-controller")]
use include_dir::{include_dir, Dir};

#[cfg(feature = "embedded-web-controller")]
pub static WEB_CONTROLLER_DIST: Dir<'_> =
    include_dir!("$CARGO_MANIFEST_DIR/../../apps/web-controller/dist");

/// Axum fallback handler. Wired via `Router::fallback`, so all routes that
/// don't otherwise match land here.
///
/// - `GET /` and any unknown path -> `index.html` (SPA fallback).
/// - `GET /assets/...`, `/manifest.webmanifest`, `/icon-*.png` -> the bundled
///   file with a guessed Content-Type.
pub async fn serve_static(uri: Uri) -> axum::response::Response {
    serve_static_impl(uri.path())
}

#[cfg(feature = "embedded-web-controller")]
fn serve_static_impl(path: &str) -> axum::response::Response {
    let trimmed = path.trim_start_matches('/');

    // Empty path = root = index.html.
    if !trimmed.is_empty() {
        if let Some(file) = WEB_CONTROLLER_DIST.get_file(trimmed) {
            let mime = mime_guess::from_path(trimmed)
                .first_or_octet_stream()
                .essence_str()
                .to_string();
            return ([(header::CONTENT_TYPE, mime)], file.contents().to_vec()).into_response();
        }
    }

    // SPA fallback: serve index.html so client-side routing handles the path.
    match WEB_CONTROLLER_DIST.get_file("index.html") {
        Some(index) => (
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            index.contents().to_vec(),
        )
            .into_response(),
        None => (StatusCode::NOT_FOUND, "not_found").into_response(),
    }
}

#[cfg(not(feature = "embedded-web-controller"))]
fn serve_static_impl(_path: &str) -> axum::response::Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        "web controller not embedded; rebuild with the `embedded-web-controller` feature \
         (cd apps/web-controller && npm install && npm run build, then cargo build)",
    )
        .into_response()
}
