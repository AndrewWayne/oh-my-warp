//! Reproduce the GUI's `# hi` 502 bug at a layer we can iterate on.
//!
//! The simple "spawn fresh kernel + one POST" test (`inline_prompt_real_kernel.rs`)
//! returns 201 every time — the GUI hits 502 every time. Difference is what
//! the GUI does that the simple test doesn't:
//!
//!   - Long-lived AgentProcess + long-lived `agent_router` shared across
//!     many session/create POSTs.
//!   - GUI retries: each `# hi` calls `OmwAgentState::start` which
//!     `stop()`s the previous WS task — including aborting the previous
//!     POST mid-flight if it hadn't completed.
//!   - The kernel may have leftover state (sessions map, pending
//!     approvals, agent runtime).
//!
//! Each test in this file mirrors one of those patterns. The first one
//! that 502s is the bug. Run with:
//!
//!     cargo test -p omw-server --test inline_prompt_repro \
//!         -- --nocapture --test-threads=1

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use omw_server::{agent_router, AgentProcess, AgentProcessConfig};
use serde_json::{json, Value};
use tokio::net::{TcpListener, TcpStream};

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

/// GUI-equivalent body — exact shape `OmwAgentState::create_session` builds.
fn gui_body() -> Value {
    json!({
        "providerConfig": {
            "kind": "openai",
            "key_ref": "keychain:omw/openai-prod",
        },
        "model": "gpt-5.5",
        "policy": { "mode": "ask_before_write" },
    })
}

/// Spawn the real kernel + agent_router on a random port. Returns the
/// bound address AND a clone of the AgentProcess so tests that need to
/// drive multiple POSTs can share state.
async fn spawn_real_kernel_server() -> (std::net::SocketAddr, Arc<AgentProcess>) {
    let kernel = real_kernel_path().expect("kernel not found");
    let cfg = AgentProcessConfig {
        command: "node".into(),
        args: vec![
            kernel.to_string_lossy().into_owned(),
            "--serve-stdio".into(),
        ],
        env: Vec::new(),
    };
    let agent = AgentProcess::spawn(cfg).await.expect("agent spawn failed");
    let app = agent_router(agent.clone());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app.into_make_service()).await;
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    (addr, agent)
}

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
    (status, String::from_utf8_lossy(&body_bytes).into_owned())
}

/// Pattern 1: 5 sequential POSTs against the same long-lived router.
/// Mirrors the user clicking `# hi` five times in a row.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sequential_posts_same_kernel() {
    if !node_available() || real_kernel_path().is_none() {
        eprintln!("[repro] skipping: node or kernel missing");
        return;
    }
    let (addr, _agent) = spawn_real_kernel_server().await;

    let mut results = Vec::new();
    for i in 0..5 {
        let (status, body) = post_session(addr, gui_body()).await;
        println!("[repro:seq] attempt {i}: status={status} body={body}");
        results.push((status, body));
    }

    let bad: Vec<_> = results.iter().filter(|(s, _)| *s != 201).collect();
    if !bad.is_empty() {
        panic!(
            "REPRODUCED — {} of 5 sequential POSTs returned non-201: {:?}",
            bad.len(),
            bad
        );
    }
    println!("[repro:seq] all 5 returned 201 — sequential pattern does NOT repro");
}

/// Pattern 2: 5 concurrent POSTs racing against each other.
/// JSON-RPC ids are allocated atomically but responses must route by
/// id — concurrent calls stress that mapping.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_posts_same_kernel() {
    if !node_available() || real_kernel_path().is_none() {
        eprintln!("[repro] skipping: node or kernel missing");
        return;
    }
    let (addr, _agent) = spawn_real_kernel_server().await;

    let mut tasks = Vec::new();
    for i in 0..5 {
        let task = tokio::spawn(async move {
            let (status, body) = post_session(addr, gui_body()).await;
            (i, status, body)
        });
        tasks.push(task);
    }
    let mut results = Vec::new();
    for t in tasks {
        results.push(t.await.expect("task join"));
    }

    for (i, status, body) in &results {
        println!("[repro:concurrent] {i}: status={status} body={body}");
    }
    let bad: Vec<_> = results.iter().filter(|(_, s, _)| *s != 201).collect();
    if !bad.is_empty() {
        panic!(
            "REPRODUCED — {} of 5 concurrent POSTs returned non-201: {:?}",
            bad.len(),
            bad
        );
    }
    println!("[repro:concurrent] all 5 returned 201 — concurrent pattern does NOT repro");
}

