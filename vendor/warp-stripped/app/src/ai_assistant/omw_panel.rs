//! Phase 3c — agent panel render layer.
//!
//! `render_omw_agent_panel` is the single entry point: panel.rs calls it
//! instead of the old `is_omw_placeholder` text block. The render is
//! mostly text rows; the Phase 4c4 approval card adds Approve/Reject
//! buttons whose `on_click` handlers call into
//! [`OmwAgentState::send_approval_decision`] directly (no view-context
//! action plumbing needed because the agent state is a process-wide
//! singleton).
//!
//! The L3a tests in `omw_agent_panel_test.rs` exercise
//! `OmwAgentTranscriptModel::apply_event` directly and do not call into
//! this render function, so the render only needs to compile cleanly.

use warpui::elements::{Align, Container, CrossAxisAlignment, Flex, MainAxisSize, MouseStateHandle, ParentElement, Shrinkable};
use warpui::elements::Element;
use warpui::ui_components::button::ButtonVariant;
use warpui::ui_components::components::{UiComponent, UiComponentStyles};

use crate::appearance::Appearance;
use super::omw_protocol::ApprovalDecision as ProtocolApprovalDecision;
use super::omw_transcript::{ApprovalDecision as TranscriptApprovalDecision, OmwAgentMessage, OmwAgentTranscriptModel, ToolCallStatus};
use super::omw_agent_state::OmwAgentState;

const BODY_FONT_SIZE: f32 = 13.;
const PANEL_PADDING: f32 = 16.;

/// Render the agent panel.
///
/// Minimal v0 render: status line + transcript messages as text rows.
/// Prompt editor and click handlers are scoped for Task 11.
pub fn render_omw_agent_panel(
    transcript: &OmwAgentTranscriptModel,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let theme = appearance.theme();

    let status_text = format!("Agent status: {:?}", OmwAgentState::shared().status());

    let mut col = Flex::column().with_main_axis_size(MainAxisSize::Min);

    // Status line.
    col.add_child(
        appearance
            .ui_builder()
            .wrappable_text(status_text, false)
            .with_style(UiComponentStyles {
                font_family_id: Some(appearance.ui_font_family()),
                font_size: Some(BODY_FONT_SIZE),
                font_color: Some(theme.nonactive_ui_text_color().into()),
                ..Default::default()
            })
            .build()
            .finish(),
    );

    // Message rows.
    for message in transcript.messages() {
        // Pending approvals get a card with Approve/Reject buttons. Other
        // states (Approved/Rejected/Cancelled) and other message variants
        // fall through to the text-summary path below.
        if let OmwAgentMessage::Approval {
            id,
            session_id,
            summary,
            decision: TranscriptApprovalDecision::Pending,
        } = message
        {
            col.add_child(render_approval_card(appearance, id, session_id, summary));
            continue;
        }

        let line = format_message_summary(message);
        col.add_child(
            appearance
                .ui_builder()
                .wrappable_text(line, true)
                .with_style(UiComponentStyles {
                    font_family_id: Some(appearance.ui_font_family()),
                    font_size: Some(BODY_FONT_SIZE),
                    font_color: Some(theme.nonactive_ui_text_color().into()),
                    ..Default::default()
                })
                .build()
                .finish(),
        );
    }

    Align::new(
        Container::new(Shrinkable::new(1., col.finish()).finish())
            .with_uniform_padding(PANEL_PADDING)
            .finish(),
    )
    .finish()
}

fn format_message_summary(message: &OmwAgentMessage) -> String {
    match message {
        OmwAgentMessage::User { text } => format!("You: {}", text),
        OmwAgentMessage::Assistant { text, finished } => {
            if *finished {
                format!("Agent: {}", text)
            } else {
                format!("Agent: {}…", text)
            }
        }
        OmwAgentMessage::ToolCall { name, status, .. } => {
            let status_str = match status {
                ToolCallStatus::Running => "running",
                ToolCallStatus::Done => "done",
                ToolCallStatus::Failed => "failed",
            };
            format!("Tool [{}]: {}", status_str, name)
        }
        OmwAgentMessage::Approval { summary, decision, .. } => {
            format!("Approval [{:?}]: {}", decision, summary)
        }
        OmwAgentMessage::Error { message } => format!("Error: {}", message),
    }
}

/// Render the approval card row for a pending decision. Two buttons —
/// `Approve` and `Reject` — call into the kernel session that issued the
/// `approval/request`. When that session is a per-pane `# foo` session
/// the decision routes via [`PaneSession::send_approval_decision`]; the
/// AI-panel singleton session falls back to
/// [`OmwAgentState::send_approval_decision`]. We don't optimistically
/// update the local transcript model here; the kernel resolves the
/// decision and the next turn surfaces any follow-up state.
fn render_approval_card(
    appearance: &Appearance,
    approval_id: &str,
    session_id: &str,
    summary: &str,
) -> Box<dyn Element> {
    let theme = appearance.theme();
    let summary_text = format!("Approval needed: {}", summary);

    let summary_el = appearance
        .ui_builder()
        .wrappable_text(summary_text, true)
        .with_style(UiComponentStyles {
            font_family_id: Some(appearance.ui_font_family()),
            font_size: Some(BODY_FONT_SIZE),
            font_color: Some(theme.active_ui_text_color().into()),
            ..Default::default()
        })
        .build()
        .finish();

    let approve_id = approval_id.to_string();
    let approve_session = session_id.to_string();
    let approve_btn = appearance
        .ui_builder()
        .button(ButtonVariant::Accent, MouseStateHandle::default())
        .with_text_label("Approve".to_owned())
        .build()
        .on_click(move |_ctx, _app, _pt| {
            send_decision(&approve_session, &approve_id, ProtocolApprovalDecision::Approve);
        })
        .finish();

    let reject_id = approval_id.to_string();
    let reject_session = session_id.to_string();
    let reject_btn = appearance
        .ui_builder()
        .button(ButtonVariant::Text, MouseStateHandle::default())
        .with_text_label("Reject".to_owned())
        .build()
        .on_click(move |_ctx, _app, _pt| {
            send_decision(&reject_session, &reject_id, ProtocolApprovalDecision::Reject);
        })
        .finish();

    let buttons = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_child(Container::new(approve_btn).with_margin_right(8.).finish())
        .with_child(reject_btn);

    Container::new(
        Flex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_child(Container::new(summary_el).with_margin_bottom(6.).finish())
            .with_child(buttons.finish())
            .finish(),
    )
    .with_margin_top(6.)
    .with_margin_bottom(6.)
    .finish()
}

/// Pick the right WS for the approval and dispatch the decision. Per-pane
/// sessions own their own outbound mpsc (each `# foo` flow); the
/// singleton [`OmwAgentState`] outbound is only correct for the AI-panel
/// session. Looking up by `session_id` ensures the kernel session that
/// asked for approval is the one that hears the answer.
fn send_decision(session_id: &str, approval_id: &str, decision: ProtocolApprovalDecision) {
    let state = OmwAgentState::shared();
    let result = match state.pane_session_by_id(session_id) {
        Some((_, pane)) => pane.send_approval_decision(approval_id.to_string(), decision),
        None => state.send_approval_decision(approval_id.to_string(), decision),
    };
    if let Err(e) = result {
        log::warn!("omw approval: send decision failed: {e}");
    }
}
