//! omw-local agent panel state — process-wide singleton accessed from the
//! UI panel. Mirrors the [`OmwRemoteState`] pattern (`omw/remote_state.rs`)
//! but for the agent surface rather than the BYORC daemon.
//!
//! Owns:
//! - A dedicated tokio runtime in a background thread (independent of
//!   [`OmwRemoteState`]'s runtime — agent state survives `omw remote stop`).
//! - The HTTP client used to `POST /api/v1/agent/sessions` against
//!   omw-server.
//! - The WS task connected to `/ws/v1/agent/:sessionId`. Inbound text
//!   frames are deserialized into [`super::omw_protocol::OmwAgentEventDown`]
//!   and broadcast on an internal channel; outbound prompts and cancels
//!   from the UI flow back via an mpsc.
//! - A [`tokio::sync::watch::Sender<OmwAgentStatus>`] for reactive UI label
//!   updates.
//!
//! Phase 3b lands this **compiled-but-unused** — the panel.rs surgery in
//! Phase 3c flips `is_omw_placeholder` to call into [`OmwAgentState::shared`].
//! Until then, the entire module is allowlisted via `#[allow(dead_code)]`
//! to keep the omw_local build warning-free.

#![allow(dead_code)]

use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::Duration;

use parking_lot::Mutex;
use tokio::runtime::Builder;
use tokio::sync::{broadcast, mpsc, watch};
use tokio::task::JoinHandle;

use super::omw_protocol::{ApprovalDecision, OmwAgentEventDown, OmwAgentEventUp};

/// Default omw-server URL. Phase 3b assumes the GUI and the server share
/// a host. Callers can override via the `OMW_SERVER_URL` env var.
const DEFAULT_SERVER_URL: &str = "http://127.0.0.1:8788";

/// HTTP/WS request timeout. Generous because the kernel's first-token
/// latency includes a Node spawn + provider TLS handshake.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Capacity of the per-state broadcast channel. The agent panel is the
/// sole consumer in v0.1; the channel is dimensioned for a few-second
/// burst of streaming deltas.
const EVENT_CHANNEL_CAPACITY: usize = 256;

/// Capacity of the outbound mpsc the UI uses to inject prompts / cancels
/// into the WS writer task.
const OUTBOUND_CHANNEL_CAPACITY: usize = 32;

/// Public status surface for the agent-panel header.
#[derive(Clone, Debug, PartialEq)]
pub enum OmwAgentStatus {
    /// No session active. Initial state and post-`stop` resting state.
    Idle,
    /// `start` is in flight — POST /api/v1/agent/sessions has been issued
    /// but the WS isn't connected yet.
    Starting,
    /// WS is connected, ready to accept a prompt.
    Connected { session_id: String },
    /// A prompt is streaming.
    Streaming { session_id: String },
    /// Last `start` failed; carries the human-readable error.
    Failed { error: String },
}

/// Configuration handed to [`OmwAgentState::start`]. The provider config
/// is passed through verbatim to the kernel's `session/create` JSON-RPC
/// method, so the shape mirrors `apps/omw-agent/src/session.ts`'s
/// `ProviderConfig`.
#[derive(Clone, Debug)]
pub struct OmwAgentSessionParams {
    pub provider_kind: String,
    pub key_ref: Option<String>,
    pub base_url: Option<String>,
    pub model: String,
    pub system_prompt: Option<String>,
    pub cwd: Option<String>,
    /// Approval-policy mode forwarded to the kernel as `policy.mode` in
    /// `session/create`. Wire form: `"read_only" | "ask_before_write" |
    /// "trusted"`. `None` lets the kernel apply its default.
    pub approval_mode: Option<String>,
}

