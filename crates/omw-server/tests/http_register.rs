//! C.2 — Contract test for the HTTP register / list / get / delete surface.
//!
//! Uses `tower::ServiceExt::oneshot` to drive the assembled `axum::Router`
//! in-process, no real socket binding. Verifies:
//!   - POST  /internal/v1/sessions returns 200 or 201 + `{ id, name, ... }`
//!   - GET   /internal/v1/sessions includes the just-registered session
//!   - GET   /internal/v1/sessions/:id returns 200 + matching metadata
//!   - GET   /internal/v1/sessions/<bogus> returns 404
//!   - DELETE /internal/v1/sessions/:id returns 200/204
//!   - GET   /internal/v1/sessions/:id after delete returns 404 OR alive=false
//!
//! `register_lists_gets_and_deletes` uses a short-lived `echo`/`printf` child,
//! which exercises the natural-exit + DELETE-after-exit path. The
//! `delete_kills_live_session` test below uses a long-lived `sleep`/`timeout`
//! child to prove DELETE actually kills a session that is still running,
//! distinct from auto-eviction of an already-exited child.

use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

use omw_server::{router, SessionRegistry};

fn quick_spec_json() -> Value {
    if cfg!(windows) {
        json!({
            "name": "http-quick",
            "command": "cmd",
            "args": ["/c", "echo hello"],
        })
    } else {
        json!({
            "name": "http-quick",
            "command": "sh",
            "args": ["-c", "printf hello\\n"],
        })
    }
}

async fn body_to_json(body: Body) -> Value {
    let bytes = body.collect().await.expect("collect body").to_bytes();
    serde_json::from_slice(&bytes).expect("valid JSON body")
}

fn assert_uuid_shape(s: &str) {
    // 8-4-4-4-12 hex (36 chars total).
    assert_eq!(s.len(), 36, "uuid must be 36 chars: got {s:?}");
    let segments: Vec<&str> = s.split('-').collect();
    assert_eq!(
        segments.len(),
        5,
        "uuid must have 5 dash-separated segments: {s:?}"
    );
    assert_eq!(segments[0].len(), 8);
    assert_eq!(segments[1].len(), 4);
    assert_eq!(segments[2].len(), 4);
    assert_eq!(segments[3].len(), 4);
    assert_eq!(segments[4].len(), 12);
    for seg in segments {
        assert!(
            seg.chars().all(|c| c.is_ascii_hexdigit()),
            "uuid segment must be hex: {seg:?}"
        );
    }
}

#[tokio::test]
async fn register_lists_gets_and_deletes() {
    let registry = SessionRegistry::new();
    let app = router(registry);

    // --- POST /internal/v1/sessions ---
    let body = serde_json::to_vec(&quick_spec_json()).unwrap();
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
        .expect("router must accept POST /sessions");
    let status = resp.status();
    assert!(
        status == StatusCode::CREATED || status == StatusCode::OK,
        "expected 200 or 201 from POST /sessions, got {status}"
    );
    let body = body_to_json(resp.into_body()).await;
    let id = body
        .get("id")
        .and_then(Value::as_str)
        .expect("response must include `id` as string");
    assert_uuid_shape(id);
    assert_eq!(
        body.get("name").and_then(Value::as_str),
        Some("http-quick"),
        "response must echo the name"
    );
    assert!(
        body.get("created_at").is_some(),
        "response must include created_at"
    );

    // --- GET /internal/v1/sessions ---
    let req = Request::builder()
        .method("GET")
        .uri("/internal/v1/sessions")
        .body(Body::empty())
        .unwrap();
    let resp = app
        .clone()
        .oneshot(req)
        .await
        .expect("router must accept GET /sessions");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_to_json(resp.into_body()).await;
    let sessions = body
        .get("sessions")
        .and_then(Value::as_array)
        .expect("response must be {sessions: [...]}");
    assert!(
        sessions
            .iter()
            .any(|s| s.get("id").and_then(Value::as_str) == Some(id)),
        "list must include the just-registered id {id}"
    );

    // --- GET /internal/v1/sessions/:id ---
    let req = Request::builder()
        .method("GET")
        .uri(format!("/internal/v1/sessions/{id}"))
        .body(Body::empty())
        .unwrap();
    let resp = app
        .clone()
        .oneshot(req)
        .await
        .expect("router must accept GET /sessions/:id");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_to_json(resp.into_body()).await;
    assert_eq!(body.get("id").and_then(Value::as_str), Some(id));
    assert_eq!(body.get("name").and_then(Value::as_str), Some("http-quick"));

    // --- GET /internal/v1/sessions/<bogus> ---
    let bogus = "00000000-0000-0000-0000-000000000000";
    let req = Request::builder()
        .method("GET")
        .uri(format!("/internal/v1/sessions/{bogus}"))
        .body(Body::empty())
        .unwrap();
    let resp = app
        .clone()
        .oneshot(req)
        .await
        .expect("router must accept GET /sessions/<bogus>");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "GET /sessions/<unknown-id> must return 404"
    );

    // --- DELETE /internal/v1/sessions/:id ---
    let req = Request::builder()
        .method("DELETE")
        .uri(format!("/internal/v1/sessions/{id}"))
        .body(Body::empty())
        .unwrap();
    let resp = app
        .clone()
        .oneshot(req)
        .await
        .expect("router must accept DELETE /sessions/:id");
    let status = resp.status();
    assert!(
        status == StatusCode::OK || status == StatusCode::NO_CONTENT,
        "expected 200 or 204 from DELETE /sessions/:id, got {status}"
    );

    // --- GET after DELETE: 404 OR alive=false ---
    let req = Request::builder()
        .method("GET")
        .uri(format!("/internal/v1/sessions/{id}"))
        .body(Body::empty())
        .unwrap();
    let resp = app
        .clone()
        .oneshot(req)
        .await
        .expect("router must still accept GET after delete");
    let status = resp.status();
    if status == StatusCode::OK {
        let body = body_to_json(resp.into_body()).await;
        assert_eq!(
            body.get("alive").and_then(Value::as_bool),
            Some(false),
            "after DELETE, GET must return alive=false (or 404)"
        );
    } else {
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "after DELETE, GET must return 404 or 200+alive=false"
        );
    }
}

