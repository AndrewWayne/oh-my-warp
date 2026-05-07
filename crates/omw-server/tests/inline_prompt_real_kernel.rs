//! End-to-end reducer test for the `# hello`-prefix flow.
//!
//! What this reproduces: the GUI's `OmwAgentState::start_with_config` →
//! `run_session` → `POST /api/v1/agent/sessions` round trip, but driven
//! from a plain Rust test (no GPUI / no terminal pane). The test spawns:
//!
//!   - the **real** apps/omw-agent kernel (`apps/omw-agent/bin/omw-agent.mjs`)
//!     via `AgentProcess::spawn` — same code path `omw_inproc_server::boot`
//!     uses inside warp-oss.
//!   - the agent router on a random localhost port — same code path the
//!     in-process server registers.
//!
//! Then it issues the exact JSON body the GUI sends in
//! `omw_agent_state.rs::create_session`:
//!
//!   ```json
//!   {
//!     "providerConfig": { "kind": "openai", "key_ref": "keychain:omw/openai-prod" },
//!     "model": "gpt-5.5",
//!     "policy": { "mode": "ask_before_write" }
//!   }
//!   ```
//!
//! and prints both the HTTP status and the raw response body. If the
//! GUI's 502 reproduces here, we have a deterministic local repro that
//! does NOT require launching the full Warp app. If it doesn't reproduce,
//! the bug is GUI-specific (e.g. a different body, a different runtime
//! configuration, or kernel state pollution from prior runs).
//!
//! Run with:
//!
//!     cargo test -p omw-server --test inline_prompt_real_kernel \
//!         -- --nocapture --test-threads=1
//!
//! The `--nocapture` is important — we use `println!` to dump the
//! kernel's response body so it's visible without grepping a logfile.

use std::path::PathBuf;
use std::time::Duration;

use omw_server::{agent_router, AgentProcess, AgentProcessConfig};
use serde_json::{json, Value};
use tokio::net::{TcpListener, TcpStream};

/// Locate the real omw-agent kernel script. Walks up from CARGO_MANIFEST_DIR
/// (which is `crates/omw-server/`) until it finds `apps/omw-agent/bin/omw-agent.mjs`.
fn real_kernel_path() -> Option<PathBuf> {
    let mut current = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for _ in 0..6 {
        let candidate = current
            .join("apps")
            .join("omw-agent")
            .join("bin")
            .join("omw-agent.mjs");
        if candidate.exists() {
            return Some(candidate);
        }
        if !current.pop() {
            return None;
        }
    }
    None
}

fn node_available() -> bool {
    std::process::Command::new("node")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Spawn the real kernel + agent_router on a random port. Returns the
/// bound address.
async fn spawn_real_kernel_server() -> std::net::SocketAddr {
    let kernel = real_kernel_path().expect("apps/omw-agent/bin/omw-agent.mjs not found");
    println!("[reducer-test] kernel script: {}", kernel.display());
    let cfg = AgentProcessConfig {
        command: "node".into(),
        args: vec![
            kernel.to_string_lossy().into_owned(),
            "--serve-stdio".into(),
        ],
        env: Vec::new(),
    };
    let agent = AgentProcess::spawn(cfg).await.expect("agent spawn failed");
    println!("[reducer-test] kernel spawned");
    let app = agent_router(agent);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    println!("[reducer-test] agent_router bound at {addr}");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app.into_make_service()).await;
    });
    // Yield so the listener accepts before the first client connects.
    tokio::time::sleep(Duration::from_millis(50)).await;
    addr
}