/// Opaque handle to the currently-focused terminal pane. Stored by the
/// broker so it can route `ExecCommand` to the right PTY.
///
/// `event_loop_tx` and `pty_reads_tx` are clones of the local-PTY channels
/// that `local_tty::TerminalManager` exposes (the same handles the
/// `omw/pane_share.rs` bridge uses). The broker captures this struct at
/// `bash/exec` time and drives the original pane through completion even
/// if the user shifts focus mid-command.
#[derive(Clone)]
pub struct ActiveTerminalHandle {
    pub view_id: warpui::EntityId,
    /// Sender into the local-PTY event loop. The broker pushes
    /// `Message::Input(bytes)` to inject the agent's command into the
    /// pane's stdin.
    pub event_loop_tx:
        std::sync::Arc<parking_lot::Mutex<crate::terminal::local_tty::mio_channel::Sender<crate::terminal::writeable_pty::Message>>>,
    /// Broadcast sender for raw PTY output bytes. The broker calls
    /// `.new_receiver()` on it to tap output without disturbing existing
    /// subscribers (the renderer, throughput recorder, omw-remote share).
    pub pty_reads_tx: async_broadcast::Sender<std::sync::Arc<Vec<u8>>>,
}

/// Process-wide singleton.
pub struct OmwAgentState {
    inner: Mutex<Inner>,
    status_tx: watch::Sender<OmwAgentStatus>,
    /// Bus the UI subscribes to for inbound events. Survives session
    /// restarts so the same UI subscription can keep listening across
    /// `start` cycles.
    event_tx: broadcast::Sender<OmwAgentEventDown>,
    /// Currently-focused terminal pane. Set by TerminalView on focus;
    /// cleared on blur/close. `None` when no pane is active.
    active_terminal: Mutex<Option<ActiveTerminalHandle>>,
}

struct Inner {
    status: OmwAgentStatus,
    /// Outbound sender into the WS writer task. `Some` while a session is
    /// active; cleared on `stop`.
    outbound: Option<mpsc::Sender<OmwAgentEventUp>>,
    /// WS task handle. Aborted on `stop`.
    ws_task: Option<JoinHandle<()>>,
    /// Phase 5b command-broker task. Spawned on first `start` and lives
    /// until the singleton is dropped — the broker just polls
    /// `subscribe_events`, so we don't tear it down on `stop` (cheap to
    /// leave running across session restarts).
    command_broker_task: Option<JoinHandle<()>>,
    runtime_handle: Option<tokio::runtime::Handle>,
    runtime_thread: Option<thread::JoinHandle<()>>,
}

static SHARED: OnceLock<Arc<OmwAgentState>> = OnceLock::new();

impl OmwAgentState {
    /// Process-wide accessor. Lazily constructs on first call.
    pub fn shared() -> Arc<Self> {
        SHARED
            .get_or_init(|| {
                let (status_tx, _rx) = watch::channel(OmwAgentStatus::Idle);
                let (event_tx, _ev_rx) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
                Arc::new(Self {
                    inner: Mutex::new(Inner {
                        status: OmwAgentStatus::Idle,
                        outbound: None,
                        ws_task: None,
                        command_broker_task: None,
                        runtime_handle: None,
                        runtime_thread: None,
                    }),
                    status_tx,
                    event_tx,
                    active_terminal: Mutex::new(None),
                })
            })
            .clone()
    }

    /// Snapshot of the current status. Cheap; the panel can call on every
    /// render.
    pub fn status(&self) -> OmwAgentStatus {
        self.inner.lock().status.clone()
    }

    /// Subscribe to status transitions. UI uses
    /// [`tokio::sync::watch::Receiver::changed`] to await transitions and
    /// re-render label/tooltip.
    pub fn status_rx(&self) -> watch::Receiver<OmwAgentStatus> {
        self.status_tx.subscribe()
    }

    /// Subscribe to inbound agent events. Each subscriber gets every
    /// event from the moment they subscribe forward; missed events
    /// (`Lagged`) silently skip — the assistant turn re-renders cleanly
    /// because `OmwAgentTranscriptModel::apply_event` only ever appends
    /// or mutates the *last* assistant row, so a missed delta is the
    /// only consequence (we do not lose final-message integrity).
    pub fn subscribe_events(&self) -> broadcast::Receiver<OmwAgentEventDown> {
        self.event_tx.subscribe()
    }

