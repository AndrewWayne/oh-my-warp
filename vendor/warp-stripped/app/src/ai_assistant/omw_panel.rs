//! Phase 3c — agent panel render layer.
//!
//! `render_omw_agent_panel` is the single entry point: panel.rs calls it
//! instead of the old `is_omw_placeholder` text block. The render is
//! intentionally text-only for now (v0 / Task 8); prompt-editor wiring
//! is deferred to Task 11 as noted in the progress doc.
//!
//! The L3a tests in `omw_agent_panel_test.rs` exercise
//! `OmwAgentTranscriptModel::apply_event` directly and do not call into
//! this render function, so the render only needs to compile cleanly.

use warpui::elements::{Align, Container, Flex, MainAxisSize, ParentElement, Shrinkable};
use warpui::elements::Element;
use warpui::ui_components::components::{UiComponent, UiComponentStyles};

use crate::appearance::Appearance;
use super::omw_transcript::{OmwAgentMessage, OmwAgentTranscriptModel, ToolCallStatus};
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