/// POST a JSON body to /api/v1/agent/sessions and return (status, body_text).
async fn post_session(addr: std::net::SocketAddr, body: Value) -> (u16, String) {
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
    println!(
        "[reducer-test] POST /api/v1/agent/sessions body={}",
        String::from_utf8_lossy(&req_body)
    );
    let req = hyper::Request::builder()
        .method("POST")
        .uri(format!("http://{addr}/api/v1/agent/sessions"))
        .header("host", format!("{addr}"))
        .header("content-type", "application/json")
        .body(http_body_util::Full::new(bytes::Bytes::from(req_body)))
        .unwrap();
    let resp = sender.send_request(req).await.expect("POST send");
    let status = resp.status().as_u16();
    let body_bytes = http_body_util::BodyExt::collect(resp.into_body())
        .await
        .unwrap()
        .to_bytes();
    let body_text = String::from_utf8_lossy(&body_bytes).into_owned();
    println!("[reducer-test] response status={status} body={body_text}");
    (status, body_text)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn inline_prompt_round_trip_real_kernel() {
    if !node_available() {
        eprintln!("[reducer-test] skipping: node not on PATH");
        return;
    }
    if real_kernel_path().is_none() {
        eprintln!("[reducer-test] skipping: real kernel script not found");
        return;
    }

    let addr = spawn_real_kernel_server().await;

    // Exact body shape the GUI sends in
    // `vendor/warp-stripped/app/src/ai_assistant/omw_agent_state.rs::create_session`:
    //
    //   provider_kind=openai, key_ref=Some("keychain:omw/openai-prod"),
    //   base_url=None, model="gpt-5.5", system_prompt=None, cwd=None,
    //   approval_mode=Some("ask_before_write")
    //
    // The Rust side serialises it as:
    let body = json!({
        "providerConfig": {
            "kind": "openai",
            "key_ref": "keychain:omw/openai-prod",
        },
        "model": "gpt-5.5",
        "policy": { "mode": "ask_before_write" },
    });

    let (status, body_text) = post_session(addr, body).await;

    // Test passes either way — the print output above is the diagnostic.
    // If we got 201 with a sessionId, the bug is GUI-specific.
    // If we got 502 here too, we've reproduced it locally and can iterate.
    println!("[reducer-test] === RESULT ===");
    println!("[reducer-test] status={status}");
    println!("[reducer-test] body  ={body_text}");
    if status == 201 {
        let parsed: Value =
            serde_json::from_str(&body_text).expect("201 response should be JSON");
        println!(
            "[reducer-test] sessionId = {:?}",
            parsed.get("sessionId").and_then(|v| v.as_str())
        );
    } else {
        println!(
            "[reducer-test] non-201 — captures whatever the kernel returned that the omw-server \
             couldn't interpret as a sessionId"
        );
    }
}

/// Sanity test: minimal body (just kind + model) — does the kernel
/// require the `policy` field, or is it optional? We compare the GUI's
/// "with policy" body against this minimal one.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn inline_prompt_minimal_body() {
    if !node_available() || real_kernel_path().is_none() {
        eprintln!("[reducer-test] skipping: node or kernel missing");
        return;
    }
    let addr = spawn_real_kernel_server().await;
    let body = json!({
        "providerConfig": { "kind": "openai", "key_ref": "keychain:omw/openai-prod" },
        "model": "gpt-5.5",
    });
    let (status, body_text) = post_session(addr, body).await;
    println!("[reducer-test minimal] status={status} body={body_text}");
}

/// Sanity test: openai-compatible kind (the kind the existing
/// agent_session.rs tests use) — to confirm the kernel is alive at all
/// and the bug is openai-specific.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn inline_prompt_openai_compatible_baseline() {
    if !node_available() || real_kernel_path().is_none() {
        eprintln!("[reducer-test] skipping: node or kernel missing");
        return;
    }
    let addr = spawn_real_kernel_server().await;
    let body = json!({
        "providerConfig": {
            "kind": "openai-compatible",
            "key_ref": "omw/test",
            "base_url": "http://example",
        },
        "model": "test-model",
    });
    let (status, body_text) = post_session(addr, body).await;
    println!("[reducer-test baseline] status={status} body={body_text}");
}