    /// Start an agent session. Issues `POST /api/v1/agent/sessions`,
    /// then opens the WS for the returned sessionId. Idempotent against
    /// the *current* session: calling `start` while one is already
    /// running stops it first.
    pub fn start(self: &Arc<Self>, params: OmwAgentSessionParams) -> Result<(), String> {
        let runtime = self.ensure_runtime()?;
        // Clear any existing session so we don't leak the prior WS task.
        self.stop();

        let server_url = std::env::var("OMW_SERVER_URL")
            .unwrap_or_else(|_| DEFAULT_SERVER_URL.to_string());

        self.set_status(OmwAgentStatus::Starting);

        let (out_tx, out_rx) = mpsc::channel::<OmwAgentEventUp>(OUTBOUND_CHANNEL_CAPACITY);
        let event_tx = self.event_tx.clone();
        let status_tx = self.status_tx.clone();
        let weak_self = Arc::downgrade(self);

        let task = runtime.spawn(async move {
            run_session(server_url, params, out_rx, event_tx, status_tx, weak_self).await;
        });

        // Spawn the Phase 5b command broker once per process. Idempotent on
        // repeated `start` calls — the broker subscribes to the long-lived
        // event_tx and lives until the singleton is dropped.
        let mut g = self.inner.lock();
        g.outbound = Some(out_tx);
        g.ws_task = Some(task);
        if g.command_broker_task.is_none() {
            let broker = super::omw_command_broker::spawn_command_broker(self.clone(), &runtime);
            g.command_broker_task = Some(broker);
        }
        Ok(())
    }

    /// Stop the current session. Aborts the WS task; the status drops to
    /// `Idle`. Idempotent.
    pub fn stop(&self) {
        let task = {
            let mut g = self.inner.lock();
            g.outbound = None;
            g.ws_task.take()
        };
        if let Some(t) = task {
            t.abort();
        }
        self.set_status(OmwAgentStatus::Idle);
    }

    /// Send a prompt over the active WS. No-op (returns Err) if no session.
    pub fn send_prompt(&self, prompt: String) -> Result<(), String> {
        let outbound = self
            .inner
            .lock()
            .outbound
            .as_ref()
            .cloned()
            .ok_or_else(|| "no active agent session".to_string())?;
        outbound
            .try_send(OmwAgentEventUp::Prompt { prompt })
            .map_err(|e| format!("send_prompt: {e}"))
    }

    /// Cancel the current in-flight prompt.
    pub fn cancel(&self) -> Result<(), String> {
        let outbound = self
            .inner
            .lock()
            .outbound
            .as_ref()
            .cloned()
            .ok_or_else(|| "no active agent session".to_string())?;
        outbound
            .try_send(OmwAgentEventUp::Cancel)
            .map_err(|e| format!("cancel: {e}"))
    }

    /// Convenience wrapper around [`start`] that loads `omw-config` and
    /// resolves the default provider into [`OmwAgentSessionParams`]. Returns
    /// `Err` if no provider is configured, the agent is disabled, or the
    /// default provider points to a missing entry.
    pub fn start_with_config(self: &Arc<Self>) -> Result<(), String> {
        let cfg = omw_config::Config::load().map_err(|e| e.to_string())?;
        if !cfg.agent.enabled {
            return Err("Agent is disabled in settings".into());
        }
        let provider_id = cfg
            .default_provider
            .as_ref()
            .ok_or_else(|| "No default provider configured".to_string())?;
        let provider = cfg
            .providers
            .get(provider_id)
            .ok_or_else(|| format!("default_provider `{provider_id}` not found"))?;

        let approval_mode = match cfg.approval.mode {
            omw_config::ApprovalMode::ReadOnly => Some("read_only".into()),
            omw_config::ApprovalMode::AskBeforeWrite => Some("ask_before_write".into()),
            omw_config::ApprovalMode::Trusted => Some("trusted".into()),
        };

        let params = OmwAgentSessionParams {
            provider_kind: provider.kind_str().to_string(),
            key_ref: provider.key_ref().map(|k| k.to_string()),
            base_url: match provider {
                omw_config::ProviderConfig::OpenAiCompatible { base_url, .. } => {
                    Some(base_url.as_str().to_string())
                }
                omw_config::ProviderConfig::Ollama { base_url, .. } => {
                    base_url.as_ref().map(|u| u.as_str().to_string())
                }
                _ => None,
            },
            model: provider
                .default_model()
                .map(|s| s.to_string())
                .unwrap_or_default(),
            system_prompt: None,
            cwd: None,
            approval_mode,
        };

        self.start(params)
    }