fn long_lived_spec_json() -> Value {
    if cfg!(windows) {
        // `timeout /t 60 /nobreak` blocks for 60 seconds without consuming
        // input, run via `cmd /c` so it inherits the PTY cleanly.
        json!({
            "name": "http-sleep",
            "command": "cmd",
            "args": ["/c", "timeout", "/t", "60", "/nobreak"],
        })
    } else {
        json!({
            "name": "http-sleep",
            "command": "sh",
            "args": ["-c", "sleep 60"],
        })
    }
}

#[tokio::test]
async fn delete_kills_live_session() {
    // DELETE on a long-lived child must kill it. This is distinct from the
    // short-lived case (which natural-exits and may auto-evict on its own):
    // here the child would still be running 60s from now, so any "gone after
    // DELETE" observation has to be caused by DELETE itself.
    let registry = SessionRegistry::new();
    let app = router(registry);

    // --- POST /internal/v1/sessions (long-lived) ---
    let body = serde_json::to_vec(&long_lived_spec_json()).unwrap();
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
        .expect("router must accept POST /sessions");
    let status = resp.status();
    assert!(
        status == StatusCode::CREATED || status == StatusCode::OK,
        "expected 200 or 201 from POST /sessions, got {status}"
    );
    let body = body_to_json(resp.into_body()).await;
    let id = body
        .get("id")
        .and_then(Value::as_str)
        .expect("response must include `id` as string")
        .to_string();

    // Give the child ~200ms to actually spawn before we ask about its state.
    tokio::time::sleep(Duration::from_millis(200)).await;

    // --- GET pre-DELETE: must show alive=true (live session, not yet exited) ---
    let req = Request::builder()
        .method("GET")
        .uri(format!("/internal/v1/sessions/{id}"))
        .body(Body::empty())
        .unwrap();
    let resp = app
        .clone()
        .oneshot(req)
        .await
        .expect("router must accept GET /sessions/:id");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "GET pre-DELETE must succeed for a live session"
    );
    let body = body_to_json(resp.into_body()).await;
    assert_eq!(
        body.get("alive").and_then(Value::as_bool),
        Some(true),
        "long-lived child must still be alive ~200ms after spawn"
    );

    // --- DELETE /internal/v1/sessions/:id ---
    let req = Request::builder()
        .method("DELETE")
        .uri(format!("/internal/v1/sessions/{id}"))
        .body(Body::empty())
        .unwrap();
    let resp = app
        .clone()
        .oneshot(req)
        .await
        .expect("router must accept DELETE /sessions/:id");
    let status = resp.status();
    assert!(
        status == StatusCode::OK || status == StatusCode::NO_CONTENT,
        "expected 200 or 204 from DELETE /sessions/:id, got {status}"
    );

    // Give kill propagation a moment.
    tokio::time::sleep(Duration::from_millis(500)).await;

    // --- GET post-DELETE: 404 OR alive=false ---
    // Tolerate either auto-evict (404) or the alive-flag flip; both are valid
    // outcomes per the registry surface. What we are NOT tolerating is the
    // session still showing alive=true, which would mean DELETE didn't kill.
    let req = Request::builder()
        .method("GET")
        .uri(format!("/internal/v1/sessions/{id}"))
        .body(Body::empty())
        .unwrap();
    let resp = app
        .clone()
        .oneshot(req)
        .await
        .expect("router must still accept GET after delete");
    let status = resp.status();
    if status == StatusCode::OK {
        let body = body_to_json(resp.into_body()).await;
        assert_eq!(
            body.get("alive").and_then(Value::as_bool),
            Some(false),
            "after DELETE, GET must return alive=false (or 404); the long-lived child must NOT still be running"
        );
    } else {
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "after DELETE, GET must return 404 or 200+alive=false"
        );
    }
}

#[tokio::test]
async fn register_with_malformed_json_returns_4xx() {
    let registry = SessionRegistry::new();
    let app = router(registry);

    let req = Request::builder()
        .method("POST")
        .uri("/internal/v1/sessions")
        .header("content-type", "application/json")
        .body(Body::from("{ this is not valid json"))
        .unwrap();

    let resp = app
        .oneshot(req)
        .await
        .expect("router must accept POST /sessions even with malformed body");
    let status = resp.status();
    assert!(
        status.is_client_error(),
        "malformed JSON must be a 4xx, got {status}"
    );
}
