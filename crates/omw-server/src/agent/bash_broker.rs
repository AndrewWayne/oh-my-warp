//! Phase 5a server-side bash broker.
//!
//! Pattern B: bash/* are correlated JSON-RPC notifications scoped by
//! `commandId`. The broker is the kernel-side translation layer for the
//! kernel→GUI direction:
//!
//! - On `bash/exec` from the kernel, look up the GUI WS subscribed to the
//!   target `terminalSessionId` and emit a `kind: "exec_command"` text
//!   frame on its broadcast bus.
//! - When no GUI is subscribed (terminal pane never connected, or the
//!   user closed the panel mid-call), respond with `bash/finished
//!   { snapshot: true }` back to the kernel so the in-flight exec resolves
//!   instead of hanging until its TS-side timeout fires.
//!
//! The reverse direction (`command_data` / `command_exit` from the GUI →
//! `bash/data` / `bash/finished` to the kernel) is handled inline by the
//! WS handler in `handlers/agent.rs` via `AgentProcess::send_notification`.
//! It does not touch this module.
//!
//! The broker shares the same `SessionMap` and `stdin` handle with
//! [`AgentProcess`], wired up in [`AgentProcess::spawn`].

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use tokio::io::AsyncWriteExt;
use tokio::process::ChildStdin;
use tokio::sync::broadcast;
use tokio::sync::Mutex as AsyncMutex;

pub(crate) type SessionMap = Arc<Mutex<HashMap<String, broadcast::Sender<Value>>>>;
pub(crate) type SharedStdin = Arc<AsyncMutex<ChildStdin>>;

/// Broker for kernel-emitted `bash/exec` frames.
pub struct BashBroker {
    sessions: SessionMap,
    stdin: SharedStdin,
}

impl BashBroker {
    pub(crate) fn new(sessions: SessionMap, stdin: SharedStdin) -> Arc<Self> {
        Arc::new(Self { sessions, stdin })
    }

    /// Handle a `bash/exec` notification arriving from the kernel.
    ///
    /// Looks up the GUI bus by `params.terminalSessionId`. If a subscriber
    /// is present, emits a `kind: "exec_command"` text frame onto that bus
    /// (the WS handler forwards each broadcast frame as text to the GUI).
    ///
    /// If no subscriber exists, sends a synthetic `bash/finished` back to
    /// the kernel with `snapshot: true` so the kernel's tool call resolves
    /// without waiting for the TS-side timeout to fire.
    pub async fn handle_kernel_bash_exec(&self, params: &Value) {
        let command_id = params.get("commandId").cloned().unwrap_or(Value::Null);
        let terminal_session_id = params
            .get("terminalSessionId")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        if let Some(tid) = terminal_session_id.as_deref() {
            let sender = {
                let map = self.sessions.lock().expect("sessions poisoned");
                map.get(tid).cloned()
            };
            if let Some(sender) = sender {
                if sender.receiver_count() > 0 {
                    let exec_frame = json!({
                        "kind": "exec_command",
                        "commandId": command_id,
                        "command": params.get("command").cloned().unwrap_or(Value::Null),
                        "cwd": params.get("cwd").cloned().unwrap_or(Value::Null),
                        "agentSessionId": params
                            .get("agentSessionId")
                            .cloned()
                            .unwrap_or(Value::Null),
                        "toolCallId": params
                            .get("toolCallId")
                            .cloned()
                            .unwrap_or(Value::Null),
                    });
                    let _ = sender.send(exec_frame);
                    return;
                }
            }
        }

        // No live GUI for this terminal — synthesise a snapshot bash/finished
        // back to the kernel so the in-flight tool call resolves promptly.
        self.send_kernel_notification(
            "bash/finished",
            json!({
                "commandId": command_id,
                "snapshot": true,
                "error": "no active GUI terminal",
            }),
        )
        .await;
    }

    async fn send_kernel_notification(&self, method: &str, params: Value) {
        let frame = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        let line = match serde_json::to_string(&frame) {
            Ok(s) => s,
            Err(_) => return,
        };
        let mut sink = self.stdin.lock().await;
        if sink.write_all(line.as_bytes()).await.is_err() {
            return;
        }
        if sink.write_all(b"\n").await.is_err() {
            return;
        }
        let _ = sink.flush().await;
    }
}
