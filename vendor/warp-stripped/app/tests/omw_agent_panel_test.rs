//! L3a — agent panel transcript-model interaction tests.
//!
//! These tests exercise `OmwAgentTranscriptModel::apply_event` directly —
//! no warpui App context is needed. The panel-mount test that requires a
//! running omw-server is #[ignore]d.
//!
//! Run serially to avoid shared-state issues:
//!   cargo test -p warp --features "omw_local test-exports" \
//!     --test omw_agent_panel_test -- --test-threads=1

#![cfg(feature = "omw_local")]

use warp::test_exports::{OmwAgentEventDown, OmwAgentTranscriptModel};

#[test]
fn inbound_assistant_delta_appends_to_transcript() {
    let mut model = OmwAgentTranscriptModel::new();
    let event = OmwAgentEventDown::AssistantDelta {
        session_id: "sess-1".into(),
        delta: "hello".into(),
    };
    model.apply_event(&event);
    assert_eq!(model.last_assistant_text().as_deref(), Some("hello"));
}

#[test]
fn inbound_tool_call_started_renders_tool_card() {
    let mut model = OmwAgentTranscriptModel::new();
    let event = OmwAgentEventDown::ToolCallStarted {
        session_id: "sess-1".into(),
        tool_call_id: "tc-1".into(),
        tool_name: "bash".into(),
        args: serde_json::json!({ "command": "ls" }),
    };
    model.apply_event(&event);
    assert!(model.has_tool_call("tc-1"));
}

#[test]
fn tool_call_finished_flips_card_status() {
    let mut model = OmwAgentTranscriptModel::new();
    model.apply_event(&OmwAgentEventDown::ToolCallStarted {
        session_id: "sess-1".into(),
        tool_call_id: "tc-1".into(),
        tool_name: "bash".into(),
        args: serde_json::json!({ "command": "ls" }),
    });
    model.apply_event(&OmwAgentEventDown::ToolCallFinished {
        session_id: "sess-1".into(),
        tool_call_id: "tc-1".into(),
        tool_name: "bash".into(),
        is_error: false,
    });
    assert!(model.tool_call_finished("tc-1"));
}

#[test]
#[ignore = "requires stub omw-server"]
fn panel_mount_with_valid_config_starts_omw_agent_state() {
    // Covered indirectly by L3b agent_session.rs. Marked pending.
}
