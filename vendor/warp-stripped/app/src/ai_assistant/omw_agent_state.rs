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

/// Per-pane agent session. Each pane that uses the `# `-prefix inline
/// flow gets its own kernel session, WS connection, and event bus —
/// so prompts in different panes don't share conversation context
/// (the agent's history grows per pane, not globally).
///
/// Keyed by [`warpui::EntityId`] of the [`TerminalView`] in
/// `OmwAgentState::pane_sessions`. The session is provisioned lazily
/// on the first `# `-prefix submission within a pane (see
/// [`OmwAgentState::start_pane_session`]) and torn down on pane
/// close.
pub struct PaneSession {
    /// Kernel-issued sessionId returned by `POST /api/v1/agent/sessions`.
    pub session_id: String,
    /// Outbound sender into the per-pane WS writer task. Drops to
    /// `Err(SendError::Closed)` once `ws_task` exits.
    outbound: mpsc::Sender<OmwAgentEventUp>,
    /// WS reader/writer task for `/ws/v1/agent/:session_id`. Aborted
    /// when the session is replaced or the pane is removed.
    ws_task: Mutex<Option<JoinHandle<()>>>,
    /// Per-pane inbound event bus. The pump task spawned by
    /// [`OmwAgentState::send_prompt_inline`] subscribes here.
    /// Capacity = [`EVENT_CHANNEL_CAPACITY`].
    event_tx: broadcast::Sender<OmwAgentEventDown>,
    /// Per-pane status — the panel header reads this for the pane's
    /// header tooltip.
    status_tx: watch::Sender<OmwAgentStatus>,
}

impl PaneSession {
    /// Snapshot of the per-pane status.
    pub fn status(&self) -> OmwAgentStatus {
        self.status_tx.borrow().clone()
    }

    /// Subscribe to this pane's inbound event stream. Each subscriber
    /// gets every event from the moment they subscribe forward — same
    /// semantics as [`OmwAgentState::subscribe_events`] but scoped to
    /// the pane's session.
    pub fn subscribe_events(&self) -> broadcast::Receiver<OmwAgentEventDown> {
        self.event_tx.subscribe()
    }

    /// Send a prompt UP to the kernel via this pane's WS. Errors map
    /// the same as [`OmwAgentState::send_prompt`].
    pub fn send_prompt(&self, prompt: String) -> Result<(), String> {
        self.outbound
            .try_send(OmwAgentEventUp::Prompt { prompt })
            .map_err(|e| format!("send_prompt (pane {}): {e}", self.session_id))
    }

    /// Tear down the WS task. Called on pane close.
    pub fn stop(&self) {
        if let Some(t) = self.ws_task.lock().take() {
            t.abort();
        }
        let _ = self.status_tx.send(OmwAgentStatus::Idle);
    }
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
    /// cleared on blur/close. `None` when no pane is active. Used by
    /// the command broker (which is invoked by remote tooling and
    /// genuinely needs "whatever pane the user is looking at").
    /// **Not** used for the inline `# foo` path — that uses `pane_io`
    /// keyed by the submitting Input's own `terminal_view_id`.
    active_terminal: Mutex<Option<ActiveTerminalHandle>>,
    /// Per-pane io handles keyed by view_id. Populated for every
    /// local-tty pane on each `on_pane_state_change` (and refreshed
    /// idempotently on focus changes). The inline `# foo` path looks
    /// up by `Input::terminal_view_id` so the prompt is always
    /// dispatched to the pane the user actually typed in, regardless
    /// of which pane the global `active_terminal` happens to point at.
    pane_io: Mutex<std::collections::HashMap<warpui::EntityId, ActiveTerminalHandle>>,
    /// Per-pane agent sessions. Each pane gets its own kernel session
    /// + WS so its conversation history doesn't bleed into other
    /// panes. Keyed by the pane's [`warpui::EntityId`]. Populated
    /// lazily on the first `# `-prefix submission per pane.
    pane_sessions: Mutex<std::collections::HashMap<warpui::EntityId, Arc<PaneSession>>>,
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
                    pane_io: Mutex::new(std::collections::HashMap::new()),
                    pane_sessions: Mutex::new(std::collections::HashMap::new()),
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
        log::info!(
            "omw# state: start entry provider_kind={} model={}",
            params.provider_kind,
            params.model
        );
        let runtime = self.ensure_runtime()?;
        log::info!("omw# state: runtime ensured");
        // Clear any existing session so we don't leak the prior WS task.
        self.stop();

        // Bring up the in-process omw-server on first start so the user
        // doesn't have to launch a sidecar process. Idempotent — only the
        // first call binds the listener; later calls are O(1).
        log::info!("omw# state: calling inproc_server::ensure_running");
        if let Err(e) = super::omw_inproc_server::ensure_running(&runtime) {
            log::warn!("omw# state: ensure_running FAILED: {e}");
            self.set_status(OmwAgentStatus::Failed { error: e.clone() });
            return Err(e);
        }
        log::info!("omw# state: inproc_server ready");

