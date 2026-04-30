//! C.3 — Contract test for `POST /internal/v1/sessions/:id/input`.
//!
//! Spawns an ACK-transform session (echo each line back with an `ACK:` prefix
//! — same trick used in `omw-pty`'s write-and-echo test to disambiguate from
//! PTY line-discipline echo) and verifies that posting a base64-encoded input
//! body returns 2xx.
//!
//! The negative-path tests (unknown id → 404, bad base64 → 4xx) only assert
//! the response status. The `post_input_round_trips_through_pty` test
//! additionally proves the bytes actually reach the PTY by subscribing to the
//! session output stream via `SessionRegistry::subscribe` and asserting the
//! ACK transform shows up — without this, a handler that validates and
//! returns OK without calling `write_input` would slip through.

use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tokio::time::timeout;
use tower::ServiceExt;

use omw_server::{router, SessionRegistry};

fn ack_loop_spec_json() -> Value {
    if cfg!(windows) {
        let script = "while ($true) { $line = Read-Host; if ($null -eq $line) { break }; Write-Host ('ACK:' + $line) }";
        json!({
            "name": "ack-loop",
            "command": "powershell",
            "args": ["-NoProfile", "-Command", script],
        })
    } else {
        json!({
            "name": "ack-loop",
            "command": "sh",
            "args": [
                "-c",
                "stty -echo; while IFS= read -r line; do printf 'ACK:%s\\n' \"$line\"; done",
            ],
        })
    }
}

async fn body_to_json(body: Body) -> Value {
    let bytes = body.collect().await.expect("collect").to_bytes();
    serde_json::from_slice(&bytes).expect("valid JSON")
}

async fn register_session(app: axum::Router) -> (axum::Router, String) {
    let body = serde_json::to_vec(&ack_loop_spec_json()).unwrap();
    let req = Request::builder()
        .method("POST")
        .uri("/internal/v1/sessions")
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap();
    let resp = app
        .clone()
        .oneshot(req)
        .await
        .expect("POST /sessions must succeed");
    let status = resp.status();
    assert!(
        status == StatusCode::CREATED || status == StatusCode::OK,
        "expected 200 or 201 from POST /sessions, got {status}"
    );
    let body = body_to_json(resp.into_body()).await;
    let id = body
        .get("id")
        .and_then(Value::as_str)
        .expect("response must include id")
        .to_string();
    (app, id)
}

#[tokio::test]
async fn post_input_with_valid_base64_returns_2xx() {
    let registry = SessionRegistry::new();
    let app = router(registry);
    let (app, id) = register_session(app).await;

    let payload = "omw\n";
    let b64 = base64::engine::general_purpose::STANDARD.encode(payload.as_bytes());
    let body = serde_json::to_vec(&json!({ "bytes": b64 })).unwrap();

    let req = Request::builder()
        .method("POST")
        .uri(format!("/internal/v1/sessions/{id}/input"))
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap();

    let resp = app
        .oneshot(req)
        .await
        .expect("POST /sessions/:id/input must be routed");

    let status = resp.status();
    assert!(
        status == StatusCode::OK || status == StatusCode::NO_CONTENT,
        "expected 200 or 204 from POST /sessions/:id/input, got {status}"
    );
}

#[tokio::test]
async fn post_input_with_unknown_id_returns_404() {
    let registry = SessionRegistry::new();
    let app = router(registry);

    let bogus = "00000000-0000-0000-0000-000000000000";
    let b64 = base64::engine::general_purpose::STANDARD.encode(b"hi\n");
    let body = serde_json::to_vec(&json!({ "bytes": b64 })).unwrap();

    let req = Request::builder()
        .method("POST")
        .uri(format!("/internal/v1/sessions/{bogus}/input"))
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap();

    let resp = app
        .oneshot(req)
        .await
        .expect("router must accept POST input for unknown id");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "POST input for unknown id must be 404"
    );
}

#[tokio::test]
async fn post_input_round_trips_through_pty() {
    // Prove the input handler actually pumps bytes into the PTY by subscribing
    // to the session's output broadcast channel and looking for the ACK echo.
    // A handler that 200s without calling `write_input` would never produce
    // output here.
    let registry = SessionRegistry::new();
    let app = router(registry.clone());
    let (app, id_str) = register_session(app).await;

    let id: omw_server::SessionId = id_str.parse().expect("registered id must be a valid uuid");

    // Subscribe BEFORE posting input so the broadcast receiver is in place
    // when the child writes its ACK back. `tokio::sync::broadcast` only
    // delivers messages sent after the subscription point.
    let mut rx = registry
        .subscribe(id)
        .expect("subscribe must succeed for a freshly-registered session");

    // Give the child a moment to apply `stty -echo` (Unix) before we send,
    // so the assertion can reliably distinguish ACK output from input echo.
    tokio::time::sleep(Duration::from_millis(200)).await;

    // On Windows, PowerShell `Read-Host` running on ConPTY only treats CRLF
    // as a line terminator — bare LF leaves the line buffered and the child
    // never echoes back. Unix `read -r` is happy with LF alone.
    const INPUT_LINE: &[u8] = if cfg!(windows) { b"omw\r\n" } else { b"omw\n" };
    let b64 = base64::engine::general_purpose::STANDARD.encode(INPUT_LINE);
    let body = serde_json::to_vec(&json!({ "bytes": b64 })).unwrap();

    let req = Request::builder()
        .method("POST")
        .uri(format!("/internal/v1/sessions/{id_str}/input"))
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap();

    let resp = app
        .oneshot(req)
        .await
        .expect("POST /sessions/:id/input must be routed");
    let status = resp.status();
    assert!(
        status == StatusCode::OK || status == StatusCode::NO_CONTENT,
        "expected 200 or 204 from POST /sessions/:id/input, got {status}"
    );

    // Drain the broadcast for up to 5s, accumulating bytes until we see
    // "ACK:omw" — proving the handler actually called write_input on the
    // session's PTY rather than just rubber-stamping the request.
    let saw_ack = timeout(Duration::from_secs(5), async {
        let mut acc = Vec::<u8>::new();
        loop {
            match rx.recv().await {
                Ok(chunk) => {
                    acc.extend_from_slice(&chunk);
                    if acc.windows(b"ACK:omw".len()).any(|w| w == b"ACK:omw") {
                        return true;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    // Lossy on overflow; keep reading.
                    continue;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return false,
            }
        }
    })
    .await
    .expect("did not see ACK:omw on the broadcast within 5s");

    assert!(
        saw_ack,
        "POST /input must reach the PTY: expected ACK:omw on the session output broadcast"
    );
}

#[tokio::test]
async fn post_input_with_invalid_base64_returns_4xx() {
    let registry = SessionRegistry::new();
    let app = router(registry);
    let (app, id) = register_session(app).await;

    // Not valid base64 — `!` is not in the standard alphabet, and the length
    // is not a multiple of 4 with appropriate padding.
    let body = serde_json::to_vec(&json!({ "bytes": "!!!not-base64!!!" })).unwrap();
    let req = Request::builder()
        .method("POST")
        .uri(format!("/internal/v1/sessions/{id}/input"))
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap();
    let resp = app
        .oneshot(req)
        .await
        .expect("router must accept POST input with malformed body");
    let status = resp.status();
    assert!(
        status.is_client_error(),
        "malformed base64 must yield a 4xx, got {status}"
    );
}
