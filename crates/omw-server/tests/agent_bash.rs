//! Phase 5a — L3b round-trip tests for the bash broker.
//!
//! Spawns the mock omw-agent fixture (`tests/fixtures/mock-omw-agent.mjs`),
//! binds the agent router on a random port, and exercises the bash routing
//! paths end-to-end:
//!
//!   1. `bash/exec` from kernel → `kind: "exec_command"` text frame on the
//!      GUI WebSocket scoped to the matching `terminalSessionId`.
//!   2. `kind: "command_data"` from the GUI → `bash/data` notification
//!      forwarded verbatim to the kernel.
//!   3. `kind: "command_exit"` from the GUI → `bash/finished` notification
//!      forwarded to the kernel with `exitCode` preserved.
//!   4. Two concurrent `bash/exec` frames with distinct `commandId`s
//!      are routed independently — interleaved replies match by id.
//!   5. `bash/exec` for a `terminalSessionId` with no live GUI bus →
//!      synthetic `bash/finished { snapshot: true }` reply back to the
//!      kernel, so the in-flight tool call never hangs past its timeout.
//!
//! Each test skips with a printed warning when `node` is missing so CI
//! agents without Node still pass — same convention as `agent_session.rs`.
//!
//! Helpers are duplicated from `agent_session.rs` to keep this file
//! self-contained; extracting a shared `tests/common/mod.rs` is deferred
//! to a follow-up that touches both files at once.

use std::collections::HashSet;
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

/// Test-side handle to the mock kernel. Holds the AgentProcess so the
/// `__test/emit__` injection method can reach the mock, plus a receiver on
/// the synthetic mock-control bus for asserting on inbound-frame echoes.
struct MockHandle {
    rx: broadcast::Receiver<Value>,
    agent: std::sync::Arc<AgentProcess>,
}

impl MockHandle {
    /// Inject a kernel-side notification by asking the mock fixture to
    /// re-emit it on stdout. Round-trips through the JSON-RPC stdio bridge,
    /// which is the same path real notifications take.
    async fn emit_notification(&self, method: &str, params: Value) {
        self.agent
            .send_method(
                "__test/emit__",
                json!({ "method": method, "params": params }),
            )
            .await
            .expect("__test/emit__ failed");
    }

    /// Wait for the next inbound JSON-RPC notification (no id) the kernel
    /// observed whose method matches `method`. Skips request echoes and
    /// non-matching frames.
    async fn next_kernel_notification(&mut self, method: &str) -> Value {
        let deadline = Duration::from_secs(5);
        loop {
            let frame = timeout(deadline, self.rx.recv())
                .await
                .expect("mock observation timeout")
                .expect("mock observation channel closed");
            if frame["method"] != "kernel/received" {
                continue;
            }
            if frame["params"]["hasId"].as_bool() == Some(true) {
                continue;
            }
            if frame["params"]["method"] == method {
                return frame["params"].clone();
            }
        }
    }
}

async fn spawn_server_and_mock_agent() -> (Server, MockHandle) {
    let cfg = AgentProcessConfig {
        command: "node".into(),
        args: vec![mock_fixture_path().to_string_lossy().into_owned()],
    };
    let agent = AgentProcess::spawn(cfg).await.expect("agent spawn failed");
    let rx = agent.subscribe(MOCK_CONTROL_SESSION);
    let app = agent_router(agent.clone());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app.into_make_service()).await;
    });
    tokio::time::sleep(Duration::from_millis(20)).await;
    (Server { addr }, MockHandle { rx, agent })
}

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

async fn create_test_session(server: &Server) -> String {
    let body = json!({
        "providerConfig": { "kind": "openai-compatible", "key_ref": "omw/test", "base_url": "http://example" },
        "model": "test-model"
    });
    let created = http_create_session(server, body).await;
    created["sessionId"].as_str().expect("sessionId").to_string()
}

struct WsClient {
    inner: WebSocketStream<MaybeTlsStream<TcpStream>>,
}

impl WsClient {
    async fn send_json(&mut self, value: &Value) {
        let line = serde_json::to_string(value).unwrap();
        self.inner.send(WsMessage::Text(line)).await.unwrap();
    }

