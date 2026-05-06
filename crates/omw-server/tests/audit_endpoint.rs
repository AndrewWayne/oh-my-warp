//! Integration test for `POST /api/v1/audit/append`.
//!
//! Builds the audit_router against a tempdir-backed AuditWriter, fires
//! three POSTs, then re-reads the file and runs verify_chain to confirm
//! the on-disk state.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use omw_audit::{verify_chain, AuditWriter, GENESIS_PREV_HASH};
use omw_server::{audit_router, handlers::audit::AuditState};
use serde_json::json;
use tempfile::TempDir;
use tokio::sync::Mutex;
use tower::ServiceExt;

fn build_app(audit: AuditState) -> axum::Router {
    audit_router(audit)
}

async fn post_append(
    app: &axum::Router,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let bytes = serde_json::to_vec(&body).unwrap();
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/audit/append")
        .header("content-type", "application/json")
        .body(Body::from(bytes))
        .unwrap();
    let resp = app.clone().oneshot(req).await.expect("router accepts");
    let status = resp.status();
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let value: serde_json::Value = if body.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null)
    };
    (status, value)
}

#[tokio::test]
async fn round_trip_three_entries_and_verify() {
    let tmp = TempDir::new().unwrap();
    let writer = AuditWriter::open(tmp.path().to_path_buf()).expect("open writer");
    let path = writer.current_path();
    let audit: AuditState = Arc::new(Mutex::new(writer));
    let app = build_app(audit);

    let session = uuid::Uuid::nil();
    for i in 0..3 {
        let (status, body) = post_append(
            &app,
            json!({
                "kind": "test_event",
                "session_id": session.to_string(),
                "fields": { "i": i }
            }),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "POST {i} failed: {body:?}");
        assert!(body.get("hash").is_some(), "missing hash on POST {i}");
    }

    // Re-read the file and verify the chain.
    let head = verify_chain(&path, GENESIS_PREV_HASH).expect("clean chain");
    assert_eq!(head.len(), 64);
}

#[tokio::test]
async fn malformed_body_returns_400() {
    let tmp = TempDir::new().unwrap();
    let writer = AuditWriter::open(tmp.path().to_path_buf()).expect("open writer");
    let audit: AuditState = Arc::new(Mutex::new(writer));
    let app = build_app(audit);

    // Missing kind + session_id.
    let (status, _) = post_append(&app, json!({ "fields": { "x": 1 } })).await;
    assert!(
        status == StatusCode::UNPROCESSABLE_ENTITY || status == StatusCode::BAD_REQUEST,
        "expected 400 / 422 for missing fields, got {status}"
    );
}
