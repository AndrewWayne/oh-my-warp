// SPDX-License-Identifier: AGPL-3.0-only
//
// omw-authored file in the in-tree Warp fork (warpdotdev/warp), part of the
// AGPL-3.0 derivative work. See specs/fork-strategy.md §3.
//
// Copyright (C) 2026 Shenhao Miao and the omw contributors
// Copyright (C) 2020-2026 Denver Technologies, Inc.
//
// This program is free software: you can redistribute it and/or modify it
// under the terms of the GNU Affero General Public License, version 3, as
// published by the Free Software Foundation. See the LICENSE file at the
// repository root for the full text.

//! Phase 3c — agent panel render layer.
//!
//! `render_omw_agent_panel` is the single entry point: panel.rs calls it
//! instead of the old `is_omw_placeholder` text block. The render is
//! mostly text rows; the Phase 4c4 approval card adds Approve/Reject
//! buttons whose `on_click` handlers dispatch a typed action to the owning
//! panel, which routes the decision to the correct per-pane session.
//!
//! The L3a tests in `omw_agent_panel_test.rs` exercise
//! `OmwAgentTranscriptModel::apply_event` directly and do not call into
//! this render function, so the render only needs to compile cleanly.

use std::collections::HashMap;

use warpui::elements::{Align, Container, CrossAxisAlignment, Flex, MainAxisSize, MouseStateHandle, ParentElement, Shrinkable};
use warpui::elements::Element;
use warpui::ui_components::button::ButtonVariant;
use warpui::ui_components::components::{UiComponent, UiComponentStyles};

use crate::appearance::Appearance;
use super::omw_protocol::ApprovalDecision;
use super::omw_transcript::{ApprovalCardStatus, OmwAgentMessage, OmwAgentTranscriptModel, ToolCallStatus};
use super::omw_agent_state::{OmwAgentState, OmwAgentStatus};
use super::panel::AIAssistantAction;

const BODY_FONT_SIZE: f32 = 13.;
const PANEL_PADDING: f32 = 16.;

/// Stable pointer state for one approval card. These handles must outlive a
/// render: mouse-down can trigger a repaint before mouse-up, and recreating the
/// handles during that repaint prevents the button from completing its click.
#[derive(Clone, Default)]
pub(crate) struct ApprovalButtonMouseStates {
    approve: MouseStateHandle,
    reject: MouseStateHandle,
}

/// Render the agent panel.
///
/// Minimal v0 render: status line + transcript messages as text rows.
/// Prompt editor and click handlers are scoped for Task 11.
pub(crate) fn render_omw_agent_panel(
    transcript: &OmwAgentTranscriptModel,
    appearance: &Appearance,
    approval_mouse_states: &HashMap<String, ApprovalButtonMouseStates>,
) -> Box<dyn Element> {
    let theme = appearance.theme();

    let status_text = format_agent_status(&OmwAgentState::shared().status());

    let mut col = Flex::column().with_main_axis_size(MainAxisSize::Min);

    // Status line.
    col.add_child(
        appearance
            .ui_builder()
            .wrappable_text(status_text, true)
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
            decision: ApprovalCardStatus::Pending,
        } = message
        {
            let mouse_states = approval_mouse_states.get(id).cloned().unwrap_or_default();
            col.add_child(render_approval_card(
                appearance,
                id,
                session_id,
                summary,
                mouse_states,
            ));
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

pub(crate) fn format_agent_status(status: &OmwAgentStatus) -> String {
    match status {
        OmwAgentStatus::Idle => "Agent status: Idle".to_owned(),
        OmwAgentStatus::Starting => "Agent status: Starting".to_owned(),
        OmwAgentStatus::Connected { .. } => "Agent status: Connected".to_owned(),
        OmwAgentStatus::Streaming { .. } => "Agent status: Streaming".to_owned(),
        OmwAgentStatus::Failed { error } => format!("Agent status: Failed - {error}"),
    }
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
/// `Approve` and `Reject` — dispatch a typed action to the owning panel.
/// The panel sends the decision on the correct session and synchronously
/// marks the card resolved, which also makes rapid duplicate clicks no-ops.
fn render_approval_card(
    appearance: &Appearance,
    approval_id: &str,
    session_id: &str,
    summary: &str,
    mouse_states: ApprovalButtonMouseStates,
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
        .button(ButtonVariant::Accent, mouse_states.approve)
        .with_text_label("Approve".to_owned())
        .build()
        .on_click(move |ctx, _app, _pt| {
            ctx.dispatch_typed_action(AIAssistantAction::OmwApprovalDecision {
                session_id: approve_session.clone(),
                approval_id: approve_id.clone(),
                decision: ApprovalDecision::Approve,
            });
        })
        .finish();

    let reject_id = approval_id.to_string();
    let reject_session = session_id.to_string();
    let reject_btn = appearance
        .ui_builder()
        .button(ButtonVariant::Text, mouse_states.reject)
        .with_text_label("Reject".to_owned())
        .build()
        .on_click(move |ctx, _app, _pt| {
            ctx.dispatch_typed_action(AIAssistantAction::OmwApprovalDecision {
                session_id: reject_session.clone(),
                approval_id: reject_id.clone(),
                decision: ApprovalDecision::Reject,
            });
        })
        .finish();

    let buttons = Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Start)
        .with_child(Container::new(approve_btn).with_margin_bottom(6.).finish())
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
