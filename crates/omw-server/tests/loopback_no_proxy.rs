//! Regression test: the GUI's reqwest client used to honor system /
//! env proxy config (`https_proxy=http://127.0.0.1:6789`, macOS
//! Network panel proxies, …). On developer machines running local
//! HTTP proxies (Clash, Telegram-style), this re-routed the
//! `POST http://127.0.0.1:8788/api/v1/agent/sessions` to the proxy,
//! which couldn't proxy localhost-to-localhost and returned 502.
//! See the long comment on `create_session()` in
//! `vendor/warp-stripped/app/src/ai_assistant/omw_agent_state.rs`.
//!
//! This test simulates a hostile proxy: it points `HTTPS_PROXY` (and
//! `HTTP_PROXY`) at an unreachable port, then verifies that a reqwest
//! client built with `.no_proxy()` *still* reaches localhost
//! successfully — i.e. our fix is durable against any proxy env var.
//! Without `.no_proxy()` the test 502s / connection-refuses, exactly
//! mirroring the GUI bug.
//!
//! Run with:
//!     cargo test -p omw-server --test loopback_no_proxy \
//!         -- --nocapture --test-threads=1

use std::path::PathBuf;
use std::time::Duration;

use omw_server::{agent_router, AgentProcess, AgentProcessConfig};
use serde_json::{json, Value};
use tokio::net::TcpListener;

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

async fn spawn_real_kernel_server() -> std::net::SocketAddr {
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
    let app = agent_router(agent);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app.into_make_service()).await;
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    addr
}

fn gui_body() -> Value {
    json!({
        "providerConfig": { "kind": "openai", "key_ref": "keychain:omw/openai-prod" },
        "model": "gpt-5.5",
        "policy": { "mode": "ask_before_write" },
    })
}

/// Bind a port to a TCP listener that immediately closes every
/// connection — a stand-in for a misbehaving local HTTP proxy that
/// returns 502 / connection-refused for whatever it can't actually
/// proxy. We point `HTTPS_PROXY` at this port to simulate the user's
/// reported environment.
async fn spawn_hostile_proxy() -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (sock, _) = match listener.accept().await {
                Ok(p) => p,
                Err(_) => break,
            };
            // Close immediately; reqwest sees this as a proxy error.
            drop(sock);
        }
    });
    tokio::time::sleep(Duration::from_millis(20)).await;
    addr
}

/// Bound *guard* for the env vars: tests within this binary share the
/// process env, so we can't run in parallel with each other (test 1
/// would leak HTTPS_PROXY into test 2). The Drop restores the prior
/// values. Call sites use `--test-threads=1` (set in this file's
/// header comment) so the guard's atomicity holds.
struct EnvGuard {
    keys: Vec<(&'static str, Option<String>)>,
}
impl EnvGuard {
    fn set(pairs: &[(&'static str, &str)]) -> Self {
        let prior: Vec<_> = pairs
            .iter()
            .map(|(k, _)| (*k, std::env::var(k).ok()))
            .collect();
        for (k, v) in pairs {
            std::env::set_var(k, v);
        }
        Self { keys: prior }
    }
}
impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (k, prior) in &self.keys {
            match prior {
                Some(v) => std::env::set_var(k, v),
                None => std::env::remove_var(k),
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn no_proxy_client_reaches_loopback_despite_https_proxy_env() {
    if !node_available() || real_kernel_path().is_none() {
        eprintln!("[loopback-no-proxy] skipping: node or kernel missing");
        return;
    }
    let server_addr = spawn_real_kernel_server().await;
    let proxy_addr = spawn_hostile_proxy().await;

    let proxy_url = format!("http://{proxy_addr}");
    let _guard = EnvGuard::set(&[
        ("HTTPS_PROXY", proxy_url.as_str()),
        ("HTTP_PROXY", proxy_url.as_str()),
        ("https_proxy", proxy_url.as_str()),
        ("http_proxy", proxy_url.as_str()),
    ]);
    println!("[loopback-no-proxy] hostile proxy at {proxy_addr}, real server at {server_addr}");

    // Build a client the same way the GUI does AFTER the fix.
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("client build");

    let url = format!("http://{server_addr}/api/v1/agent/sessions");
    let resp = client
        .post(&url)
        .json(&gui_body())
        .send()
        .await
        .expect("POST should succeed despite hostile proxy env");
    let status = resp.status().as_u16();
    let body = resp.text().await.unwrap_or_default();
    println!("[loopback-no-proxy] status={status} body={body}");
    assert_eq!(
        status, 201,
        "client must bypass the proxy and reach the real server; got body={body}"
    );
    let parsed: Value = serde_json::from_str(&body).unwrap();
    assert!(parsed["sessionId"].is_string(), "expected sessionId");
}

/// Negative control: a default-built client (without `.no_proxy()`)
/// honors the env proxy and FAILS to reach localhost. Locks in the
/// understanding of the bug — if reqwest ever changes its default
/// proxy resolution, this test catches it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn default_client_is_intercepted_by_https_proxy_env() {
    if !node_available() || real_kernel_path().is_none() {
        eprintln!("[loopback-no-proxy] skipping: node or kernel missing");
        return;
    }
    let server_addr = spawn_real_kernel_server().await;
    let proxy_addr = spawn_hostile_proxy().await;

    let proxy_url = format!("http://{proxy_addr}");
    let _guard = EnvGuard::set(&[
        ("HTTPS_PROXY", proxy_url.as_str()),
        ("HTTP_PROXY", proxy_url.as_str()),
        ("https_proxy", proxy_url.as_str()),
        ("http_proxy", proxy_url.as_str()),
    ]);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .expect("client build");

    let url = format!("http://{server_addr}/api/v1/agent/sessions");
    let result = client.post(&url).json(&gui_body()).send().await;
    match result {
        Ok(resp) => {
            // If reqwest somehow reached the real server, the bug
            // doesn't manifest — but on most reqwest builds + this env
            // setup, we expect a non-201 (proxy intercepted).
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            println!(
                "[loopback-no-proxy:negative] reqwest reached server with status={status} body={body}"
            );
            assert_ne!(
                status, 201,
                "default client without .no_proxy() unexpectedly succeeded — \
                 the proxy interception bug may not reproduce on this reqwest version"
            );
        }
        Err(e) => {
            // Expected path: reqwest tried the proxy, proxy closed the
            // socket, reqwest surfaces a connection error.
            println!("[loopback-no-proxy:negative] reqwest error (expected): {e}");
        }
    }
}
