//! L3a — approval card model-state interaction tests.
//!
//! Exercises OmwAgentTranscriptModel approval state transitions without
//! an App context or omw-server. Run serially:
//!   cargo test -p warp --features "omw_local test-exports" \
//!     --test omw_agent_approval_test -- --test-threads=1

#![cfg(feature = "omw_local")]

use warp::test_exports::{
    ApprovalCardStatus, OmwAgentEventDown, OmwAgentMessage, OmwAgentTranscriptModel,
};

#[test]
fn approval_request_renders_card_pending() {
    let mut model = OmwAgentTranscriptModel::new();
    model.apply_event(&OmwAgentEventDown::ApprovalRequest {
        session_id: "s1".into(),
        approval_id: "a1".into(),
        tool_call: serde_json::json!({ "name": "bash", "params": { "command": "rm /tmp/x" } }),
    });
    assert!(model.has_pending_approval("a1"));
}

#[test]
fn update_approval_flips_card_status_to_approved() {
    let mut model = OmwAgentTranscriptModel::new();
    model.apply_event(&OmwAgentEventDown::ApprovalRequest {
        session_id: "s1".into(),
        approval_id: "a1".into(),
        tool_call: serde_json::json!({}),
    });
    model.update_approval("a1", ApprovalCardStatus::Approved);
    assert!(!model.has_pending_approval("a1"));

    let approved = model.messages().iter().any(|m| matches!(m,
        OmwAgentMessage::Approval { id, decision, .. }
            if id == "a1" && matches!(decision, ApprovalCardStatus::Approved)));
    assert!(approved);
}

#[test]
fn update_approval_flips_card_status_to_rejected() {
    let mut model = OmwAgentTranscriptModel::new();
    model.apply_event(&OmwAgentEventDown::ApprovalRequest {
        session_id: "s1".into(),
        approval_id: "a1".into(),
        tool_call: serde_json::json!({}),
    });
    model.update_approval("a1", ApprovalCardStatus::Rejected);

    let rejected = model.messages().iter().any(|m| matches!(m,
        OmwAgentMessage::Approval { id, decision, .. }
            if id == "a1" && matches!(decision, ApprovalCardStatus::Rejected)));
    assert!(rejected);
}

#[test]
#[ignore = "requires stub omw-server"]
fn clicking_approve_sends_approval_decide_approve() {
    // Covered indirectly by L3b agent_session.rs in omw-server tests.
}
