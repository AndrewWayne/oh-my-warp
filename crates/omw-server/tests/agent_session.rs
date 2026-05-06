//! Phase 2 integration tests for the omw-server agent surface.
//!
//! Spawns the mock omw-agent stdio fixture (`tests/fixtures/mock-omw-agent.mjs`),
//! binds the agent router on a random localhost port, and exercises the
//! HTTP create + WS event-stream surfaces end to end.
//!
//! Skipped (with a printed warning) if `node` is not on `$PATH` so CI
//! agents without Node still pass.

use std::path::PathBuf;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use omw_server::{agent_router, AgentProcess, AgentProcessConfig};
use serde_json::{json, Value};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

/// Synthetic sessionId the mock fixture publishes inbound-frame echoes on.
/// Kept in sync with `tests/fixtures/mock-omw-agent.mjs`.
const MOCK_CONTROL_SESSION: &str = "__mock_control__";

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

/// Test harness wrapping the bound HTTP/WS server address.
struct Server {
    addr: std::net::SocketAddr,
}

impl Server {
    fn url(&self, path: &str) -> String {
        format!("http://{}{}", self.addr, path)
    }

    fn ws_url(&self, path: &str) -> String {
        format!("ws://{}{}", self.addr, path)
    }
}

/// Test-side handle to the mock kernel. Exposes accessors that wait for
/// inbound JSON-RPC frames the fixture observed (via the
/// `kernel/received` echo notification on the synthetic mock-control bus).
struct MockHandle {
    rx: broadcast::Receiver<Value>,
}

impl MockHandle {
    /// Wait for and return the next `session/create` request the kernel
    /// received. Returns the full echoed frame summary
    /// (`{ method, params, hasId, sessionId }`).
    async fn next_session_create(&mut self) -> Value {
        self.next_kernel_request("session/create").await
    }

    /// Wait for and return the next inbound frame whose method matches
    /// `method`. Other observed frames are skipped.
    async fn next_kernel_request(&mut self, method: &str) -> Value {
        let deadline = Duration::from_secs(5);
        loop {
            let frame = timeout(deadline, self.rx.recv())
                .await
                .expect("mock observation timeout")
                .expect("mock observation channel closed");
            if frame["method"] != "kernel/received" {
                continue;
            }
            if frame["params"]["method"] == method {
                return frame["params"].clone();
            }
        }
    }
}

/// Spawn the mock kernel + omw-server agent router on a random port.
async fn spawn_server_and_mock_agent() -> (Server, MockHandle) {
    let cfg = AgentProcessConfig {
        command: "node".into(),
        args: vec![mock_fixture_path().to_string_lossy().into_owned()],
    };
    let agent = AgentProcess::spawn(cfg).await.expect("agent spawn failed");
    // Subscribe to the mock-control bus *before* binding the router so we
    // never miss an early observation echo.
    let rx = agent.subscribe(MOCK_CONTROL_SESSION);
    let app = agent_router(agent);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app.into_make_service()).await;
    });
    // Yield so the listener accepts before the first client connects.
    tokio::time::sleep(Duration::from_millis(20)).await;
    (Server { addr }, MockHandle { rx })
}

