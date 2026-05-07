//! `AgentProcess` — owns the omw-agent stdio child process and brokers
//! JSON-RPC 2.0 frames between callers (axum handlers) and the agent.
//!
//! Single Node process per omw-server instance. pi-agent multiplexes any
//! number of agent sessions through this single child by `sessionId`. A
//! crash kills every active session and is broadcast as a synthetic
//! `agent/crashed` notification on each subscriber bus.
//!
//! ## Frame routing
//!
//! Two categories of inbound frames from the child's stdout:
//!
//! - **Responses** (have a JSON-RPC `id`): matched against in-flight
//!   `send_method` calls via a `pending` map.
//! - **Notifications** (no `id`): the reader extracts `params.sessionId`
//!   and forwards to the matching session's broadcast channel. Frames
//!   with no `sessionId` (e.g. crash signals) fan out to *every*
//!   subscribed session.
//!
//! ## Lifecycle
//!
//! `spawn` starts the child and the reader task. Drop kills the child
//! and aborts the reader. The reader emits a `agent/crashed` notification
//! to every active session bus on EOF / non-zero exit, then closes the
//! pending-response oneshots so any in-flight `send_method` rejects with
//! `AgentProcessError::ChildExited`.

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin};
use tokio::sync::{broadcast, oneshot, Mutex as AsyncMutex};
use tokio::task::JoinHandle;

use super::bash_broker::BashBroker;

/// Capacity of the per-session notification broadcast channel.
const NOTIFICATION_CAPACITY: usize = 256;

