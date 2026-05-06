//! Phase 2 integration tests for the omw-server agent surface.
//!
//! Spawns the mock omw-agent stdio fixture (`tests/fixtures/mock-omw-agent.mjs`),
//! binds the agent router on a random localhost port, and exercises the
//! HTTP create + WS event-stream surfaces end to end.
//!
//! Skipped (with a printed warning) if `node` is not on `$PATH` so CI
//! agents without Node still pass.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use omw_server::{agent_router, AgentProcess, AgentProcessConfig};
use serde_json::{json, Value};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message as WsMessage;

fn mock_fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("mock-omw-agent.mjs")
}

fn node_available() -> bool {
    std::process::Command::new("node")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

async fn spawn_agent_with_mock() -> Arc<AgentProcess> {
    let cfg = AgentProcessConfig {
        command: "node".into(),
        args: vec![mock_fixture_path().to_string_lossy().into_owned()],
    };
    AgentProcess::spawn(cfg).await.expect("agent spawn failed")
}

async fn bind_router(agent: Arc<AgentProcess>) -> std::net::SocketAddr {
    let app = agent_router(agent);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app.into_make_service()).await;
    });
    // Yield so the listener accepts before the first client connects.
    tokio::time::sleep(Duration::from_millis(20)).await;
    addr
}

/// POST a JSON body to the agent surface using a hand-rolled hyper client
/// (the existing tests prefer this over pulling in reqwest).
async fn http_create_session(addr: std::net::SocketAddr, body: Value) -> Value {
    let stream = TcpStream::connect(addr).await.expect("tcp connect");
    let (mut sender, conn) = hyper::client::conn::http1::handshake::<
        _,
        http_body_util::Full<bytes::Bytes>,
    >(hyper_util::rt::TokioIo::new(stream))
    .await
    .expect("hyper handshake");
    tokio::spawn(async move {
        let _ = conn.await;
    });
    let req_body = serde_json::to_vec(&body).unwrap();
    let req = hyper::Request::builder()
        .method("POST")
        .uri(format!("http://{addr}/api/v1/agent/sessions"))
        .header("host", format!("{addr}"))
        .header("content-type", "application/json")
        .body(http_body_util::Full::new(bytes::Bytes::from(req_body)))
        .unwrap();
    let resp = sender.send_request(req).await.expect("POST send");
    assert_eq!(
        resp.status().as_u16(),
        201,
        "expected 201 Created, got {}",
        resp.status()
    );
    let body_bytes = http_body_util::BodyExt::collect(resp.into_body())
        .await
        .unwrap()
        .to_bytes();
    serde_json::from_slice(&body_bytes).expect("response is JSON")
}

#[tokio::test]
async fn round_trip_prompt() {
    if !node_available() {
        eprintln!("node not on PATH — skipping agent_session::round_trip_prompt");
        return;
    }
    let agent = spawn_agent_with_mock().await;
    let addr = bind_router(agent).await;

    let body = json!({
        "providerConfig": { "kind": "openai-compatible", "key_ref": "omw/test", "base_url": "http://example" },
        "model": "test-model"
    });
    let created = http_create_session(addr, body).await;
    let session_id = created["sessionId"].as_str().expect("sessionId").to_string();
    assert!(!session_id.is_empty());

    let ws_url = format!("ws://{addr}/ws/v1/agent/{session_id}");
    let (mut ws, _resp) = timeout(
        Duration::from_secs(5),
        tokio_tungstenite::connect_async(&ws_url),
    )
    .await
    .expect("WS connect timeout")
    .expect("WS connect failed");

    let prompt = json!({ "kind": "prompt", "prompt": "say hi" }).to_string();
    ws.send(WsMessage::Text(prompt)).await.unwrap();

    let mut deltas: Vec<String> = Vec::new();
    let mut got_finished = false;
    while !got_finished {
        let msg = timeout(Duration::from_secs(5), ws.next())
            .await
            .expect("ws timeout")
            .expect("ws closed")
            .expect("ws error");
        let text = match msg {
            WsMessage::Text(t) => t,
            WsMessage::Binary(_) | WsMessage::Ping(_) | WsMessage::Pong(_) => continue,
            other => panic!("unexpected ws frame: {other:?}"),
        };
        let frame: Value = serde_json::from_str(&text).expect("ws frame is JSON");
        match frame["method"].as_str().unwrap_or("") {
            "assistant/delta" => {
                deltas.push(frame["params"]["delta"].as_str().unwrap_or("").to_string());
            }
            "turn/finished" => {
                assert_eq!(frame["params"]["sessionId"], session_id);
                assert_eq!(frame["params"]["cancelled"], false);
                got_finished = true;
            }
            _ => {}
        }
    }
    assert!(!deltas.is_empty(), "should have at least one delta");
    let joined: String = deltas.concat();
    assert!(joined.contains("Hello"), "deltas should contain 'Hello': {joined}");
    let _ = ws.close(None).await;
}

#[tokio::test]
async fn ws_disconnect_does_not_kill_session() {
    if !node_available() {
        eprintln!("node not on PATH — skipping");
        return;
    }
    let agent = spawn_agent_with_mock().await;
    let addr = bind_router(agent.clone()).await;

    let body = json!({
        "providerConfig": { "kind": "openai-compatible", "key_ref": "omw/test", "base_url": "http://example" },
        "model": "test-model"
    });
    let created = http_create_session(addr, body).await;
    let session_id = created["sessionId"].as_str().unwrap().to_string();

    // First WS — connect and immediately close. Kernel should stay alive.
    let ws_url = format!("ws://{addr}/ws/v1/agent/{session_id}");
    let (ws, _) = timeout(
        Duration::from_secs(5),
        tokio_tungstenite::connect_async(&ws_url),
    )
    .await
    .expect("WS connect timeout")
    .expect("WS connect failed");
    drop(ws);
    tokio::time::sleep(Duration::from_millis(80)).await;

    // Reconnect and prompt — the kernel should still be alive (single
    // shared process), and the new WS should observe the full lifecycle.
    let (mut ws, _) = timeout(
        Duration::from_secs(5),
        tokio_tungstenite::connect_async(&ws_url),
    )
    .await
    .expect("WS reconnect timeout")
    .expect("WS reconnect failed");
    let prompt = json!({ "kind": "prompt", "prompt": "again" }).to_string();
    ws.send(WsMessage::Text(prompt)).await.unwrap();

    let mut got_finished = false;
    while !got_finished {
        let msg = timeout(Duration::from_secs(5), ws.next())
            .await
            .expect("ws timeout after reconnect")
            .expect("ws closed after reconnect")
            .expect("ws error after reconnect");
        if let WsMessage::Text(text) = msg {
            let frame: Value = serde_json::from_str(&text).expect("JSON");
            if frame["method"] == "turn/finished" {
                got_finished = true;
            }
        }
    }
    let _ = ws.close(None).await;
}

#[tokio::test]
async fn unknown_session_ws_connects_then_closes() {
    if !node_available() {
        eprintln!("node not on PATH — skipping");
        return;
    }
    let agent = spawn_agent_with_mock().await;
    let addr = bind_router(agent).await;

    let ws_url = format!("ws://{addr}/ws/v1/agent/no-such-session");
    let (mut ws, _) = timeout(
        Duration::from_secs(5),
        tokio_tungstenite::connect_async(&ws_url),
    )
    .await
    .expect("WS connect timeout")
    .expect("WS connect failed");
    let _ = ws.close(None).await;
}