    /// Read the next text frame, skip notifications whose `kind` matches a
    /// caller-supplied filter, and return the first payload that matches.
    /// Used to filter out unrelated frames the GUI bus may receive
    /// (e.g. legacy JSON-RPC forwarded notifications) so concurrency tests
    /// can pin on `exec_command` specifically.
    async fn next_kind(&mut self, kind: &str) -> Value {
        let deadline = Duration::from_secs(5);
        loop {
            let msg = timeout(deadline, self.inner.next())
                .await
                .expect("ws frame timeout")
                .expect("ws closed")
                .expect("ws error");
            let text = match msg {
                WsMessage::Text(t) => t,
                WsMessage::Binary(_) | WsMessage::Ping(_) | WsMessage::Pong(_) => continue,
                WsMessage::Close(_) | WsMessage::Frame(_) => panic!("ws closed"),
            };
            let parsed: Value = serde_json::from_str(&text).expect("ws frame is JSON");
            if parsed["kind"] == kind {
                return parsed;
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
async fn bash_exec_notification_forwarded_as_exec_command_to_gui() {
    if !node_available() {
        eprintln!("node not on PATH — skipping");
        return;
    }
    let (server, mock) = spawn_server_and_mock_agent().await;
    let session_id = create_test_session(&server).await;
    let mut ws = connect_ws(&server, &session_id).await;
    // Yield once so the WS subscription fully registers before we inject.
    tokio::time::sleep(Duration::from_millis(40)).await;

    mock.emit_notification(
        "bash/exec",
        json!({
            "commandId": "cmd-1",
            "command": "ls",
            "cwd": "/tmp",
            "terminalSessionId": session_id,
            "agentSessionId": session_id,
            "toolCallId": "tc-1",
        }),
    )
    .await;

    let frame = ws.next_kind("exec_command").await;
    assert_eq!(frame["commandId"], "cmd-1");
    assert_eq!(frame["command"], "ls");
    assert_eq!(frame["cwd"], "/tmp");
    ws.close().await;
}

#[tokio::test]
async fn command_data_from_gui_forwarded_as_bash_data_to_kernel() {
    if !node_available() {
        eprintln!("node not on PATH — skipping");
        return;
    }
    let (server, mut mock) = spawn_server_and_mock_agent().await;
    let session_id = create_test_session(&server).await;
    let mut ws = connect_ws(&server, &session_id).await;
    tokio::time::sleep(Duration::from_millis(40)).await;

    ws.send_json(&json!({
        "kind": "command_data",
        "commandId": "cmd-1",
        "bytes": "hello\n",
    }))
    .await;

    let received = mock.next_kernel_notification("bash/data").await;
    assert_eq!(received["params"]["commandId"], "cmd-1");
    assert_eq!(received["params"]["bytes"], "hello\n");
    ws.close().await;
}

#[tokio::test]
async fn command_exit_from_gui_forwarded_as_bash_finished_to_kernel() {
    if !node_available() {
        eprintln!("node not on PATH — skipping");
        return;
    }
    let (server, mut mock) = spawn_server_and_mock_agent().await;
    let session_id = create_test_session(&server).await;
    let mut ws = connect_ws(&server, &session_id).await;
    tokio::time::sleep(Duration::from_millis(40)).await;

    ws.send_json(&json!({
        "kind": "command_exit",
        "commandId": "cmd-1",
        "exitCode": 0,
    }))
    .await;

    let received = mock.next_kernel_notification("bash/finished").await;
    assert_eq!(received["params"]["commandId"], "cmd-1");
    assert_eq!(received["params"]["exitCode"], 0);
    ws.close().await;
}

#[tokio::test]
async fn concurrent_bash_calls_routed_by_command_id() {
    if !node_available() {
        eprintln!("node not on PATH — skipping");
        return;
    }
    let (server, mut mock) = spawn_server_and_mock_agent().await;
    let session_id = create_test_session(&server).await;
    let mut ws = connect_ws(&server, &session_id).await;
    tokio::time::sleep(Duration::from_millis(40)).await;

    // Inject two bash/exec notifications back-to-back. The broker must
    // route both to the GUI keyed by terminalSessionId without coalescing.
    mock.emit_notification(
        "bash/exec",
        json!({
            "commandId": "cmd-A",
            "command": "ls",
            "cwd": "/tmp",
            "terminalSessionId": session_id,
        }),
    )
    .await;
    mock.emit_notification(
        "bash/exec",
        json!({
            "commandId": "cmd-B",
            "command": "pwd",
            "cwd": "/tmp",
            "terminalSessionId": session_id,
        }),
    )
    .await;

    let frame1 = ws.next_kind("exec_command").await;
    let frame2 = ws.next_kind("exec_command").await;
    let mut received_ids: HashSet<String> = HashSet::new();
    received_ids.insert(frame1["commandId"].as_str().unwrap().to_string());
    received_ids.insert(frame2["commandId"].as_str().unwrap().to_string());
    assert!(
        received_ids.contains("cmd-A"),
        "expected cmd-A in {received_ids:?}"
    );
    assert!(
        received_ids.contains("cmd-B"),
        "expected cmd-B in {received_ids:?}"
    );

    // Reply for B first, then A. The kernel must observe each by its own id
    // — no crossing of state.
    ws.send_json(&json!({
        "kind": "command_exit",
        "commandId": "cmd-B",
        "exitCode": 0,
    }))
    .await;
    let r1 = mock.next_kernel_notification("bash/finished").await;
    assert_eq!(r1["params"]["commandId"], "cmd-B");

    ws.send_json(&json!({
        "kind": "command_exit",
        "commandId": "cmd-A",
        "exitCode": 0,
    }))
    .await;
    let r2 = mock.next_kernel_notification("bash/finished").await;
    assert_eq!(r2["params"]["commandId"], "cmd-A");
    ws.close().await;
}

#[tokio::test]
async fn bash_exec_with_no_active_gui_returns_snapshot_finished() {
    if !node_available() {
        eprintln!("node not on PATH — skipping");
        return;
    }
    let (server, mut mock) = spawn_server_and_mock_agent().await;
    // Create a session but never connect a WS for it.
    let _session_id = create_test_session(&server).await;

    mock.emit_notification(
        "bash/exec",
        json!({
            "commandId": "cmd-1",
            "command": "ls",
            "cwd": "/tmp",
            "terminalSessionId": "no-such-terminal",
            "agentSessionId": "no-such-terminal",
            "toolCallId": "tc-1",
        }),
    )
    .await;

    let received = mock.next_kernel_notification("bash/finished").await;
    assert_eq!(received["params"]["commandId"], "cmd-1");
    assert_eq!(received["params"]["snapshot"], true);
}
