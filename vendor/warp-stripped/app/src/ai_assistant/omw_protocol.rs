//! Wire protocol types for the GUI ↔ omw-server agent WebSocket.
//!
//! `OmwAgentEventDown` is a typed view of the JSON-RPC notification frames
//! omw-server forwards from the omw-agent stdio kernel. The notifications
//! carry a `method` discriminator (e.g. `"assistant/delta"`) and a `params`
//! payload; `serde(tag = "method", content = "params")` matches that shape
//! 1:1 so deserialising a raw kernel frame into this enum is a single
//! `serde_json::from_str` call.
//!
//! `OmwAgentEventUp` is the GUI → server direction. Today (Phase 3) the
//! handler accepts `{kind:"prompt", prompt:"..."}` and `{kind:"cancel"}`.
//! Approval-decision and bash-broker variants land dormant here so Phase 4
//! (approvals) and Phase 5 (bash) only need to flip serde tags, not redefine
//! the type.

use serde::{Deserialize, Serialize};

/// Server → GUI event.
///
/// The variant payloads mirror `apps/omw-agent/src/serve.ts` notification
/// shapes. JSON-RPC framing fields (`jsonrpc`, `id`) are not modelled —
/// serde silently drops unknown fields, so the raw notification frames
/// from the WS round-trip cleanly.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "method", content = "params")]
pub enum OmwAgentEventDown {
    #[serde(rename = "assistant/delta")]
    AssistantDelta {
        #[serde(rename = "sessionId")]
        session_id: String,
        delta: String,
    },
    #[serde(rename = "tool/call_started")]
    ToolCallStarted {
        #[serde(rename = "sessionId")]
        session_id: String,
        #[serde(rename = "toolCallId")]
        tool_call_id: String,
        #[serde(rename = "toolName")]
        tool_name: String,
        args: serde_json::Value,
    },
    #[serde(rename = "tool/call_finished")]
    ToolCallFinished {
        #[serde(rename = "sessionId")]
        session_id: String,
        #[serde(rename = "toolCallId")]
        tool_call_id: String,
        #[serde(rename = "toolName")]
        tool_name: String,
        #[serde(rename = "isError")]
        is_error: bool,
    },
    #[serde(rename = "turn/finished")]
    TurnFinished {
        #[serde(rename = "sessionId")]
        session_id: String,
        cancelled: bool,
    },
    /// Phase 5 — approval queue. Variant exists today so future flips
    /// don't require a protocol re-serialize. The `tool_call` payload is
    /// kept as `serde_json::Value` because its shape (pi-ai `ToolCall`)
    /// is owned by the kernel; the GUI only renders metadata + name.
    #[serde(rename = "approval/request")]
    ApprovalRequest {
        #[serde(rename = "sessionId")]
        session_id: String,
        #[serde(rename = "approvalId")]
        approval_id: String,
        #[serde(rename = "toolCall")]
        tool_call: serde_json::Value,
    },
    /// Phase 5 — bash broker outbound. Same dormancy rationale.
    #[serde(rename = "bash/exec")]
    ExecCommand {
        #[serde(rename = "sessionId")]
        session_id: String,
        #[serde(rename = "commandId")]
        command_id: String,
        command: String,
        cwd: Option<String>,
    },
    #[serde(rename = "bash/data")]
    CommandData {
        #[serde(rename = "sessionId")]
        session_id: String,
        #[serde(rename = "commandId")]
        command_id: String,
        /// base64-encoded byte chunk from the pane PTY. Field rename
        /// matches the kernel's `frame.params.bytes` reader and
        /// omw-server's WS forwarder; aligned with the Up direction's
        /// `OmwAgentEventUp::CommandData` rename.
        #[serde(rename = "bytes")]
        data: String,
    },
    #[serde(rename = "bash/finished")]
    CommandExit {
        #[serde(rename = "sessionId")]
        session_id: String,
        #[serde(rename = "commandId")]
        command_id: String,
        #[serde(rename = "exitCode")]
        exit_code: Option<i32>,
        #[serde(default)]
        snapshot: bool,
    },
    /// Process-level crash signal. Fan-out from omw-server's reader task
    /// when the agent stdio child exits unexpectedly.
    #[serde(rename = "agent/crashed")]
    AgentCrashed,
    /// Generic error notification. `session_id` may be missing for
    /// process-scoped errors.
    #[serde(rename = "error")]
    Error {
        #[serde(rename = "sessionId", default)]
        session_id: Option<String>,
        message: String,
    },
}

/// Approval decision the GUI sends back to the kernel for a previously
/// emitted `approval/request`. Wire form mirrors
/// `apps/omw-agent/src/policy-hook.ts:35`'s `"approve" | "reject" | "cancel"`
/// — the snake_case rename yields exactly those single-word lowercase
/// strings.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    Approve,
    Reject,
    /// Used when the GUI cancels an in-flight approval (e.g. user closes
    /// the panel mid-prompt). Phase 5b wires the cancel path on the GUI side.
    Cancel,
}

/// GUI → server event over the WS. Phase 3 supports prompt + cancel;
/// approval-decision and bash variants are dormant (Phase 4 / 5).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OmwAgentEventUp {
    Prompt { prompt: String },
    Cancel,
    /// Phase 5.
    ApprovalDecision {
        #[serde(rename = "approvalId")]
        approval_id: String,
        decision: ApprovalDecision,
    },
    /// Phase 5 — pane bash broker reply: pane PTY chunk back to the agent.
    /// Field names match what `omw-server`'s WS handler reads (`bytes`)
    /// and what the kernel's `warp-session-bash.ts:105` parses
    /// (`frame.params.bytes`). Earlier serde used `data` here, which the
    /// server treated as None and forwarded an empty string to the kernel
    /// — producing tool results with no stdout/stderr.
    CommandData {
        #[serde(rename = "commandId")]
        command_id: String,
        /// base64-encoded byte chunk.
        #[serde(rename = "bytes")]
        data: String,
    },
    /// Phase 5 — pane bash broker reply: command lifecycle terminator.
    /// `exit_code` is renamed to `exitCode` to match the server's WS
    /// parse (`parsed.get("exitCode")`) and the kernel's
    /// `frame.params.exitCode` reader. Without this rename every
    /// CommandExit landed at the kernel as `exitCode: null`, surfacing
    /// to the agent as `[exit unknown]` even when the broker correctly
    /// detected the CommandFinished marker with a real `exit_code: 0`.
    CommandExit {
        #[serde(rename = "commandId")]
        command_id: String,
        #[serde(rename = "exitCode")]
        exit_code: Option<i32>,
        #[serde(default)]
        snapshot: bool,
    },
}