/// POST a JSON body to `/api/v1/agent/sessions` and return the parsed
/// response. Asserts a 201 status.
async fn http_create_session(server: &Server, body: Value) -> Value {
    let stream = TcpStream::connect(server.addr).await.expect("tcp connect");
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
        .uri(server.url("/api/v1/agent/sessions"))
        .header("host", format!("{}", server.addr))
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

/// Convenience: create a session with default provider config and return
/// its sessionId.
async fn create_test_session(server: &Server) -> String {
    let body = json!({
        "providerConfig": { "kind": "openai-compatible", "key_ref": "omw/test", "base_url": "http://example" },
        "model": "test-model"
    });
    let created = http_create_session(server, body).await;
    created["sessionId"]
        .as_str()
        .expect("sessionId")
        .to_string()
}

/// Thin wrapper around a tungstenite WebSocket exposing a `send_json`
/// helper.
struct WsClient {
    inner: WebSocketStream<MaybeTlsStream<TcpStream>>,
}

impl WsClient {
    async fn send_json(&mut self, value: &Value) {
        let line = serde_json::to_string(value).unwrap();
        self.inner.send(WsMessage::Text(line)).await.unwrap();
    }

    async fn next_text(&mut self) -> Option<String> {
        loop {
            let msg = self.inner.next().await?.ok()?;
            match msg {
                WsMessage::Text(t) => return Some(t),
                WsMessage::Binary(_) | WsMessage::Ping(_) | WsMessage::Pong(_) => continue,
                WsMessage::Close(_) | WsMessage::Frame(_) => return None,
            }
        }
    }

    async fn close(mut self) {
        let _ = self.inner.close(None).await;
    }
}

async fn connect_ws(server: &Server, session_id: &str) -> WsClient {
    let url = server.ws_url(&format!("/ws/v1/agent/{session_id}"));
    let (inner, _resp) = timeout(Duration::from_secs(5), tokio_tungstenite::connect_async(&url))
        .await
        .expect("WS connect timeout")
        .expect("WS connect failed");
    WsClient { inner }
}

#[tokio::test]
async fn round_trip_prompt() {
    if !node_available() {
        eprintln!("node not on PATH — skipping agent_session::round_trip_prompt");
        return;
    }
    let (server, _mock) = spawn_server_and_mock_agent().await;

    let session_id = create_test_session(&server).await;
    assert!(!session_id.is_empty());

    let mut ws = connect_ws(&server, &session_id).await;
    ws.send_json(&json!({ "kind": "prompt", "prompt": "say hi" }))
        .await;

    let mut deltas: Vec<String> = Vec::new();
    let mut got_finished = false;
    while !got_finished {
        let text = timeout(Duration::from_secs(5), ws.next_text())
            .await
            .expect("ws timeout")
            .expect("ws closed");
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
    ws.close().await;
}

#[tokio::test]
async fn ws_disconnect_does_not_kill_session() {
    if !node_available() {
        eprintln!("node not on PATH — skipping");
        return;
    }
    let (server, _mock) = spawn_server_and_mock_agent().await;
    let session_id = create_test_session(&server).await;

    // First WS — connect and immediately close. Kernel should stay alive.
    let ws = connect_ws(&server, &session_id).await;
    drop(ws);
    tokio::time::sleep(Duration::from_millis(80)).await;

    // Reconnect and prompt — the kernel should still be alive (single
    // shared process), and the new WS should observe the full lifecycle.
    let mut ws = connect_ws(&server, &session_id).await;
    ws.send_json(&json!({ "kind": "prompt", "prompt": "again" }))
        .await;

    let mut got_finished = false;
    while !got_finished {
        let text = timeout(Duration::from_secs(5), ws.next_text())
            .await
            .expect("ws timeout after reconnect")
            .expect("ws closed after reconnect");
        let frame: Value = serde_json::from_str(&text).expect("JSON");
        if frame["method"] == "turn/finished" {
            got_finished = true;
        }
    }
    ws.close().await;
}

#[tokio::test]
async fn unknown_session_ws_connects_then_closes() {
    if !node_available() {
        eprintln!("node not on PATH — skipping");
        return;
    }
    let (server, _mock) = spawn_server_and_mock_agent().await;

    let ws = connect_ws(&server, "no-such-session").await;
    ws.close().await;
}

#[tokio::test]
async fn session_create_forwards_provider_config_to_kernel() {
    if !node_available() {
        eprintln!("node not on PATH — skipping");
        return;
    }
    let (server, mut mock) = spawn_server_and_mock_agent().await;

    let body = json!({
        "providerConfig": {
            "kind": "openai",
            "key_ref": "keychain:omw/test",
        },
        "model": "gpt-4o",
    });

    let resp = http_create_session(&server, body).await;
    assert!(resp["sessionId"].as_str().is_some());

    let received = mock.next_session_create().await;
    assert_eq!(received["params"]["providerConfig"]["kind"], "openai");
    assert_eq!(
        received["params"]["providerConfig"]["key_ref"],
        "keychain:omw/test"
    );
    assert_eq!(received["params"]["model"], "gpt-4o");
}

#[tokio::test]
async fn session_create_forwards_policy_mode_to_kernel() {
    if !node_available() {
        eprintln!("node not on PATH — skipping");
        return;
    }
    let (server, mut mock) = spawn_server_and_mock_agent().await;

    let body = json!({
        "providerConfig": { "kind": "openai", "key_ref": "keychain:omw/test" },
        "model": "gpt-4o",
        "policy": { "mode": "trusted" },
    });

    let _ = http_create_session(&server, body).await;

    let received = mock.next_session_create().await;
    assert_eq!(received["params"]["policy"]["mode"], "trusted");
}

#[tokio::test]
async fn ws_translates_approval_decision_to_kernel_request() {
    if !node_available() {
        eprintln!("node not on PATH — skipping");
        return;
    }
    let (server, mut mock) = spawn_server_and_mock_agent().await;
    let session_id = create_test_session(&server).await;
    // Drain the session/create observation so the next match is the
    // approval/decide we're about to send.
    let _ = mock.next_session_create().await;

    let mut ws = connect_ws(&server, &session_id).await;
    ws.send_json(&json!({
        "kind": "approval_decision",
        "approvalId": "abc123",
        "decision": "approve",
    }))
    .await;

    let received = mock.next_kernel_request("approval/decide").await;
    assert_eq!(received["params"]["sessionId"], session_id);
    assert_eq!(received["params"]["approvalId"], "abc123");
    assert_eq!(received["params"]["decision"], "approve");
    ws.close().await;
}
