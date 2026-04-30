//! C.5 (optional) — Two WS clients see the same broadcast output stream.
//!
//! Marked `#[ignore]` because the subscription model for v0.4-thin is "every
//! connect gets the live tail". We need to think about replay semantics
//! (does a late subscriber get a backfill, or only post-connect frames?)
//! before pinning the contract here.
//!
//! Beyond-v1 cleanup: revisit replay / backpressure semantics.

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
            "name": "ws-multi",
            "command": "powershell",
            "args": ["-NoProfile", "-Command", script],
        })
    } else {
        json!({
            "name": "ws-multi",
            "command": "sh",
            "args": [
                "-c",
                "stty -echo; while IFS= read -r line; do printf 'ACK:%s\\n' \"$line\"; done",
            ],
        })
    }
}

async fn spawn_server() -> std::net::SocketAddr {
    let registry = SessionRegistry::new();
    let app = router(registry);
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app.into_make_service()).await;
    });
    addr
}

async fn register(addr: std::net::SocketAddr) -> String {
    let stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
    let (mut sender, conn) = hyper::client::conn::http1::handshake::<_, http_body_util::Full<bytes::Bytes>>(
        hyper_util::rt::TokioIo::new(stream),
    )
    .await
    .expect("handshake");
    tokio::spawn(async move {
        let _ = conn.await;
    });
    let body = serde_json::to_vec(&ack_loop_body()).unwrap();
    let req = hyper::Request::builder()
        .method("POST")
        .uri(format!("http://{addr}/internal/v1/sessions"))
        .header("host", format!("{addr}"))
        .header("content-type", "application/json")
        .body(http_body_util::Full::new(bytes::Bytes::from(body)))
        .expect("build req");
    let resp = sender.send_request(req).await.expect("send");
    assert!(resp.status().is_success());
    let body = http_body_util::BodyExt::collect(resp.into_body())
        .await
        .expect("collect")
        .to_bytes();
    let v: Value = serde_json::from_slice(&body).expect("json");
    v.get("id")
        .and_then(Value::as_str)
        .expect("id")
        .to_string()
}

#[tokio::test]
#[ignore = "Beyond-v1: replay / broadcast semantics not yet pinned for v0.4-thin"]
async fn two_ws_clients_see_same_output() {
    let addr = spawn_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    let id = register(addr).await;

    let ws_url = format!("ws://{addr}/internal/v1/sessions/{id}/pty");
    let (mut ws_a, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("ws_a connect");
    let (mut ws_b, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("ws_b connect");

    tokio::time::sleep(Duration::from_millis(200)).await;

    // Send via one client; both clients must observe the ACK.
    ws_a.send(Message::Binary(b"shared\n".to_vec()))
        .await
        .expect("send");

    let saw_a = timeout(Duration::from_secs(5), drain_until(&mut ws_a, b"ACK:shared"))
        .await
        .expect("client A timed out");
    let saw_b = timeout(Duration::from_secs(5), drain_until(&mut ws_b, b"ACK:shared"))
        .await
        .expect("client B timed out");

    assert!(saw_a, "client A must observe ACK:shared");
    assert!(saw_b, "client B must observe ACK:shared");

    let _ = ws_a.close(None).await;
    let _ = ws_b.close(None).await;
}

async fn drain_until<S>(ws: &mut S, needle: &[u8]) -> bool
where
    S: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
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
            _ => {}
        }
        if acc.windows(needle.len()).any(|w| w == needle) {
            return true;
        }
    }
    false
}