#[derive(Debug, Error)]
pub enum AgentProcessError {
    #[error("failed to spawn agent process: {0}")]
    Spawn(#[from] std::io::Error),
    #[error("agent process exited before request completed")]
    ChildExited,
    #[error("agent returned protocol error: {0}")]
    Protocol(String),
    #[error("agent JSON-RPC error: {code} {message}")]
    JsonRpc { code: i64, message: String },
    #[error("malformed agent response: {0}")]
    Malformed(String),
}

/// Spawn configuration. Production callers fill this from `OMW_AGENT_BIN`
/// + `$PATH`; tests pass an explicit path to a fixture .mjs.
#[derive(Debug, Clone)]
pub struct AgentProcessConfig {
    pub command: String,
    pub args: Vec<String>,
}

impl AgentProcessConfig {
    /// Production default: `node $OMW_AGENT_BIN --serve-stdio`. If the env
    /// var is unset, falls back to bare `omw-agent --serve-stdio` which
    /// delegates to whatever resolves on `$PATH`.
    pub fn from_env() -> Self {
        match std::env::var("OMW_AGENT_BIN") {
            Ok(bin) => Self {
                command: "node".into(),
                args: vec![bin, "--serve-stdio".into()],
            },
            Err(_) => Self {
                command: "omw-agent".into(),
                args: vec!["--serve-stdio".into()],
            },
        }
    }
}

type PendingMap = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, AgentProcessError>>>>>;
type SessionMap = Arc<Mutex<HashMap<String, broadcast::Sender<Value>>>>;
type SharedStdin = Arc<AsyncMutex<ChildStdin>>;

pub struct AgentProcess {
    /// stdin sink, shared with the bash broker so kernel-bound notifications
    /// (e.g. snapshot bash/finished) can be sent from the reader task.
    stdin: SharedStdin,
    /// In-flight request map keyed by JSON-RPC id.
    pending: PendingMap,
    /// Per-session notification fan-out.
    sessions: SessionMap,
    /// Phase 5a bash routing — dispatch target for `bash/exec` from kernel.
    bash_broker: Arc<BashBroker>,
    /// Atomic id allocator for outbound requests.
    next_id: AtomicU64,
    /// Reader task handle. Aborted on drop.
    reader: Mutex<Option<JoinHandle<()>>>,
    /// Stderr-drain handle. Aborted on drop.
    stderr_drain: Mutex<Option<JoinHandle<()>>>,
    /// Watcher task handle. Aborted on drop.
    watcher: Mutex<Option<JoinHandle<()>>>,
    /// Kept alive so Drop kills the child.
    _child_guard: Mutex<Option<Child>>,
}

impl AgentProcess {
    /// Spawn the agent process and start reader + watcher tasks.
    pub async fn spawn(config: AgentProcessConfig) -> Result<Arc<Self>, AgentProcessError> {
        let mut cmd = tokio::process::Command::new(&config.command);
        cmd.args(&config.args);
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        cmd.kill_on_drop(true);

        let mut child = cmd.spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| AgentProcessError::Spawn(std::io::Error::other("no stdin")))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| AgentProcessError::Spawn(std::io::Error::other("no stdout")))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| AgentProcessError::Spawn(std::io::Error::other("no stderr")))?;

        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
        let sessions: SessionMap = Arc::new(Mutex::new(HashMap::new()));
        let stdin: SharedStdin = Arc::new(AsyncMutex::new(stdin));
        let bash_broker = BashBroker::new(sessions.clone(), stdin.clone());

        let reader_pending = pending.clone();
        let reader_sessions = sessions.clone();
        let reader_broker = bash_broker.clone();
        let reader = tokio::spawn(async move {
            run_reader(stdout, reader_pending, reader_sessions, reader_broker).await;
        });

        // Drain stderr without folding it into errors (I-1 defense in
        // depth — a buggy/malicious agent could leak secrets there).
        let stderr_drain = tokio::spawn(async move {
            let mut reader = BufReader::new(stderr);
            let mut buf = Vec::new();
            loop {
                buf.clear();
                match reader.read_until(b'\n', &mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
            }
        });

        let watcher_pending = pending.clone();
        let watcher_sessions = sessions.clone();
        let watcher = tokio::spawn(async move {
            // The Child is owned here so `wait()` can run. We move it in
            // by handing it to the watcher task. Drop semantics are
            // preserved by `kill_on_drop(true)` above when the watcher is
            // aborted.
            let _exit = child.wait().await;
            // Notify pending requests.
            {
                let mut map = watcher_pending.lock().expect("pending poisoned");
                for (_, tx) in map.drain() {
                    let _ = tx.send(Err(AgentProcessError::ChildExited));
                }
            }
            // Notify every active session bus that the process is gone.
            let crashed = json!({ "method": "agent/crashed", "params": {} });
            let map = watcher_sessions.lock().expect("sessions poisoned");
            for (_, sender) in map.iter() {
                let _ = sender.send(crashed.clone());
            }
        });

        // Note: we deliberately do NOT keep `child` here because the
        // watcher took ownership above. The watcher is responsible for
        // reaping; Drop only needs to abort the background tasks.
        Ok(Arc::new(Self {
            stdin,
            pending,
            sessions,
            bash_broker,
            next_id: AtomicU64::new(1),
            reader: Mutex::new(Some(reader)),
            stderr_drain: Mutex::new(Some(stderr_drain)),
            watcher: Mutex::new(Some(watcher)),
            _child_guard: Mutex::new(None),
        }))
    }

    /// Bash broker handle. Public so future code (e.g. integration tests
    /// or panel-level inspection) can introspect routing without going
    /// through the JSON-RPC wire.
    pub fn bash_broker(&self) -> &Arc<BashBroker> {
        &self.bash_broker
    }

    /// Send a JSON-RPC request and await its response.
    pub async fn send_method(
        self: &Arc<Self>,
        method: &str,
        params: Value,
    ) -> Result<Value, AgentProcessError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        {
            let mut map = self.pending.lock().expect("pending poisoned");
            map.insert(id, tx);
        }

        let frame = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        let line = serde_json::to_string(&frame).map_err(|e| {
            AgentProcessError::Malformed(format!("serialize request: {e}"))
        })?;

        // Acquire the stdin lock for the whole write so two concurrent
        // requests don't interleave bytes on the wire.
        {
            let mut sink = self.stdin.lock().await;
            sink.write_all(line.as_bytes()).await.map_err(|e| {
                AgentProcessError::Malformed(format!("write request: {e}"))
            })?;
            sink.write_all(b"\n").await.map_err(|e| {
                AgentProcessError::Malformed(format!("write request newline: {e}"))
            })?;
            sink.flush().await.ok();
        }

        match rx.await {
            Ok(result) => result,
            Err(_) => {
                // Sender dropped — child died before responding. Best to
                // remove our pending entry; the watcher's drain may have
                // beaten us to it.
                let mut map = self.pending.lock().expect("pending poisoned");
                map.remove(&id);
                Err(AgentProcessError::ChildExited)
            }
        }
    }

    /// Send a JSON-RPC notification (no `id`, no response expected). Used
    /// for fire-and-forget frames like `bash/data` and `bash/finished`
    /// where the kernel does not reply.
    pub async fn send_notification(
        self: &Arc<Self>,
        method: &str,
        params: Value,
    ) -> Result<(), AgentProcessError> {
        let frame = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        let line = serde_json::to_string(&frame).map_err(|e| {
            AgentProcessError::Malformed(format!("serialize notification: {e}"))
        })?;
        let mut sink = self.stdin.lock().await;
        sink.write_all(line.as_bytes()).await.map_err(|e| {
            AgentProcessError::Malformed(format!("write notification: {e}"))
        })?;
        sink.write_all(b"\n").await.map_err(|e| {
            AgentProcessError::Malformed(format!("write notification newline: {e}"))
        })?;
        sink.flush().await.ok();
        Ok(())
    }

    /// Subscribe to a session's notification stream. The sessionId must
    /// already have been minted via `session/create`. If the session is
    /// not yet known, this allocates the bus eagerly so callers can
    /// subscribe before the first notification arrives.
    pub fn subscribe(&self, session_id: &str) -> broadcast::Receiver<Value> {
        let mut map = self.sessions.lock().expect("sessions poisoned");
        let sender = map
            .entry(session_id.to_string())
            .or_insert_with(|| {
                let (tx, _rx) = broadcast::channel(NOTIFICATION_CAPACITY);
                tx
            });
        sender.subscribe()
    }

    /// Drop the per-session bus. Existing receivers will see EOF on the
    /// next `recv`. Idempotent.
    pub fn drop_session(&self, session_id: &str) {
        let mut map = self.sessions.lock().expect("sessions poisoned");
        map.remove(session_id);
    }
}