/// Pattern 3: cancel-mid-flight + retry, simulating GUI's
/// start→stop→start cycle. We start a POST, drop the future before it
/// completes (mimicking `tokio::JoinHandle::abort` on the run_session
/// task), then issue a fresh POST. The kernel may have already received
/// the first request and queued a response that arrives AFTER the
/// pending map has dropped its waiter.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancel_mid_flight_then_retry() {
    if !node_available() || real_kernel_path().is_none() {
        eprintln!("[repro] skipping: node or kernel missing");
        return;
    }
    let (addr, _agent) = spawn_real_kernel_server().await;

    // Issue and immediately abort. We use a tokio task so we can `abort()`
    // it before its body finishes.
    let task = tokio::spawn(async move {
        post_session(addr, gui_body()).await
    });
    // Yield very briefly so the request has time to actually go on the
    // wire and reach the kernel's stdin.
    tokio::time::sleep(Duration::from_millis(5)).await;
    task.abort();
    let _ = task.await;
    println!("[repro:abort] aborted first request");

    // Drain any orphan response: give the kernel a moment to finish its
    // first session/create and write the response (which the now-dropped
    // waiter will discard) before we try again.
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Now retry — this is what the user's second `# hi` looks like.
    let (status, body) = post_session(addr, gui_body()).await;
    println!("[repro:abort] retry: status={status} body={body}");
    if status != 201 {
        panic!("REPRODUCED — retry after aborted POST returned status={status} body={body}");
    }
    println!("[repro:abort] retry succeeded — abort pattern does NOT repro");
}

/// Pattern 4: 10 cancel→retry cycles in rapid succession.
/// Builds up a backlog of orphaned kernel responses that may interfere
/// with later requests.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn many_cancel_retry_cycles() {
    if !node_available() || real_kernel_path().is_none() {
        eprintln!("[repro] skipping: node or kernel missing");
        return;
    }
    let (addr, _agent) = spawn_real_kernel_server().await;

    for cycle in 0..10 {
        let task = tokio::spawn(async move {
            post_session(addr, gui_body()).await
        });
        tokio::time::sleep(Duration::from_millis(2)).await;
        task.abort();
        let _ = task.await;
        // Don't drain — let orphan responses pile up.
    }
    println!("[repro:burst] dropped 10 in-flight POSTs");

    tokio::time::sleep(Duration::from_millis(100)).await;

    let (status, body) = post_session(addr, gui_body()).await;
    println!("[repro:burst] final POST: status={status} body={body}");
    if status != 201 {
        panic!("REPRODUCED — final POST after 10 aborts returned status={status} body={body}");
    }
    println!("[repro:burst] final POST succeeded — burst-cancel pattern does NOT repro");
}

