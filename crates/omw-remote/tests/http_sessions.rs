//! Tests for `/api/v1/sessions`. Covers signed-request enforcement +
//! cap-scope checks + create/list/delete round-trip.

#[path = "http_common/mod.rs"]
mod http_common;

use std::time::Duration;

use omw_remote::Capability;
use serde_json::Value;

use http_common::{http_request, pair_device, sign_headers, spawn_server};

#[tokio::test]
async fn create_session_signed_succeeds() {
    let f = spawn_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    let (device, cap_b64, device_id) = pair_device(
        &f.pairings,
        &f.host,
        11,
        &[Capability::PtyRead, Capability::PtyWrite],
    );
    let body = b"{\"name\":\"main\"}".to_vec();
    let device_priv = device.to_bytes();
    let headers = sign_headers(
        "POST",
        "/api/v1/sessions",
        &body,
        &cap_b64,
        &device_id,
        &device_priv,
        "nonce-create-1",
    );

    let (status, body) = http_request(f.addr, "POST", "/api/v1/sessions", body, &headers).await;
    assert_eq!(status, 200, "expected 200; got {status}; body={body:?}");
    let v: Value = serde_json::from_slice(&body).expect("valid JSON");
    let id = v["id"].as_str().expect("id string");
    uuid::Uuid::parse_str(id).expect("id is a UUID");
    assert_eq!(v["name"], "main");
    assert!(v["created_at"].is_string());

    // Cleanup.
    let uuid = uuid::Uuid::parse_str(id).unwrap();
    let _ = f.registry.kill(uuid).await;
}

#[tokio::test]
async fn create_session_unsigned_fails_401() {
    let f = spawn_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    let (status, _body) = http_request(
        f.addr,
        "POST",
        "/api/v1/sessions",
        b"{}".to_vec(),
        &[("content-type", "application/json".to_string())],
    )
    .await;
    assert_eq!(status, 401, "unsigned create must be 401");
}

#[tokio::test]
async fn create_session_with_readonly_cap_fails_403() {
    let f = spawn_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Read-only token: PtyRead alone, no PtyWrite.
    let (device, cap_b64, device_id) =
        pair_device(&f.pairings, &f.host, 12, &[Capability::PtyRead]);
    let body = b"{}".to_vec();
    let device_priv = device.to_bytes();
    let headers = sign_headers(
        "POST",
        "/api/v1/sessions",
        &body,
        &cap_b64,
        &device_id,
        &device_priv,
        "nonce-readonly-1",
    );
    let (status, _body) = http_request(f.addr, "POST", "/api/v1/sessions", body, &headers).await;
    assert_eq!(status, 403, "readonly cap must be 403; got {status}");
}

#[tokio::test]
async fn list_sessions_returns_created() {
    let f = spawn_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    let (device, cap_b64, device_id) = pair_device(
        &f.pairings,
        &f.host,
        13,
        &[Capability::PtyRead, Capability::PtyWrite],
    );
    let device_priv = device.to_bytes();

    // Create.
    let create_body = b"{\"name\":\"alpha\"}".to_vec();
    let create_headers = sign_headers(
        "POST",
        "/api/v1/sessions",
        &create_body,
        &cap_b64,
        &device_id,
        &device_priv,
        "nonce-list-create",
    );
    let (cs, cb) = http_request(
        f.addr,
        "POST",
        "/api/v1/sessions",
        create_body,
        &create_headers,
    )
    .await;
    assert_eq!(cs, 200, "create status; body={cb:?}");
    let cv: Value = serde_json::from_slice(&cb).unwrap();
    let id = cv["id"].as_str().unwrap().to_string();

    // List.
    let list_headers = sign_headers(
        "GET",
        "/api/v1/sessions",
        b"",
        &cap_b64,
        &device_id,
        &device_priv,
        "nonce-list-list",
    );
    let (ls, lb) = http_request(f.addr, "GET", "/api/v1/sessions", vec![], &list_headers).await;
    assert_eq!(ls, 200, "list status");
    let lv: Value = serde_json::from_slice(&lb).unwrap();
    let sessions = lv["sessions"].as_array().expect("sessions array");
    assert!(
        sessions.iter().any(|s| s["id"] == id),
        "list must contain just-created id; got {sessions:?}"
    );

    // Cleanup.
    let uuid = uuid::Uuid::parse_str(&id).unwrap();
    let _ = f.registry.kill(uuid).await;
}

#[tokio::test]
async fn delete_session_kills() {
    let f = spawn_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    let (device, cap_b64, device_id) = pair_device(
        &f.pairings,
        &f.host,
        14,
        &[Capability::PtyRead, Capability::PtyWrite],
    );
    let device_priv = device.to_bytes();

    // Create.
    let create_body = b"{\"name\":\"to-delete\"}".to_vec();
    let create_headers = sign_headers(
        "POST",
        "/api/v1/sessions",
        &create_body,
        &cap_b64,
        &device_id,
        &device_priv,
        "nonce-delete-create",
    );
    let (cs, cb) = http_request(
        f.addr,
        "POST",
        "/api/v1/sessions",
        create_body,
        &create_headers,
    )
    .await;
    assert_eq!(cs, 200);
    let cv: Value = serde_json::from_slice(&cb).unwrap();
    let id = cv["id"].as_str().unwrap().to_string();
    let uuid = uuid::Uuid::parse_str(&id).unwrap();

    // Delete.
    let delete_path = format!("/api/v1/sessions/{id}");
    let delete_headers = sign_headers(
        "DELETE",
        &delete_path,
        b"",
        &cap_b64,
        &device_id,
        &device_priv,
        "nonce-delete-del",
    );
    let (ds, _) = http_request(f.addr, "DELETE", &delete_path, vec![], &delete_headers).await;
    assert_eq!(ds, 204, "delete must be 204");

    // Confirm registry no longer has it.
    assert!(
        f.registry.get(uuid).is_none(),
        "registry must drop the deleted session"
    );
}
