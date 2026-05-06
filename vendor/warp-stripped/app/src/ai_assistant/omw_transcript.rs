//! Pure-data transcript model for the omw-local agent panel.
//!
//! Mirrors the role of `transcript::Transcript`/`requests::Requests` in
//! the upstream cloud-AI panel, but is independent — no `ServerApi`, no
//! `AIClient`, no `RequestStatus`. Phase 3 wires this against omw-server
//! agent events; the upstream `Transcript` stays compiled-but-dormant
//! under `omw_local` (smaller diff, easier upstream sync per CLAUDE.md §5).
//!
//! `apply_event` is the only mutator. The view layer (Phase 3b's
//! `omw_panel.rs`) reads `messages()` and renders.

use super::omw_protocol::OmwAgentEventDown;

/// Status of a tool call inside the transcript.
#[derive(Debug, Clone, PartialEq)]
pub enum ToolCallStatus {
    /// Call surfaced; the kernel hasn't finished it yet.
    Running,
    /// Call finished cleanly.
    Done,
    /// Call returned an error.
    Failed,
}

/// Approval card decision state.
#[derive(Debug, Clone, PartialEq)]
pub enum ApprovalDecision {
    Pending,
    Approved,
    Rejected,
    Cancelled,
}

/// One row in the transcript.
#[derive(Debug, Clone, PartialEq)]
pub enum OmwAgentMessage {
    User {
        text: String,
    },
    Assistant {
        /// Concatenated text from `assistant/delta` events. May be empty
        /// during the streaming phase.
        text: String,
        /// `true` once we've seen `turn/finished` for the assistant turn
        /// that produced this message.
        finished: bool,
    },
    ToolCall {
        id: String,
        name: String,
        status: ToolCallStatus,
    },
    Approval {
        id: String,
        /// Human-readable summary (e.g. the bash command). Phase 5
        /// derives this from the `tool_call` JSON.
        summary: String,
        decision: ApprovalDecision,
    },
    /// Surfaced when the kernel emits an `error` notification mid-turn,
    /// or when the agent crashes outright.
    Error {
        message: String,
    },
}

/// In-memory transcript model. Cheap to clone (Vec).
#[derive(Debug, Clone, Default)]
pub struct OmwAgentTranscriptModel {
    messages: Vec<OmwAgentMessage>,
}

impl OmwAgentTranscriptModel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn messages(&self) -> &[OmwAgentMessage] {
        &self.messages
    }

    /// Append a user-typed prompt. Called by the panel's editor before
    /// the prompt is sent over the WS.
    pub fn push_user(&mut self, text: String) {
        self.messages.push(OmwAgentMessage::User { text });
        // Reserve a streaming-assistant row so deltas have somewhere to
        // accumulate without a race against `apply_event`.
        self.messages.push(OmwAgentMessage::Assistant {
            text: String::new(),
            finished: false,
        });
    }

    /// Apply an inbound kernel event to the transcript. Idempotent for
    /// terminal events (a duplicate `turn/finished` is a no-op).
    pub fn apply_event(&mut self, event: &OmwAgentEventDown) {
        match event {
            OmwAgentEventDown::AssistantDelta { delta, .. } => {
                if let Some(OmwAgentMessage::Assistant { text, finished }) = self.messages.last_mut() {
                    if !*finished {
                        text.push_str(delta);
                        return;
                    }
                }
                // No streaming row in place — start one. This handles
                // the "delta arrives before push_user reserved the slot"
                // race (shouldn't happen in production, but harmless).
                self.messages.push(OmwAgentMessage::Assistant {
                    text: delta.clone(),
                    finished: false,
                });
            }
            OmwAgentEventDown::TurnFinished { .. } => {
                if let Some(OmwAgentMessage::Assistant { finished, .. }) = self.messages.last_mut() {
                    *finished = true;
                }
            }
            OmwAgentEventDown::ToolCallStarted {
                tool_call_id,
                tool_name,
                ..
            } => {
                self.messages.push(OmwAgentMessage::ToolCall {
                    id: tool_call_id.clone(),
                    name: tool_name.clone(),
                    status: ToolCallStatus::Running,
                });
            }
            OmwAgentEventDown::ToolCallFinished {
                tool_call_id,
                is_error,
                ..
            } => {
                if let Some(card) = self
                    .messages
                    .iter_mut()
                    .rev()
                    .find_map(|m| match m {
                        OmwAgentMessage::ToolCall { id, status, .. } if id == tool_call_id => {
                            Some(status)
                        }
                        _ => None,
                    })
                {
                    *card = if *is_error {
                        ToolCallStatus::Failed
                    } else {
                        ToolCallStatus::Done
                    };
                }
            }
            OmwAgentEventDown::ApprovalRequest {
                approval_id,
                tool_call,
                ..
            } => {
                let summary = tool_call
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("tool call")
                    .to_string();
                self.messages.push(OmwAgentMessage::Approval {
                    id: approval_id.clone(),
                    summary,
                    decision: ApprovalDecision::Pending,
                });
            }
            OmwAgentEventDown::Error { message, .. } => {
                self.messages.push(OmwAgentMessage::Error {
                    message: message.clone(),
                });
            }
            OmwAgentEventDown::AgentCrashed => {
                self.messages.push(OmwAgentMessage::Error {
                    message: "agent process crashed; restart omw-server to recover".to_string(),
                });
            }
            // Phase 5 broker variants are observed by the bash broker, not
            // the transcript view. Ignore here.
            OmwAgentEventDown::ExecCommand { .. }
            | OmwAgentEventDown::CommandData { .. }
            | OmwAgentEventDown::CommandExit { .. } => {}
        }
    }

    /// Update an approval card's decision. Called by the WS sender when
    /// the user clicks Approve/Reject and the upstream message is sent.
    pub fn update_approval(&mut self, approval_id: &str, decision: ApprovalDecision) {
        for msg in self.messages.iter_mut().rev() {
            if let OmwAgentMessage::Approval {
                id,
                decision: d,
                ..
            } = msg
            {
                if id == approval_id {
                    *d = decision;
                    return;
                }
            }
        }
    }
}