/// Pattern 5b: GUI's exact failing sequence — must pass once the kernel
/// is hardened so spawn ENOENT doesn't crash the Node process. With or
/// without `OMW_KEYCHAIN_HELPER` set, the kernel must survive a missing
/// helper and return an `error` notification so subsequent
/// session/create POSTs still succeed.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn second_attempt_after_prompt_502() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message as WsMessage;

    if !node_available() || real_kernel_path().is_none() {
        eprintln!("[repro] skipping: node or kernel missing");
        return;
    }
    let (addr, _agent) = spawn_real_kernel_server().await;

    // ─── Attempt 1 ───────────────────────────────────────────────
    let (status, body) = post_session(addr, gui_body()).await;
    println!("[repro:2nd] attempt-1 POST status={status} body={body}");
    assert_eq!(status, 201, "first POST should succeed");
    let parsed: Value = serde_json::from_str(&body).unwrap();
    let session_id = parsed["sessionId"].as_str().unwrap().to_string();

    let ws_url = format!("ws://{addr}/ws/v1/agent/{session_id}");
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("ws connect");
    println!("[repro:2nd] attempt-1 WS connected");

    ws.send(WsMessage::Text(
        json!({"kind": "prompt", "prompt": "hi"}).to_string(),
    ))
    .await
    .unwrap();
    println!("[repro:2nd] attempt-1 prompt frame sent");

    // Drain WS for up to 2s — collect any error/turn_finished
    // notifications the kernel emits in response to the prompt. This
    // mimics the run_session loop reading from WS.
    let drain_deadline = std::time::Instant::now() + Duration::from_secs(2);
    while std::time::Instant::now() < drain_deadline {
        match tokio::time::timeout(Duration::from_millis(200), ws.next()).await {
            Ok(Some(Ok(WsMessage::Text(t)))) => {
                println!("[repro:2nd] attempt-1 ws<- {t}");
            }
            Ok(Some(Ok(_))) | Ok(Some(Err(_))) | Ok(None) => break,
            Err(_) => continue, // timeout — keep draining
        }
    }
    println!("[repro:2nd] drained WS; closing");
    let _ = ws.close(None).await;
    drop(ws);
    tokio::time::sleep(Duration::from_millis(200)).await;

    // ─── Attempt 2 ───────────────────────────────────────────────
    let (status2, body2) = post_session(addr, gui_body()).await;
    println!("[repro:2nd] attempt-2 POST status={status2} body={body2}");
    if status2 != 201 {
        panic!(
            "REPRODUCED — second POST after first prompt returned status={status2} body={body2}"
        );
    }
    println!("[repro:2nd] both attempts succeeded — pattern does NOT repro");
}

