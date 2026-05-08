//! Pure-data tests for `OmwAgentTranscriptModel::apply_event`.
//!
//! No async, no UI — every test feeds an event sequence and asserts the
//! resulting messages slice.

use super::omw_protocol::OmwAgentEventDown;
use super::omw_transcript::{
    ApprovalCardStatus, OmwAgentMessage, OmwAgentTranscriptModel, ToolCallStatus,
};

fn delta(session: &str, text: &str) -> OmwAgentEventDown {
    OmwAgentEventDown::AssistantDelta {
        session_id: session.into(),
        delta: text.into(),
    }
}

fn turn_finished(session: &str, cancelled: bool) -> OmwAgentEventDown {
    OmwAgentEventDown::TurnFinished {
        session_id: session.into(),
        cancelled,
    }
}

#[test]
fn user_prompt_reserves_streaming_assistant_slot() {
    let mut t = OmwAgentTranscriptModel::new();
    t.push_user("hello".to_string());
    let msgs = t.messages();
    assert_eq!(msgs.len(), 2);
    assert!(matches!(&msgs[0], OmwAgentMessage::User { text } if text == "hello"));
    assert!(matches!(
        &msgs[1],
        OmwAgentMessage::Assistant { text, finished: false } if text.is_empty()
    ));
}

#[test]
fn deltas_concatenate_into_streaming_assistant() {
    let mut t = OmwAgentTranscriptModel::new();
    t.push_user("hi".to_string());
    t.apply_event(&delta("s1", "Hello"));
    t.apply_event(&delta("s1", " "));
    t.apply_event(&delta("s1", "world"));
    let msgs = t.messages();
    assert_eq!(msgs.len(), 2);
    if let OmwAgentMessage::Assistant { text, finished } = &msgs[1] {
        assert_eq!(text, "Hello world");
        assert!(!finished);
    } else {
        panic!("expected Assistant message at index 1");
    }
}

#[test]
fn turn_finished_marks_assistant_as_finished() {
    let mut t = OmwAgentTranscriptModel::new();
    t.push_user("hi".to_string());
    t.apply_event(&delta("s1", "Hello"));
    t.apply_event(&turn_finished("s1", false));
    let msgs = t.messages();
    if let OmwAgentMessage::Assistant { finished, .. } = &msgs[1] {
        assert!(finished, "Assistant should be finished after turn/finished");
    } else {
        panic!("expected Assistant message at index 1");
    }
}

#[test]
fn deltas_after_turn_finished_start_a_new_assistant_message() {
    // Defensive: shouldn't normally happen, but the kernel could emit
    // a stray delta after we've already finalized. The model starts a
    // fresh streaming row so the late delta isn't lost.
    let mut t = OmwAgentTranscriptModel::new();
    t.push_user("hi".to_string());
    t.apply_event(&delta("s1", "first"));
    t.apply_event(&turn_finished("s1", false));
    t.apply_event(&delta("s1", "stray"));
    let msgs = t.messages();
    assert_eq!(msgs.len(), 3);
    if let OmwAgentMessage::Assistant { text, finished } = &msgs[2] {
        assert_eq!(text, "stray");
        assert!(!finished);
    } else {
        panic!("expected fresh Assistant message at index 2");
    }
}

#[test]
fn tool_call_start_then_finish_updates_status() {
    let mut t = OmwAgentTranscriptModel::new();
    t.apply_event(&OmwAgentEventDown::ToolCallStarted {
        session_id: "s1".into(),
        tool_call_id: "tc1".into(),
        tool_name: "bash".into(),
        args: serde_json::json!({"command": "ls"}),
    });
    t.apply_event(&OmwAgentEventDown::ToolCallFinished {
        session_id: "s1".into(),
        tool_call_id: "tc1".into(),
        tool_name: "bash".into(),
        is_error: false,
    });
    let card = t.messages().iter().find_map(|m| match m {
        OmwAgentMessage::ToolCall { id, status, .. } if id == "tc1" => Some(status),
        _ => None,
    });
    assert_eq!(card, Some(&ToolCallStatus::Done));
}

#[test]
fn tool_call_finish_with_is_error_sets_failed_status() {
    let mut t = OmwAgentTranscriptModel::new();
    t.apply_event(&OmwAgentEventDown::ToolCallStarted {
        session_id: "s1".into(),
        tool_call_id: "tc1".into(),
        tool_name: "bash".into(),
        args: serde_json::json!({}),
    });
    t.apply_event(&OmwAgentEventDown::ToolCallFinished {
        session_id: "s1".into(),
        tool_call_id: "tc1".into(),
        tool_name: "bash".into(),
        is_error: true,
    });
    let status = t.messages().iter().find_map(|m| match m {
        OmwAgentMessage::ToolCall { status, .. } => Some(status),
        _ => None,
    });
    assert_eq!(status, Some(&ToolCallStatus::Failed));
}

#[test]
fn approval_request_appends_pending_card() {
    let mut t = OmwAgentTranscriptModel::new();
    t.apply_event(&OmwAgentEventDown::ApprovalRequest {
        session_id: "s1".into(),
        approval_id: "a1".into(),
        tool_call: serde_json::json!({"name": "bash"}),
    });
    let card = t.messages().iter().find_map(|m| match m {
        OmwAgentMessage::Approval { id, decision, .. } if id == "a1" => Some(decision),
        _ => None,
    });
    assert_eq!(card, Some(&ApprovalCardStatus::Pending));
}

#[test]
fn update_approval_flips_decision() {
    let mut t = OmwAgentTranscriptModel::new();
    t.apply_event(&OmwAgentEventDown::ApprovalRequest {
        session_id: "s1".into(),
        approval_id: "a1".into(),
        tool_call: serde_json::json!({"name": "bash"}),
    });
    t.update_approval("a1", ApprovalCardStatus::Approved);
    let decision = t.messages().iter().find_map(|m| match m {
        OmwAgentMessage::Approval { id, decision, .. } if id == "a1" => Some(decision),
        _ => None,
    });
    assert_eq!(decision, Some(&ApprovalCardStatus::Approved));
}

#[test]
fn agent_crashed_appends_error_message() {
    let mut t = OmwAgentTranscriptModel::new();
    t.apply_event(&OmwAgentEventDown::AgentCrashed);
    let last = t.messages().last().unwrap();
    assert!(matches!(last, OmwAgentMessage::Error { .. }));
}

#[test]
fn error_event_appends_message() {
    let mut t = OmwAgentTranscriptModel::new();
    t.apply_event(&OmwAgentEventDown::Error {
        session_id: Some("s1".into()),
        message: "provider 401".into(),
    });
    let last = t.messages().last().unwrap();
    assert!(
        matches!(last, OmwAgentMessage::Error { message } if message.contains("401"))
    );
}

#[test]
fn bash_broker_events_do_not_pollute_transcript() {
    // Phase 5 bash variants flow through the broker, not the transcript.
    let mut t = OmwAgentTranscriptModel::new();
    t.apply_event(&OmwAgentEventDown::ExecCommand {
        session_id: "s1".into(),
        command_id: "c1".into(),
        command: "ls".into(),
        cwd: None,
    });
    t.apply_event(&OmwAgentEventDown::CommandData {
        session_id: "s1".into(),
        command_id: "c1".into(),
        data: "aGVsbG8=".into(),
    });
    t.apply_event(&OmwAgentEventDown::CommandExit {
        session_id: "s1".into(),
        command_id: "c1".into(),
        exit_code: Some(0),
        snapshot: false,
    });
    assert_eq!(t.messages().len(), 0, "bash broker events must not append rows");
}