impl Drop for AgentProcess {
    fn drop(&mut self) {
        if let Some(h) = self.reader.lock().expect("reader lock").take() {
            h.abort();
        }
        if let Some(h) = self.stderr_drain.lock().expect("stderr lock").take() {
            h.abort();
        }
        if let Some(h) = self.watcher.lock().expect("watcher lock").take() {
            h.abort();
        }
    }
}

async fn run_reader(
    stdout: tokio::process::ChildStdout,
    pending: PendingMap,
    sessions: SessionMap,
    bash_broker: Arc<BashBroker>,
) {
    let mut reader = BufReader::new(stdout);
    let mut buf = String::new();
    loop {
        buf.clear();
        match reader.read_line(&mut buf).await {
            Ok(0) => return, // EOF — watcher will broadcast crash signal.
            Ok(_) => {}
            Err(_) => return,
        }
        let line = buf.trim_end_matches('\n').trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        let frame: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue, // ignore non-JSON noise
        };
        route_frame(frame, &pending, &sessions, &bash_broker).await;
    }
}

async fn route_frame(
    frame: Value,
    pending: &PendingMap,
    sessions: &SessionMap,
    bash_broker: &Arc<BashBroker>,
) {
    // Response: has an `id` and either `result` or `error`.
    if let Some(id_value) = frame.get("id").and_then(|v| v.as_u64()) {
        let waiter = {
            let mut map = pending.lock().expect("pending poisoned");
            map.remove(&id_value)
        };
        if let Some(waiter) = waiter {
            if let Some(err_obj) = frame.get("error") {
                let code = err_obj.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
                let message = err_obj
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                let _ = waiter.send(Err(AgentProcessError::JsonRpc { code, message }));
            } else {
                let result = frame.get("result").cloned().unwrap_or(Value::Null);
                let _ = waiter.send(Ok(result));
            }
        }
        return;
    }
    // Phase 5a: bash/exec is keyed by terminalSessionId, not sessionId, so
    // it would otherwise fan out to every WS subscriber. Dispatch it via
    // the broker (which translates to a `kind: "exec_command"` frame on
    // the matching session bus, or a snapshot reply when no GUI is live).
    if frame.get("method").and_then(|m| m.as_str()) == Some("bash/exec") {
        let params = frame
            .get("params")
            .cloned()
            .unwrap_or(Value::Null);
        bash_broker.handle_kernel_bash_exec(&params).await;
        return;
    }
    // Notification: route by params.sessionId, or fan-out if absent.
    let session_id = frame
        .get("params")
        .and_then(|p| p.get("sessionId"))
        .and_then(|s| s.as_str());
    let map = sessions.lock().expect("sessions poisoned");
    if let Some(id) = session_id {
        if let Some(sender) = map.get(id) {
            let _ = sender.send(frame.clone());
        }
        // Drop notifications for unknown sessions silently — the GUI
        // hasn't subscribed yet.
    } else {
        // Non-scoped frame (e.g. agent/crashed). Fan out to everyone.
        for (_, sender) in map.iter() {
            let _ = sender.send(frame.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn echo_config() -> AgentProcessConfig {
        // A tiny inline node script that echoes a fixed response for
        // session/create, then emits a couple of deltas + turn/finished
        // when session/prompt arrives. Used for smoke-testing the bridge
        // without pulling in the real omw-agent binary.
        let script = r#"
            const lines = [];
            process.stdin.on('data', (data) => {
                lines.push(...data.toString().split('\n').filter(Boolean));
                while (lines.length > 0) {
                    const line = lines.shift();
                    let req;
                    try { req = JSON.parse(line); } catch { continue; }
                    const id = req.id ?? null;
                    if (req.method === 'session/create') {
                        process.stdout.write(JSON.stringify({jsonrpc:'2.0',id,result:{sessionId:'s-test'}}) + '\n');
                    } else if (req.method === 'session/prompt') {
                        process.stdout.write(JSON.stringify({jsonrpc:'2.0',id,result:{ok:true}}) + '\n');
                        process.stdout.write(JSON.stringify({jsonrpc:'2.0',method:'assistant/delta',params:{sessionId:'s-test',delta:'hello'}}) + '\n');
                        process.stdout.write(JSON.stringify({jsonrpc:'2.0',method:'turn/finished',params:{sessionId:'s-test',cancelled:false}}) + '\n');
                    } else {
                        process.stdout.write(JSON.stringify({jsonrpc:'2.0',id,result:{ok:true}}) + '\n');
                    }
                }
            });
        "#;
        AgentProcessConfig {
            command: "node".into(),
            args: vec!["-e".into(), script.into()],
        }
    }

    #[tokio::test]
    async fn spawn_send_create_and_prompt() {
        let agent = AgentProcess::spawn(echo_config()).await.unwrap();
        let create = agent
            .send_method("session/create", json!({"providerConfig":{"kind":"openai-compatible","base_url":"x"},"model":"x"}))
            .await
            .unwrap();
        assert_eq!(create["sessionId"], "s-test");

        let mut rx = agent.subscribe("s-test");
        let prompt_resp = agent
            .send_method("session/prompt", json!({"sessionId":"s-test","prompt":"hi"}))
            .await
            .unwrap();
        assert_eq!(prompt_resp["ok"], true);

        // First notification: assistant/delta
        let n1 = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
            .await
            .expect("notification timeout")
            .expect("recv");
        assert_eq!(n1["method"], "assistant/delta");
        assert_eq!(n1["params"]["delta"], "hello");

        // Second notification: turn/finished
        let n2 = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
            .await
            .expect("notification timeout")
            .expect("recv");
        assert_eq!(n2["method"], "turn/finished");
    }

    #[tokio::test]
    async fn unknown_method_returns_jsonrpc_error() {
        // Spawn a node that always replies with a JSON-RPC error.
        let cfg = AgentProcessConfig {
            command: "node".into(),
            args: vec![
                "-e".into(),
                r#"process.stdin.on('data', d => {
                    const line = d.toString().split('\n').filter(Boolean)[0];
                    const req = JSON.parse(line);
                    process.stdout.write(JSON.stringify({jsonrpc:'2.0',id:req.id,error:{code:-32601,message:'no'}}) + '\n');
                });"#.into(),
            ],
        };
        let agent = AgentProcess::spawn(cfg).await.unwrap();
        let err = agent.send_method("bogus", json!({})).await.unwrap_err();
        match err {
            AgentProcessError::JsonRpc { code, .. } => assert_eq!(code, -32601),
            other => panic!("expected JsonRpc error, got {other:?}"),
        }
    }
}
