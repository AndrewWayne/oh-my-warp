//! C.4 — End-to-end WebSocket PTY round-trip.
//!
//! Binds the assembled `omw-server` router on `127.0.0.1:0` (OS-assigned
//! port), registers an ACK-transform session via HTTP, then connects a
//! tokio-tungstenite client to `ws://…/internal/v1/sessions/:id/pty` and
//! verifies the server forwards bytes both ways:
//!
//!   client --binary("omw\n")--> server --PTY input--> child
//!   child  --PTY output--> server --binary("ACK:omw\r\n")--> client
//!
//! Asserts `ACK:omw` appears in the inbound stream within 5 seconds.
//!
//! This is the only test in this phase that uses a real TCP listener; the
//! HTTP-only contract tests use `tower::ServiceExt::oneshot` to avoid
//! binding a port.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;

use omw_server::{router, SessionRegistry};

fn ack_loop_body() -> Value {
    if cfg!(windows) {
        let script = "while ($true) { $line = Read-Host; if ($null -eq $line) { break }; Write-Host ('ACK:' + $line) }";
        json!({
            "name": "ws-ack",
            "command": "powershell",
            "args": ["-NoProfile", "-Command", script],
        })
    } else {
        json!({
            "name": "ws-ack",
            "command": "sh",
            "args": [
                "-c",
                "stty -echo; while IFS= read -r line; do printf 'ACK:%s\\n' \"$line\"; done",
            ],
        })
    }
}

/// Spawn the omw-server router on `127.0.0.1:0` and return the bound address.
async fn spawn_server() -> std::net::SocketAddr {
    let registry = SessionRegistry::new();
    let app = router(registry);

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind 127.0.0.1:0");
    let addr = listener.local_addr().expect("local_addr");

    tokio::spawn(async move {
        // axum::serve runs forever; the test exits when the test process
        // tears down, which drops the runtime and aborts this task.
        let _ = axum::serve(listener, app.into_make_service()).await;
    });

    addr
}

/// Register a session over HTTP and return its id.
async fn register_via_http(addr: std::net::SocketAddr) -> String {
    let url = format!("http://{addr}/internal/v1/sessions");
    let body = serde_json::to_vec(&ack_loop_body()).unwrap();

    // Use a hand-rolled hyper client to avoid pulling reqwest just for one
    // POST. The test is a thin contract check; we don't need a full HTTP
    // client surface.
    let stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
    let (mut sender, conn) = hyper::client::conn::http1::handshake::<
        _,
        http_body_util::Full<bytes::Bytes>,
    >(hyper_util::rt::TokioIo::new(stream))
    .await
    .expect("hyper handshake");
    tokio::spawn(async move {
        let _ = conn.await;
    });

    let req = hyper::Request::builder()
        .method("POST")
        .uri(url)
        .header("host", format!("{addr}"))
        .header("content-type", "application/json")
        .body(http_body_util::Full::new(bytes::Bytes::from(body)))
        .expect("build request");

    let resp = sender.send_request(req).await.expect("send POST /sessions");
    assert!(
        resp.status().is_success(),
        "POST /sessions must succeed, got {}",
        resp.status()
    );
    let body_bytes = http_body_util::BodyExt::collect(resp.into_body())
        .await
        .expect("collect body")
        .to_bytes();
    let v: Value = serde_json::from_slice(&body_bytes).expect("valid json body");
    v.get("id")
        .and_then(Value::as_str)
        .expect("response must include id")
        .to_string()
}

#[tokio::test]
async fn ws_round_trips_pty_bytes() {
    // Without hyper-util in dev-deps we'd need a different POST path; gate
    // the test instead and build a leaner harness if Executor changes the
    // server-startup path.
    let addr = spawn_server().await;

    // Give the server a moment to be ready to accept.
    tokio::time::sleep(Duration::from_millis(50)).await;

    let id = register_via_http(addr).await;

    let ws_url = format!("ws://{addr}/internal/v1/sessions/{id}/pty");
    let (mut ws, _resp) = timeout(
        Duration::from_secs(5),
        tokio_tungstenite::connect_async(&ws_url),
    )
    .await
    .expect("WS connect timeout")
    .expect("WS connect failed");

    // Give the child a moment to apply `stty -echo` (Unix) before we send.
    tokio::time::sleep(Duration::from_millis(200)).await;

    // On Windows, PowerShell `Read-Host` running on ConPTY only treats CRLF
    // as a line terminator — bare LF leaves the line buffered and the child
    // never echoes back. Unix `read -r` is happy with LF alone.
    const INPUT_LINE: &[u8] = if cfg!(windows) { b"omw\r\n" } else { b"omw\n" };
    ws.send(Message::Binary(INPUT_LINE.to_vec()))
        .await
        .expect("send binary frame");

    // Drain output frames until we see "ACK:omw" or timeout.
    let saw_ack = timeout(Duration::from_secs(5), async {
        let mut acc = Vec::<u8>::new();
        while let Some(msg) = ws.next().await {
            let msg = match msg {
                Ok(m) => m,
                Err(_) => return false,
            };
            match msg {
                Message::Binary(b) => acc.extend_from_slice(&b),
                Message::Text(t) => acc.extend_from_slice(t.as_bytes()),
                Message::Close(_) => return false,
                Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => {}
            }
            if acc.windows(b"ACK:omw".len()).any(|w| w == b"ACK:omw") {
                return true;
            }
        }
        false
    })
    .await
    .expect("did not see ACK:omw within 5s");

    assert!(
        saw_ack,
        "expected to see ACK:omw on the WS output stream within 5s"
    );

    // Close cleanly.
    let _ = ws.close(None).await;
}

#[tokio::test]
async fn ws_unknown_id_is_rejected() {
    let addr = spawn_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    let bogus = "00000000-0000-0000-0000-000000000000";
    let ws_url = format!("ws://{addr}/internal/v1/sessions/{bogus}/pty");

    let result = timeout(
        Duration::from_secs(5),
        tokio_tungstenite::connect_async(&ws_url),
    )
    .await
    .expect("WS connect attempt should not hang");

    // Either the upgrade fails outright (Err) or it succeeds and the server
    // closes immediately. Both are acceptable signals that an unknown id is
    // not a valid PTY subscription target.
    match result {
        Err(_) => {
            // upgrade rejected — fine.
        }
        Ok((mut ws, _)) => {
            let next = timeout(Duration::from_secs(2), ws.next())
                .await
                .expect("server must close promptly for unknown id");
            match next {
                None => {
                    // stream ended — fine.
                }
                Some(Ok(Message::Close(_))) => {
                    // explicit close — fine.
                }
                Some(Err(_)) => {
                    // I/O error on read — fine.
                }
                Some(Ok(other)) => {
                    panic!("unknown id must not deliver data frames; got {other:?}");
                }
            }
        }
    }
}