        let server_url = std::env::var("OMW_SERVER_URL")
            .unwrap_or_else(|_| DEFAULT_SERVER_URL.to_string());

        self.set_status(OmwAgentStatus::Starting);

        let (out_tx, out_rx) = mpsc::channel::<OmwAgentEventUp>(OUTBOUND_CHANNEL_CAPACITY);
        let event_tx = self.event_tx.clone();
        let status_tx = self.status_tx.clone();
        let weak_self = Arc::downgrade(self);

        log::info!("omw# state: spawning run_session task");
        let task = runtime.spawn(async move {
            run_session(server_url, params, out_rx, event_tx, status_tx, weak_self).await;
            log::info!("omw# session: run_session task exited");
        });

        // Spawn the Phase 5b command broker once per process. Idempotent on
        // repeated `start` calls — the broker subscribes to the long-lived
        // event_tx and lives until the singleton is dropped.
        let mut g = self.inner.lock();
        g.outbound = Some(out_tx);
        g.ws_task = Some(task);
        if g.command_broker_task.is_none() {
            log::info!("omw# state: spawning command broker");
            let broker = super::omw_command_broker::spawn_command_broker(self.clone(), &runtime);
            g.command_broker_task = Some(broker);
        }
        log::info!("omw# state: start returning OK");
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
        log::info!(
            "omw# state: send_prompt outbound_capacity={} prompt_len={}",
            outbound.capacity(),
            prompt.len()
        );
        outbound
            .try_send(OmwAgentEventUp::Prompt { prompt })
            .map_err(|e| format!("send_prompt: {e}"))?;
        log::info!("omw# state: send_prompt try_send OK");
        Ok(())
    }

    /// Send a prompt and pump the streaming assistant response into the
    /// supplied terminal pane's PTY-read broadcast so the renderer shows
    /// it inline. Used by the `# `-prefix interception path so the user
    /// sees the agent's reply in the same block list they typed in,
    /// without having to open the agent panel.
    ///
    /// The pump runs on the agent runtime as a one-shot task — it
    /// subscribes to events at function entry, writes each
    /// `AssistantDelta` to the captured `pty_reads_tx` (so focus changes
    /// don't redirect the response), and exits on the first
    /// `TurnFinished`. Tool-call cards / approval cards are NOT mirrored
    /// here; they remain in the panel transcript.
    pub fn send_prompt_inline(
        self: &Arc<Self>,
        prompt: String,
        target: ActiveTerminalHandle,
    ) -> Result<(), String> {
        log::info!(
            "omw# state: send_prompt_inline entry view_id={:?} pty_recv_count={}",
            target.view_id,
            target.pty_reads_tx.receiver_count()
        );
        // Subscribe before sending so the first deltas can't race past
        // the receiver registration.
        let mut events = self.subscribe_events();
        log::info!(
            "omw# state: subscribed events; broadcast_recv_count={}",
            self.event_tx.receiver_count()
        );
        let runtime = self
            .inner
            .lock()
            .runtime_handle
            .clone()
            .ok_or_else(|| "agent runtime not yet initialised".to_string())?;

        // Echo the user's prompt above the streaming response so the
        // block list shows what they typed. CRLF + ESC[2K (erase to end
        // of line) keeps any active shell prompt redraw from clobbering
        // the echo. Sent through the same `Message::InjectBytes`
        // channel as the streaming response so the renderer treats the
        // echo + reply as a single contiguous block.
        let event_loop_tx = target.event_loop_tx.clone();
        let inject = |bytes: Vec<u8>| -> Result<(), String> {
            let n = bytes.len();
            let msg = crate::terminal::writeable_pty::Message::InjectBytes(
                std::borrow::Cow::Owned(bytes),
            );
            match event_loop_tx.lock().send(msg) {
                Ok(_) => {
                    log::trace!("omw# pump: injected {n} bytes into event_loop_tx");
                    Ok(())
                }
                Err(e) => {
                    log::warn!("omw# pump: inject {n} bytes FAILED: {e:?}");
                    Err(format!("inject into local-tty event loop: {e:?}"))
                }
            }
        };

        let echo = format!("\r\n\x1b[2K# {prompt}\r\n");
        match inject(echo.into_bytes()) {
            Ok(_) => log::info!("omw# state: echo injected OK"),
            Err(e) => log::warn!("omw# state: echo inject FAILED: {e}"),
        }

        // Send the prompt before spawning the pump — order matters: a
        // missed first delta is preferable to a missed TurnFinished.
        self.send_prompt(prompt)?;
        log::info!("omw# state: send_prompt returned OK; spawning pump");

        runtime.spawn(async move {
            log::info!("omw# pump: started, awaiting events");
            // ANSI palette: dim cyan for tool framing, dim yellow for
            // approvals, dim grey for status, red for errors. Each event
            // ends with `\x1b[0m` to reset before the assistant's text
            // resumes.
            const FRAME_DIM_CYAN: &str = "\x1b[2;36m";
            const FRAME_YELLOW: &str = "\x1b[33m";
            const FRAME_DIM: &str = "\x1b[2m";
            const FRAME_RED: &str = "\x1b[31m";
            const RESET: &str = "\x1b[0m";

            let send = |bytes: Vec<u8>| {
                let n = bytes.len();
                let msg = crate::terminal::writeable_pty::Message::InjectBytes(
                    std::borrow::Cow::Owned(bytes),
                );
                match event_loop_tx.lock().send(msg) {
                    Ok(_) => log::trace!("omw# pump: injected {n} bytes"),
                    Err(e) => log::warn!("omw# pump: inject {n} bytes FAILED: {e:?}"),
                }
            };
            while let Ok(event) = events.recv().await {
                let kind = match &event {
                    super::omw_protocol::OmwAgentEventDown::AssistantDelta { .. } => "AssistantDelta",
                    super::omw_protocol::OmwAgentEventDown::ToolCallStarted { .. } => "ToolCallStarted",
                    super::omw_protocol::OmwAgentEventDown::ToolCallFinished { .. } => "ToolCallFinished",
                    super::omw_protocol::OmwAgentEventDown::ApprovalRequest { .. } => "ApprovalRequest",
                    super::omw_protocol::OmwAgentEventDown::Error { .. } => "Error",
                    super::omw_protocol::OmwAgentEventDown::AgentCrashed => "AgentCrashed",
                    super::omw_protocol::OmwAgentEventDown::TurnFinished { .. } => "TurnFinished",
                    super::omw_protocol::OmwAgentEventDown::ExecCommand { .. } => "ExecCommand",
                    super::omw_protocol::OmwAgentEventDown::CommandData { .. } => "CommandData",
                    super::omw_protocol::OmwAgentEventDown::CommandExit { .. } => "CommandExit",
                };
                log::info!("omw# pump: recv event={kind}");
                match event {
                    super::omw_protocol::OmwAgentEventDown::AssistantDelta { delta, .. } => {
                        send(delta.into_bytes());
                    }
                    super::omw_protocol::OmwAgentEventDown::ToolCallStarted {
                        tool_name,
                        args,
                        ..
                    } => {
                        let summary = summarize_tool_args(&tool_name, &args);
                        let line = format!(
                            "\r\n{FRAME_DIM_CYAN}┌─ tool: {tool_name}{summary}{RESET}\r\n"
                        );
                        send(line.into_bytes());
                    }
                    super::omw_protocol::OmwAgentEventDown::ToolCallFinished {
                        tool_name,
                        is_error,
                        ..
                    } => {
                        let marker = if is_error { "✗ failed" } else { "✓ done" };
                        let color = if is_error { FRAME_RED } else { FRAME_DIM_CYAN };
                        let line = format!("{color}└─ {tool_name}: {marker}{RESET}\r\n");
                        send(line.into_bytes());
                    }
                    super::omw_protocol::OmwAgentEventDown::ApprovalRequest {
                        approval_id,
                        tool_call,
                        ..
                    } => {
                        let tool_name = tool_call
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("(unknown)");
                        let line = format!(
                            "\r\n{FRAME_YELLOW}⚠  approval needed for {tool_name} (id={approval_id})\r\n   open the agent panel to Approve / Reject{RESET}\r\n"
                        );
                        send(line.into_bytes());
                    }
                    super::omw_protocol::OmwAgentEventDown::Error { message, .. } => {
                        // Surface kernel/transport errors inline so the
                        // user doesn't have to dig through the log to see
                        // why a prompt produced no response.
                        let line = format!("\r\n{FRAME_RED}omw agent error: {message}{RESET}\r\n");
                        send(line.into_bytes());
                    }
                    super::omw_protocol::OmwAgentEventDown::AgentCrashed => {
                        send(format!(
                            "\r\n{FRAME_RED}omw agent kernel crashed (check ~/Library/Logs/warp-oss.log){RESET}\r\n"
                        ).into_bytes());
                        return;
                    }
                    super::omw_protocol::OmwAgentEventDown::TurnFinished { .. } => {
                        // Trailing newline + status hint so the user
                        // knows the turn is over before the next shell
                        // prompt re-draws.
                        send(format!("{FRAME_DIM}\r\n[turn finished]{RESET}\r\n").into_bytes());
                        return;
                    }
                    // bash/* events are routed via the command broker
                    // directly into the pane's PTY and don't need
                    // re-rendering here.
                    super::omw_protocol::OmwAgentEventDown::ExecCommand { .. }
                    | super::omw_protocol::OmwAgentEventDown::CommandData { .. }
                    | super::omw_protocol::OmwAgentEventDown::CommandExit { .. } => {}
                }
            }
            log::warn!("omw# pump: events.recv() returned Err — channel closed/lagged; exiting");
        });
        Ok(())
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

    /// Look up the [`PaneSession`] for a given pane. Returns `None` if
    /// the pane hasn't started a session yet (or has had its session
    /// removed via [`Self::remove_pane_session`]).
    pub fn pane_session(&self, view_id: warpui::EntityId) -> Option<Arc<PaneSession>> {
        self.pane_sessions.lock().get(&view_id).cloned()
    }

    /// Provision (or retrieve cached) a fresh kernel session bound to
    /// `view_id`. Each pane owns its own [`PaneSession`] so that
    /// `# `-prefix prompts in different panes don't share conversation
    /// context. The first call per pane:
    ///
    /// 1. Boots the in-process omw-server (idempotent — see
    ///    [`super::omw_inproc_server::ensure_running`]).
    /// 2. POSTs `/api/v1/agent/sessions` with the supplied params and
    ///    waits for the kernel-issued sessionId.
    /// 3. Connects a WS to `/ws/v1/agent/<session_id>` and parks a
    ///    reader/writer task that pumps frames between the pane's
    ///    outbound mpsc and a per-pane `event_tx` broadcast.
    /// 4. Caches the [`PaneSession`] in `pane_sessions[view_id]`.
    ///
    /// Subsequent calls with the same `view_id` return the cached
    /// session unchanged. To force a fresh session (e.g. on provider
    /// reconfiguration), call [`Self::remove_pane_session`] first.
    pub fn start_pane_session(
        self: &Arc<Self>,
        view_id: warpui::EntityId,
        params: OmwAgentSessionParams,
    ) -> Result<Arc<PaneSession>, String> {
        if let Some(existing) = self.pane_session(view_id) {
            return Ok(existing);
        }
        log::info!(
            "omw# state: start_pane_session view_id={view_id:?} kind={} model={}",
            params.provider_kind,
            params.model
        );
        let runtime = self.ensure_runtime()?;
        super::omw_inproc_server::ensure_running(&runtime)?;

        let server_url = std::env::var("OMW_SERVER_URL")
            .unwrap_or_else(|_| DEFAULT_SERVER_URL.to_string());

        // Block on session/create + WS connect synchronously so the
        // caller has a usable PaneSession on return — mirrors the
        // inproc_server's bind-before-return discipline. Without this,
        // send_prompt_inline_for_pane could race the WS connect.
        let (ready_tx, ready_rx) =
            std::sync::mpsc::sync_channel::<Result<Arc<PaneSession>, String>>(1);
        let server_url_for_task = server_url.clone();
        let view_id_for_task = view_id;
        let weak_self = Arc::downgrade(self);
        runtime.spawn(async move {
            let result = boot_pane_session(
                view_id_for_task,
                server_url_for_task,
                params,
                weak_self,
            )
            .await;
            let _ = ready_tx.send(result);
        });
        let pane_session = ready_rx
            .recv()
            .map_err(|_| "boot_pane_session channel dropped".to_string())??;

        self.pane_sessions
            .lock()
            .insert(view_id, pane_session.clone());

        // Spawn the singleton command broker on first start (it's
        // process-wide; subscribes to `event_tx`). Per-pane bash
        // routing is keyed by `terminalSessionId` on the kernel side,
        // so the existing broker works without modification.
        {
            let mut g = self.inner.lock();
            if g.command_broker_task.is_none() {
                let broker =
                    super::omw_command_broker::spawn_command_broker(self.clone(), &runtime);
                g.command_broker_task = Some(broker);
            }
        }
        Ok(pane_session)
    }

    /// Tear down a pane's session — aborts its WS task and removes the
    /// entry from `pane_sessions`. Called from
    /// [`Self::clear_active_terminal`] when a pane is closed (or
    /// optionally by callers that want to recycle a session).
    pub fn remove_pane_session(&self, view_id: warpui::EntityId) {
        if let Some(s) = self.pane_sessions.lock().remove(&view_id) {
            s.stop();
        }
    }

    /// Drop every per-pane session in one shot. Used by the settings page
    /// when the user clicks Apply: each pane caches its agent session
    /// against the config snapshot taken at first `# foo`, so without
    /// this the user would have to restart warp for the new
    /// provider/model/key to take effect.
    pub fn clear_all_pane_sessions(&self) {
        let drained: Vec<Arc<PaneSession>> =
            self.pane_sessions.lock().drain().map(|(_, s)| s).collect();
        for s in drained {
            s.stop();
        }
    }

    /// Pump the streaming response for `prompt` into the supplied
    /// terminal pane, using *the pane's own* kernel session. Mirrors
    /// [`Self::send_prompt_inline`] but scoped to one pane's context
    /// rather than the singleton session, so each pane carries its own
    /// conversation history.
    ///
    /// Resolves the session via `target.view_id` — if the pane has no
    /// session yet, the caller must call [`Self::start_pane_session`]
    /// first; this function does NOT auto-provision (the caller has
    /// the params and needs to surface validation errors).
    pub fn send_prompt_inline_for_pane(
        self: &Arc<Self>,
        prompt: String,
        target: ActiveTerminalHandle,
    ) -> Result<(), String> {
        let pane_session = self
            .pane_session(target.view_id)
            .ok_or_else(|| {
                format!(
                    "no agent session for pane {:?} — call start_pane_session first",
                    target.view_id
                )
            })?;
        log::info!(
            "omw# state: send_prompt_inline_for_pane view_id={:?} session_id={}",
            target.view_id,
            pane_session.session_id
        );

        // Subscribe BEFORE sending so the first deltas can't race past
        // the receiver registration.
        let mut events = pane_session.subscribe_events();
        let runtime = self
            .inner
            .lock()
            .runtime_handle
            .clone()
            .ok_or_else(|| "agent runtime not yet initialised".to_string())?;

        let event_loop_tx = target.event_loop_tx.clone();
        let inject = |bytes: Vec<u8>| -> Result<(), String> {
            let n = bytes.len();
            let msg = crate::terminal::writeable_pty::Message::InjectBytes(
                std::borrow::Cow::Owned(bytes),
            );
            event_loop_tx.lock().send(msg).map_err(|e| {
                log::warn!("omw# pump: inject {n} bytes FAILED: {e:?}");
                format!("inject into local-tty event loop: {e:?}")
            })?;
            Ok(())
        };

        let echo = format!("\r\n\x1b[2K# {prompt}\r\n");
        let _ = inject(echo.into_bytes());

        pane_session.send_prompt(prompt)?;
        log::info!("omw# state: send_prompt_inline_for_pane prompt sent; spawning pump");

        let pty_event_loop_tx = target.event_loop_tx.clone();
        let pump_session_id = pane_session.session_id.clone();
        runtime.spawn(async move {
            log::info!(
                "omw# pump (per-pane): started view_id={:?} session={pump_session_id}",
                target.view_id
            );
            const FRAME_DIM_CYAN: &str = "\x1b[2;36m";
            const FRAME_YELLOW: &str = "\x1b[33m";
            const FRAME_DIM: &str = "\x1b[2m";
            const FRAME_RED: &str = "\x1b[31m";
            const RESET: &str = "\x1b[0m";

            let send = |bytes: Vec<u8>| {
                let msg = crate::terminal::writeable_pty::Message::InjectBytes(
                    std::borrow::Cow::Owned(bytes),
                );
                let _ = pty_event_loop_tx.lock().send(msg);
            };

            while let Ok(event) = events.recv().await {
                match event {
                    super::omw_protocol::OmwAgentEventDown::AssistantDelta { delta, .. } => {
                        send(delta.into_bytes());
                    }
                    super::omw_protocol::OmwAgentEventDown::ToolCallStarted {
                        tool_name,
                        args,
                        ..
                    } => {
                        let summary = summarize_tool_args(&tool_name, &args);
                        let line = format!(
                            "\r\n{FRAME_DIM_CYAN}┌─ tool: {tool_name}{summary}{RESET}\r\n"
                        );
                        send(line.into_bytes());
                    }
                    super::omw_protocol::OmwAgentEventDown::ToolCallFinished {
                        tool_name,
                        is_error,
                        ..
                    } => {
                        let marker = if is_error { "✗ failed" } else { "✓ done" };
                        let color = if is_error { FRAME_RED } else { FRAME_DIM_CYAN };
                        let line = format!("{color}└─ {tool_name}: {marker}{RESET}\r\n");
                        send(line.into_bytes());
                    }
                    super::omw_protocol::OmwAgentEventDown::ApprovalRequest {
                        approval_id,
                        tool_call,
                        ..
                    } => {
                        let tool_name = tool_call
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("(unknown)");
                        let line = format!(
                            "\r\n{FRAME_YELLOW}⚠  approval needed for {tool_name} (id={approval_id})\r\n   open the agent panel to Approve / Reject{RESET}\r\n"
                        );
                        send(line.into_bytes());
                    }
                    super::omw_protocol::OmwAgentEventDown::Error { message, .. } => {
                        let line =
                            format!("\r\n{FRAME_RED}omw agent error: {message}{RESET}\r\n");
                        send(line.into_bytes());
                    }
                    super::omw_protocol::OmwAgentEventDown::AgentCrashed => {
                        send(format!(
                            "\r\n{FRAME_RED}omw agent kernel crashed (check ~/Library/Logs/warp-oss.log){RESET}\r\n"
                        ).into_bytes());
                        return;
                    }
                    super::omw_protocol::OmwAgentEventDown::TurnFinished { .. } => {
                        send(
                            format!("{FRAME_DIM}\r\n[turn finished]{RESET}\r\n").into_bytes(),
                        );
                        return;
                    }
                    super::omw_protocol::OmwAgentEventDown::ExecCommand { .. }
                    | super::omw_protocol::OmwAgentEventDown::CommandData { .. }
                    | super::omw_protocol::OmwAgentEventDown::CommandExit { .. } => {}
                }
            }
            log::warn!(
                "omw# pump (per-pane): events.recv() returned Err — channel closed/lagged; exiting"
            );
        });
        Ok(())
    }

    /// Convenience wrapper around [`start`] that loads `omw-config` and
    /// resolves the default provider into [`OmwAgentSessionParams`]. Returns
    /// `Err` if no provider is configured, the agent is disabled, or the
    /// default provider points to a missing entry.
    pub fn start_with_config(self: &Arc<Self>) -> Result<(), String> {
        log::info!("omw# state: start_with_config entry");
        let cfg = omw_config::Config::load().map_err(|e| e.to_string())?;
        log::info!(
            "omw# state: config loaded; agent.enabled={} default_provider={:?} providers_n={}",
            cfg.agent.enabled,
            cfg.default_provider,
            cfg.providers.len()
        );
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
        log::info!(
            "omw# state: selected provider={} kind={}",
            provider_id,
            provider.kind_str()
        );

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
                omw_config::ProviderConfig::OpenAi { base_url, .. } => {
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

    /// Insert (or replace) the io handles for the given pane. Called
    /// from `TerminalView::on_pane_state_change` for every local-tty
    /// pane, regardless of whether it is currently focused — so the
    /// inline-agent path (which keys off the submitting `Input`'s own
    /// `terminal_view_id`) can always find the right handles.
    pub fn register_pane_io(&self, handle: ActiveTerminalHandle) {
        self.pane_io.lock().insert(handle.view_id, handle);
    }

    /// Look up the io handles registered for `view_id`. Returns `None`
    /// when the pane is remote/SSH (no local handles), is detached, or
    /// hasn't yet gone through its first `on_pane_state_change`.
    pub fn pane_io_clone(&self, view_id: warpui::EntityId) -> Option<ActiveTerminalHandle> {
        self.pane_io.lock().get(&view_id).cloned()
    }

    /// Drop a pane's io entry — call when the pane closes so the map
    /// doesn't grow unboundedly across long sessions. Currently
    /// unwired (panes leak until process exit, but the map is bounded
    /// by simultaneous pane count, which is small).
    #[allow(dead_code)]
    pub fn remove_pane_io(&self, view_id: warpui::EntityId) {
        self.pane_io.lock().remove(&view_id);
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

    /// Test-only: install a synthetic [`PaneSession`] for `view_id`
    /// without going through the WS. The supplied outbound mpsc lets
    /// the test capture `OmwAgentEventUp` frames the per-pane code
    /// path emits; the returned `event_tx` lets the test inject
    /// inbound events that simulate the WS reader. Returns
    /// `(event_tx, replaced)` where `replaced` is `Some(_)` if a prior
    /// session was overwritten.
    #[cfg(any(test, feature = "test-exports"))]
    pub fn test_install_pane_session(
        &self,
        view_id: warpui::EntityId,
        outbound: mpsc::Sender<OmwAgentEventUp>,
    ) -> (
        broadcast::Sender<OmwAgentEventDown>,
        Option<Arc<PaneSession>>,
    ) {
        let (event_tx, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        let (status_tx, _) = watch::channel(OmwAgentStatus::Connected {
            session_id: format!("test-{view_id:?}"),
        });
        let session = Arc::new(PaneSession {
            session_id: format!("test-{view_id:?}"),
            outbound,
            ws_task: Mutex::new(None),
            event_tx: event_tx.clone(),
            status_tx,
        });
        let replaced = self.pane_sessions.lock().insert(view_id, session);
        (event_tx, replaced)
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

    log::info!("omw# session: run_session entry server_url={server_url}");
    let session_id = match create_session(&server_url, &params).await {
        Ok(id) => {
            log::info!("omw# session: create_session OK id={id}");
            id
        }
        Err(e) => {
            log::warn!("omw# session: create_session FAILED: {e}");
            set_status(OmwAgentStatus::Failed { error: e });
            return;
        }
    };

    let ws_url = match server_url.strip_prefix("http://") {
        Some(rest) => format!("ws://{rest}/ws/v1/agent/{session_id}"),
        None => match server_url.strip_prefix("https://") {
            Some(rest) => format!("wss://{rest}/ws/v1/agent/{session_id}"),
            None => {
                log::warn!("omw# session: server_url has no http(s) prefix");
                set_status(OmwAgentStatus::Failed {
                    error: "OMW_SERVER_URL must start with http:// or https://".into(),
                });
                return;
            }
        },
    };

    log::info!("omw# session: dialing WS at {ws_url}");
    let connect = match tokio_tungstenite::connect_async(&ws_url).await {
        Ok((stream, _)) => {
            log::info!("omw# session: WS connected");
            stream
        }
        Err(e) => {
            log::warn!("omw# session: WS connect FAILED: {e}");
            set_status(OmwAgentStatus::Failed {
                error: format!("ws connect: {e}"),
            });
            return;
        }
    };

    set_status(OmwAgentStatus::Connected {
        session_id: session_id.clone(),
    });
    log::info!("omw# session: status=Connected; entering message loop");

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
                log::trace!("omw# session: ws<- {} bytes", text.len());
                let parsed = serde_json::from_str::<OmwAgentEventDown>(&text);
                if let Err(ref e) = parsed {
                    log::warn!("omw# session: ws frame deserialize FAILED: {e} ; raw_prefix={}", &text.chars().take(120).collect::<String>());
                }
                if let Ok(event) = parsed {
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
                    let n = event_tx.send(event).map(|n| n as i64).unwrap_or(-1);
                    log::trace!("omw# session: ws-> broadcast subscribers={n}");
                }
            }

            // Outbound from UI -> WS as JSON text.
            up = outbound.recv() => {
                let Some(up) = up else { log::info!("omw# session: outbound channel closed; exiting loop"); break };
                let kind = match &up {
                    OmwAgentEventUp::Prompt { .. } => "Prompt",
                    OmwAgentEventUp::Cancel => "Cancel",
                    OmwAgentEventUp::ApprovalDecision { .. } => "ApprovalDecision",
                    OmwAgentEventUp::CommandData { .. } => "CommandData",
                    OmwAgentEventUp::CommandExit { .. } => "CommandExit",
                };
                log::info!("omw# session: outbound frame={kind}");
                let line = match serde_json::to_string(&up) {
                    Ok(s) => s,
                    Err(e) => { log::warn!("omw# session: serialize FAILED: {e}"); continue; }
                };
                match sink.send(WsMessage::Text(line)).await {
                    Ok(_) => log::info!("omw# session: ws<- frame={kind} sent"),
                    Err(e) => {
                        log::warn!("omw# session: ws send FAILED: {e}; exiting loop");
                        break;
                    }
                }
            }
        }
    }

    log::info!("omw# session: loop exited; setting status=Idle");
    // Stream ended — clear status.
    set_status(OmwAgentStatus::Idle);
    let _ = sink.close().await;
    if let Some(state) = weak_self.upgrade() {
        let mut g = state.inner.lock();
        g.outbound = None;
        g.ws_task = None;
    }
}

/// Provision a fresh kernel session for a pane and park its WS
/// reader/writer. Returns the [`PaneSession`] only after the WS is
/// connected, so callers can immediately invoke
/// [`PaneSession::send_prompt`] without racing.
async fn boot_pane_session(
    view_id: warpui::EntityId,
    server_url: String,
    params: OmwAgentSessionParams,
    weak_self: std::sync::Weak<OmwAgentState>,
) -> Result<Arc<PaneSession>, String> {
    log::info!("omw# pane-session: boot view_id={view_id:?} server_url={server_url}");
    let session_id = create_session(&server_url, &params)
        .await
        .map_err(|e| format!("create_session: {e}"))?;
    log::info!("omw# pane-session: create_session OK id={session_id}");

    let ws_url = match server_url.strip_prefix("http://") {
        Some(rest) => format!("ws://{rest}/ws/v1/agent/{session_id}"),
        None => match server_url.strip_prefix("https://") {
            Some(rest) => format!("wss://{rest}/ws/v1/agent/{session_id}"),
            None => return Err("OMW_SERVER_URL must start with http:// or https://".into()),
        },
    };
    use futures_util::StreamExt as _;
    let (ws_stream, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .map_err(|e| format!("ws connect: {e}"))?;
    let (mut sink, mut stream) = ws_stream.split();
    log::info!("omw# pane-session: WS connected to {ws_url}");

    let (out_tx, mut out_rx) = mpsc::channel::<OmwAgentEventUp>(OUTBOUND_CHANNEL_CAPACITY);
    let (event_tx, _) = broadcast::channel::<OmwAgentEventDown>(EVENT_CHANNEL_CAPACITY);
    let (status_tx, _) = watch::channel(OmwAgentStatus::Connected {
        session_id: session_id.clone(),
    });

    let event_tx_for_task = event_tx.clone();
    let status_tx_for_task = status_tx.clone();
    let session_id_for_task = session_id.clone();
    let view_id_for_task = view_id;

    let ws_task = tokio::spawn(async move {
        use futures_util::{SinkExt, StreamExt};
        use tokio_tungstenite::tungstenite::Message as WsMessage;

        loop {
            tokio::select! {
                msg = stream.next() => {
                    let Some(msg) = msg else { break };
                    let msg = match msg { Ok(m) => m, Err(_) => break };
                    let text = match msg {
                        WsMessage::Text(t) => t,
                        WsMessage::Binary(b) => String::from_utf8_lossy(&b).into_owned(),
                        WsMessage::Close(_) => break,
                        _ => continue,
                    };
                    if let Ok(event) = serde_json::from_str::<OmwAgentEventDown>(&text) {
                        // Status-watch: Streaming on first delta /
                        // tool_call; Connected on turn_finished.
                        match &event {
                            OmwAgentEventDown::AssistantDelta { .. }
                            | OmwAgentEventDown::ToolCallStarted { .. } => {
                                if !matches!(*status_tx_for_task.borrow(), OmwAgentStatus::Streaming { .. }) {
                                    let _ = status_tx_for_task.send(OmwAgentStatus::Streaming {
                                        session_id: session_id_for_task.clone(),
                                    });
                                }
                            }
                            OmwAgentEventDown::TurnFinished { .. } => {
                                let _ = status_tx_for_task.send(OmwAgentStatus::Connected {
                                    session_id: session_id_for_task.clone(),
                                });
                            }
                            _ => {}
                        }
                        // Per-pane fan-out — only this pane's pump sees it.
                        let _ = event_tx_for_task.send(event.clone());
                        // Also re-publish on the global bus so the
                        // command broker (singleton, keyed by
                        // terminalSessionId) can route bash/exec.
                        if let Some(state) = weak_self.upgrade() {
                            let _ = state.event_tx.send(event);
                        }
                    }
                }
                up = out_rx.recv() => {
                    let Some(up) = up else { break };
                    let line = match serde_json::to_string(&up) { Ok(s) => s, Err(_) => continue };
                    if sink.send(WsMessage::Text(line)).await.is_err() { break; }
                }
            }
        }
        log::info!(
            "omw# pane-session: WS loop exited view_id={view_id_for_task:?} session={session_id_for_task}"
        );
        let _ = status_tx_for_task.send(OmwAgentStatus::Idle);
        let _ = sink.close().await;
        // Drop the entry so a future # in this pane re-provisions.
        if let Some(state) = weak_self.upgrade() {
            state.pane_sessions.lock().remove(&view_id_for_task);
        }
    });

    Ok(Arc::new(PaneSession {
        session_id,
        outbound: out_tx,
        ws_task: Mutex::new(Some(ws_task)),
        event_tx,
        status_tx,
    }))
}

async fn create_session(
    server_url: &str,
    params: &OmwAgentSessionParams,
) -> Result<String, String> {
    // CRITICAL: explicitly disable proxies. The GUI's session/create
    // POST targets the in-process omw-server on `http://127.0.0.1:8788`
    // — but reqwest's default builder honors system + env proxy config
    // (`https_proxy`, macOS network proxy panel). On developer machines
    // running Clash / Telegram-style local proxies, the request gets
    // forwarded to the user's HTTP proxy (also on 127.0.0.1, but a
    // different port), which cannot proxy localhost-to-localhost
    // traffic and returns 502 Bad Gateway. The result: every `# hi`
    // appears to fail with a kernel-side bug while the kernel is
    // perfectly healthy. Using `.no_proxy()` forces reqwest to dial
    // the loopback directly. (Reproduced 2026-05-07 with
    // `https_proxy=http://127.0.0.1:6789`.)
    let client = reqwest::Client::builder()
        .no_proxy()
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
    log::info!("omw# session: POST {url}");
    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("post session: {e}"))?;
    log::info!("omw# session: POST status={}", resp.status());
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

/// Render a one-line preview of a tool call's arguments for the inline
/// frame. We special-case `bash` (the most common tool) by extracting
/// `command` and truncating; other tools fall back to a generic
/// JSON-keys preview. Limited to ~80 chars so the frame stays
/// terminal-width-safe.
fn summarize_tool_args(tool_name: &str, args: &serde_json::Value) -> String {
    const MAX: usize = 80;
    if tool_name == "bash" {
        if let Some(cmd) = args.get("command").and_then(|v| v.as_str()) {
            let mut s = cmd.replace('\n', " ⏎ ");
            if s.chars().count() > MAX {
                s = s.chars().take(MAX).collect::<String>() + "…";
            }
            return format!(" `{s}`");
        }
    }
    if let Some(obj) = args.as_object() {
        let keys: Vec<&str> = obj.keys().map(|k| k.as_str()).collect();
        if !keys.is_empty() {
            return format!(" ({})", keys.join(", "));
        }
    }
    String::new()
}