    /// Send an approval decision (Approve / Reject / Cancel) for an
    /// approval request the kernel emitted. Idempotent against duplicate
    /// decisions for the same approvalId — the kernel resolves only once.
    pub fn send_approval_decision(
        &self,
        approval_id: String,
        decision: ApprovalDecision,
    ) -> Result<(), String> {
        let outbound = {
            let g = self.inner.lock();
            g.outbound.clone()
        };
        let outbound = outbound.ok_or_else(|| "no active agent session".to_string())?;
        let frame = OmwAgentEventUp::ApprovalDecision {
            approval_id,
            decision,
        };
        outbound.try_send(frame).map_err(|e| e.to_string())
    }

    /// Register the currently-focused terminal pane. Called by TerminalView
    /// on focus (Task 11 stretch for the actual call-site wiring).
    pub fn register_active_terminal(&self, handle: ActiveTerminalHandle) {
        *self.active_terminal.lock() = Some(handle);
    }

    /// Clear the active terminal registration (on blur / pane close).
    pub fn clear_active_terminal(&self) {
        *self.active_terminal.lock() = None;
    }

    /// Snapshot of the currently-registered terminal handle, if any.
    pub fn active_terminal_clone(&self) -> Option<ActiveTerminalHandle> {
        self.active_terminal.lock().clone()
    }

    /// Forward a PTY data chunk back to the kernel for the given command.
    /// `data` is expected to be base64-encoded by the caller.
    pub fn send_command_data(&self, command_id: String, data: String) -> Result<(), String> {
        let outbound = self.inner.lock().outbound.clone()
            .ok_or_else(|| "no active session".to_string())?;
        outbound
            .try_send(OmwAgentEventUp::CommandData { command_id, data })
            .map_err(|e| e.to_string())
    }

    /// Signal command completion back to the kernel.
    pub fn send_command_exit(
        &self,
        command_id: String,
        exit_code: Option<i32>,
        snapshot: bool,
    ) -> Result<(), String> {
        let outbound = self.inner.lock().outbound.clone()
            .ok_or_else(|| "no active session".to_string())?;
        outbound
            .try_send(OmwAgentEventUp::CommandExit { command_id, exit_code, snapshot })
            .map_err(|e| e.to_string())
    }

    /// Test-only: install a fake outbound channel so tests can observe
    /// `OmwAgentEventUp` emissions from the broker without spinning up a
    /// real WebSocket. Returns the previous outbound, if any.
    #[cfg(any(test, feature = "test-exports"))]
    pub fn test_install_outbound(
        &self,
        sender: mpsc::Sender<OmwAgentEventUp>,
    ) -> Option<mpsc::Sender<OmwAgentEventUp>> {
        let mut g = self.inner.lock();
        g.outbound.replace(sender)
    }

    /// Test-only: clear the outbound and tear down the broker task. Allows
    /// a test to reset state before exercising a fresh code path.
    #[cfg(any(test, feature = "test-exports"))]
    pub fn test_reset(&self) {
        let mut g = self.inner.lock();
        g.outbound = None;
        if let Some(t) = g.command_broker_task.take() {
            t.abort();
        }
        if let Some(t) = g.ws_task.take() {
            t.abort();
        }
        drop(g);
        self.clear_active_terminal();
    }