/// Pattern 5c: same as 5b but with OMW_KEYCHAIN_HELPER pointed at a real
/// helper binary. Proves the fix lands: when the helper is reachable,
/// the kernel doesn't crash on session/prompt and subsequent POSTs
/// succeed.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn second_attempt_with_helper_env_passes() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message as WsMessage;

    if !node_available() || real_kernel_path().is_none() {
        eprintln!("[repro] skipping: node or kernel missing");
        return;
    }
    // Locate the freshly-built helper. Skip the test if it isn't built —
    // CI without `cargo build -p omw-keychain-helper --release` should
    // still pass.
    let helper_path = {
        let mut current = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let mut found = None;
        for _ in 0..6 {
            for profile in ["release", "debug"] {
                let cand = current
                    .join("target")
                    .join(profile)
                    .join("omw-keychain-helper");
                if cand.exists() {
                    found = Some(cand);
                    break;
                }
            }
            if found.is_some() {
                break;
            }
            if !current.pop() {
                break;
            }
        }
        match found {
            Some(p) => p,
            None => {
                eprintln!(
                    "[repro] skipping: omw-keychain-helper not built (cargo build -p omw-keychain-helper)"
                );
                return;
            }
        }
    };
    println!("[repro:fixed] OMW_KEYCHAIN_HELPER -> {}", helper_path.display());

    let kernel = real_kernel_path().expect("kernel not found");
    let cfg = AgentProcessConfig {
        command: "node".into(),
        args: vec![
            kernel.to_string_lossy().into_owned(),
            "--serve-stdio".into(),
        ],
        env: vec![(
            "OMW_KEYCHAIN_HELPER".into(),
            helper_path.to_string_lossy().into_owned(),
        )],
    };
    let agent = AgentProcess::spawn(cfg).await.expect("agent spawn failed");
    let app = agent_router(agent.clone());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app.into_make_service()).await;
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    let _agent = agent;

    // Attempt 1
    let (status, body) = post_session(addr, gui_body()).await;
    assert_eq!(status, 201, "attempt-1 POST should succeed: body={body}");
    let parsed: Value = serde_json::from_str(&body).unwrap();
    let session_id = parsed["sessionId"].as_str().unwrap().to_string();
    println!("[repro:fixed] attempt-1 sessionId={session_id}");

    let ws_url = format!("ws://{addr}/ws/v1/agent/{session_id}");
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("ws connect");
    ws.send(WsMessage::Text(
        json!({"kind": "prompt", "prompt": "hi"}).to_string(),
    ))
    .await
    .unwrap();

    // Drain WS for ~2s. With the helper reachable but the keychain entry
    // empty, we expect the kernel to emit error+turn_finished
    // notifications (NOT agent/crashed).
    let drain_deadline = std::time::Instant::now() + Duration::from_secs(2);
    let mut saw_crashed = false;
    let mut saw_error_or_turn = false;
    while std::time::Instant::now() < drain_deadline {
        match tokio::time::timeout(Duration::from_millis(200), ws.next()).await {
            Ok(Some(Ok(WsMessage::Text(t)))) => {
                println!("[repro:fixed] attempt-1 ws<- {t}");
                if t.contains("agent/crashed") {
                    saw_crashed = true;
                }
                if t.contains("\"method\":\"error\"") || t.contains("turn/finished") {
                    saw_error_or_turn = true;
                }
            }
            Ok(Some(Ok(_))) | Ok(Some(Err(_))) | Ok(None) => break,
            Err(_) => continue,
        }
    }
    let _ = ws.close(None).await;
    drop(ws);
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Attempt 2 — this is what 502s without the fix.
    let (status2, body2) = post_session(addr, gui_body()).await;
    println!("[repro:fixed] attempt-2 status={status2} body={body2}");

    assert!(
        !saw_crashed,
        "kernel still emits agent/crashed even with OMW_KEYCHAIN_HELPER set — \
         helper resolution didn't take effect"
    );
    assert_eq!(
        status2, 201,
        "attempt-2 should succeed once kernel survives (saw_error_or_turn={saw_error_or_turn}); body={body2}"
    );
    println!("[repro:fixed] FIX VERIFIED — both attempts succeeded with helper env injected");
}

/// Pattern 5: full OmwAgentState::start workflow — POST, then connect WS,
/// then send prompt — repeated.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn full_start_cycle_repeated() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message as WsMessage;

    if !node_available() || real_kernel_path().is_none() {
        eprintln!("[repro] skipping: node or kernel missing");
        return;
    }
    let (addr, _agent) = spawn_real_kernel_server().await;

    for cycle in 0..3 {
        let (status, body) = post_session(addr, gui_body()).await;
        println!("[repro:full {cycle}] POST status={status} body={body}");
        if status != 201 {
            panic!(
                "REPRODUCED at cycle {cycle} — POST returned status={status} body={body}"
            );
        }
        let parsed: Value = serde_json::from_str(&body).unwrap();
        let session_id = parsed["sessionId"].as_str().unwrap().to_string();

        let ws_url = format!("ws://{addr}/ws/v1/agent/{session_id}");
        let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url)
            .await
            .expect("ws connect");
        println!("[repro:full {cycle}] WS connected to {session_id}");

        // Send a prompt the way the GUI does (`{kind: "prompt", prompt: ...}`).
        ws.send(WsMessage::Text(
            json!({"kind": "prompt", "prompt": "say hi"}).to_string(),
        ))
        .await
        .unwrap();
        println!("[repro:full {cycle}] prompt sent; closing WS");

        // Don't wait for the assistant — close immediately, like the GUI
        // does on retry.
        let _ = ws.close(None).await;
        let _ = ws.next().await;
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    println!("[repro:full] 3 cycles complete — full-cycle pattern does NOT repro");
}