    /// Test-only: inject an event onto the broadcast bus the broker
    /// subscribes to. Mimics what `run_session` would do on receipt of
    /// a kernel notification.
    #[cfg(any(test, feature = "test-exports"))]
    pub fn test_inject_event(&self, event: OmwAgentEventDown) {
        let _ = self.event_tx.send(event);
    }

    /// Test-only: ensure the runtime is up and return its handle. Tests
    /// use this to spawn the command broker manually instead of going
    /// through `start_with_config`, which would also kick off a WS task
    /// against a non-existent server.
    #[cfg(any(test, feature = "test-exports"))]
    pub fn test_ensure_runtime(&self) -> Result<tokio::runtime::Handle, String> {
        self.ensure_runtime()
    }

    fn set_status(&self, status: OmwAgentStatus) {
        self.inner.lock().status = status.clone();
        self.status_tx.send_replace(status);
    }

    fn ensure_runtime(&self) -> Result<tokio::runtime::Handle, String> {
        let mut g = self.inner.lock();
        if let Some(h) = &g.runtime_handle {
            return Ok(h.clone());
        }
        let (handle_tx, handle_rx) = std::sync::mpsc::channel();
        let thread = thread::Builder::new()
            .name("omw-agent-rt".into())
            .spawn(move || {
                let rt = match Builder::new_multi_thread()
                    .enable_all()
                    .worker_threads(2)
                    .thread_name("omw-agent-worker")
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => {
                        let _ = handle_tx.send(Err(format!("build runtime: {e}")));
                        return;
                    }
                };
                let _ = handle_tx.send(Ok(rt.handle().clone()));
                rt.block_on(std::future::pending::<()>());
            })
            .map_err(|e| format!("spawn runtime thread: {e}"))?;
        let handle = handle_rx
            .recv()
            .map_err(|_| "runtime thread vanished".to_string())??;
        g.runtime_handle = Some(handle.clone());
        g.runtime_thread = Some(thread);
        Ok(handle)
    }
}

async fn run_session(
    server_url: String,
    params: OmwAgentSessionParams,
    mut outbound: mpsc::Receiver<OmwAgentEventUp>,
    event_tx: broadcast::Sender<OmwAgentEventDown>,
    status_tx: watch::Sender<OmwAgentStatus>,
    weak_self: std::sync::Weak<OmwAgentState>,
) {
    // Route every status transition through the singleton so the cached
    // `inner.status` (returned by `OmwAgentState::status()`) stays in
    // sync with the watch channel. If the singleton has already been
    // dropped, fall back to the watch channel — receivers may still be
    // listening even though the snapshot accessor is gone.
    let set_status = |status: OmwAgentStatus| {
        if let Some(state) = weak_self.upgrade() {
            state.set_status(status);
        } else {
            status_tx.send_replace(status);
        }
    };

    let session_id = match create_session(&server_url, &params).await {
        Ok(id) => id,
        Err(e) => {
            set_status(OmwAgentStatus::Failed { error: e });
            return;
        }
    };

    let ws_url = match server_url.strip_prefix("http://") {
        Some(rest) => format!("ws://{rest}/ws/v1/agent/{session_id}"),
        None => match server_url.strip_prefix("https://") {
            Some(rest) => format!("wss://{rest}/ws/v1/agent/{session_id}"),
            None => {
                set_status(OmwAgentStatus::Failed {
                    error: "OMW_SERVER_URL must start with http:// or https://".into(),
                });
                return;
            }
        },
    };

    let connect = match tokio_tungstenite::connect_async(&ws_url).await {
        Ok((stream, _)) => stream,
        Err(e) => {
            set_status(OmwAgentStatus::Failed {
                error: format!("ws connect: {e}"),
            });
            return;
        }
    };

    set_status(OmwAgentStatus::Connected {
        session_id: session_id.clone(),
    });

    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message as WsMessage;

    let (mut sink, mut stream) = connect.split();
    let mut current_session = session_id.clone();

    loop {
        tokio::select! {
            // Inbound from WS -> deserialize -> broadcast.
            msg = stream.next() => {
                let Some(msg) = msg else { break };
                let msg = match msg {
                    Ok(m) => m,
                    Err(_) => break,
                };
                let text = match msg {
                    WsMessage::Text(t) => t,
                    WsMessage::Binary(b) => String::from_utf8_lossy(&b).into_owned(),
                    WsMessage::Close(_) => break,
                    _ => continue,
                };
                if let Ok(event) = serde_json::from_str::<OmwAgentEventDown>(&text) {
                    // Status transitions: Streaming on first delta /
                    // tool_call; Connected on turn_finished. Both go
                    // through `set_status` so the cached snapshot stays
                    // in lockstep with the watch channel.
                    match &event {
                        OmwAgentEventDown::AssistantDelta { .. }
                        | OmwAgentEventDown::ToolCallStarted { .. } => {
                            // Avoid bouncing Streaming -> Streaming on
                            // every delta: only transition when not
                            // already streaming.
                            let already_streaming = if let Some(state) = weak_self.upgrade() {
                                matches!(state.status(), OmwAgentStatus::Streaming { .. })
                            } else {
                                matches!(*status_tx.borrow(), OmwAgentStatus::Streaming { .. })
                            };
                            if !already_streaming {
                                set_status(OmwAgentStatus::Streaming {
                                    session_id: current_session.clone(),
                                });
                            }
                        }
                        OmwAgentEventDown::TurnFinished { session_id: sid, .. } => {
                            current_session = sid.clone();
                            set_status(OmwAgentStatus::Connected {
                                session_id: current_session.clone(),
                            });
                        }
                        _ => {}
                    }
                    let _ = event_tx.send(event);
                }
            }

            // Outbound from UI -> WS as JSON text.
            up = outbound.recv() => {
                let Some(up) = up else { break };
                let line = match serde_json::to_string(&up) {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                if sink.send(WsMessage::Text(line)).await.is_err() {
                    break;
                }
            }
        }
    }

    // Stream ended — clear status.
    set_status(OmwAgentStatus::Idle);
    let _ = sink.close().await;
    if let Some(state) = weak_self.upgrade() {
        let mut g = state.inner.lock();
        g.outbound = None;
        g.ws_task = None;
    }
}

async fn create_session(
    server_url: &str,
    params: &OmwAgentSessionParams,
) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|e| format!("build http client: {e}"))?;

    let mut provider_config = serde_json::json!({ "kind": params.provider_kind });
    if let Some(k) = &params.key_ref {
        provider_config["key_ref"] = serde_json::Value::String(k.clone());
    }
    if let Some(b) = &params.base_url {
        provider_config["base_url"] = serde_json::Value::String(b.clone());
    }

    let mut body = serde_json::json!({
        "providerConfig": provider_config,
        "model": params.model,
    });
    if let Some(sp) = &params.system_prompt {
        body["systemPrompt"] = serde_json::Value::String(sp.clone());
    }
    if let Some(cwd) = &params.cwd {
        body["cwd"] = serde_json::Value::String(cwd.clone());
    }
    if let Some(mode) = &params.approval_mode {
        body["policy"] = serde_json::json!({ "mode": mode });
    }

    let url = format!("{}/api/v1/agent/sessions", server_url.trim_end_matches('/'));
    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("post session: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!(
            "post session returned {}: {}",
            resp.status(),
            resp.text().await.unwrap_or_default()
        ));
    }
    let v: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("parse session response: {e}"))?;
    v.get("sessionId")
        .and_then(|s| s.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "response missing sessionId".to_string())
}
