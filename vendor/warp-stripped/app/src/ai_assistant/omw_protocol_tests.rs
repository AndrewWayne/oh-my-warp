//! Round-trip tests for the agent WS protocol enums.
//!
//! These tests pin the JSON wire shape so a future serde refactor can't
//! silently change what omw-server sends or what the GUI accepts.

use super::omw_protocol::{OmwAgentEventDown, OmwAgentEventUp};

fn assert_round_trip<T>(value: &T, expected: serde_json::Value)
where
    T: serde::Serialize + for<'de> serde::Deserialize<'de> + std::fmt::Debug + PartialEq,
{
    let serialized = serde_json::to_value(value).expect("serialize");
    assert_eq!(serialized, expected, "wire shape drifted");
    let parsed: T = serde_json::from_value(expected).expect("deserialize");
    assert_eq!(&parsed, value, "round-trip mismatch");
}

#[test]
fn down_assistant_delta_round_trips() {
    let evt = OmwAgentEventDown::AssistantDelta {
        session_id: "s1".into(),
        delta: "hi".into(),
    };
    assert_round_trip(
        &evt,
        serde_json::json!({
            "method": "assistant/delta",
            "params": { "sessionId": "s1", "delta": "hi" }
        }),
    );
}

#[test]
fn down_turn_finished_round_trips() {
    let evt = OmwAgentEventDown::TurnFinished {
        session_id: "s1".into(),
        cancelled: false,
    };
    assert_round_trip(
        &evt,
        serde_json::json!({
            "method": "turn/finished",
            "params": { "sessionId": "s1", "cancelled": false }
        }),
    );
}

#[test]
fn down_tool_call_lifecycle_round_trips() {
    let started = OmwAgentEventDown::ToolCallStarted {
        session_id: "s1".into(),
        tool_call_id: "tc1".into(),
        tool_name: "bash".into(),
        args: serde_json::json!({"command": "ls"}),
    };
    assert_round_trip(
        &started,
        serde_json::json!({
            "method": "tool/call_started",
            "params": {
                "sessionId": "s1",
                "toolCallId": "tc1",
                "toolName": "bash",
                "args": { "command": "ls" }
            }
        }),
    );

    let finished = OmwAgentEventDown::ToolCallFinished {
        session_id: "s1".into(),
        tool_call_id: "tc1".into(),
        tool_name: "bash".into(),
        is_error: false,
    };
    assert_round_trip(
        &finished,
        serde_json::json!({
            "method": "tool/call_finished",
            "params": {
                "sessionId": "s1",
                "toolCallId": "tc1",
                "toolName": "bash",
                "isError": false
            }
        }),
    );
}

#[test]
fn down_approval_request_round_trips() {
    let evt = OmwAgentEventDown::ApprovalRequest {
        session_id: "s1".into(),
        approval_id: "a1".into(),
        tool_call: serde_json::json!({"name": "bash", "id": "tc1"}),
    };
    assert_round_trip(
        &evt,
        serde_json::json!({
            "method": "approval/request",
            "params": {
                "sessionId": "s1",
                "approvalId": "a1",
                "toolCall": { "name": "bash", "id": "tc1" }
            }
        }),
    );
}

#[test]
fn down_agent_crashed_round_trips() {
    let evt = OmwAgentEventDown::AgentCrashed;
    let serialized = serde_json::to_value(&evt).unwrap();
    assert_eq!(serialized["method"], "agent/crashed");
    let parsed: OmwAgentEventDown = serde_json::from_value(serialized).unwrap();
    assert_eq!(parsed, OmwAgentEventDown::AgentCrashed);
}

#[test]
fn down_error_round_trips_with_optional_session_id() {
    let scoped = OmwAgentEventDown::Error {
        session_id: Some("s1".into()),
        message: "boom".into(),
    };
    assert_round_trip(
        &scoped,
        serde_json::json!({
            "method": "error",
            "params": { "sessionId": "s1", "message": "boom" }
        }),
    );

    let unscoped = OmwAgentEventDown::Error {
        session_id: None,
        message: "boom".into(),
    };
    let serialized = serde_json::to_value(&unscoped).unwrap();
    // None serializes as null; deserialization back is what matters.
    let parsed: OmwAgentEventDown = serde_json::from_value(serialized).unwrap();
    assert_eq!(parsed, unscoped);
}

#[test]
fn down_extra_fields_are_ignored() {
    // omw-server forwards the original JSON-RPC notification verbatim,
    // including the `jsonrpc: "2.0"` envelope. Our enum must tolerate that.
    let raw = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "assistant/delta",
        "params": { "sessionId": "s1", "delta": "hi" }
    });
    let parsed: OmwAgentEventDown = serde_json::from_value(raw).unwrap();
    assert_eq!(
        parsed,
        OmwAgentEventDown::AssistantDelta {
            session_id: "s1".into(),
            delta: "hi".into(),
        }
    );
}

#[test]
fn down_bash_broker_variants_round_trip() {
    let exec = OmwAgentEventDown::ExecCommand {
        session_id: "s1".into(),
        command_id: "c1".into(),
        command: "ls".into(),
        cwd: Some("/tmp".into()),
    };
    let serialized = serde_json::to_value(&exec).unwrap();
    assert_eq!(serialized["method"], "bash/exec");
    let parsed: OmwAgentEventDown = serde_json::from_value(serialized).unwrap();
    assert_eq!(parsed, exec);

    let exit = OmwAgentEventDown::CommandExit {
        session_id: "s1".into(),
        command_id: "c1".into(),
        exit_code: Some(0),
        snapshot: false,
    };
    let serialized = serde_json::to_value(&exit).unwrap();
    assert_eq!(serialized["method"], "bash/finished");
    let parsed: OmwAgentEventDown = serde_json::from_value(serialized).unwrap();
    assert_eq!(parsed, exit);
}

#[test]
fn up_prompt_and_cancel_round_trip() {
    let prompt = OmwAgentEventUp::Prompt {
        prompt: "say hi".into(),
    };
    assert_round_trip(
        &prompt,
        serde_json::json!({ "kind": "prompt", "prompt": "say hi" }),
    );

    let cancel = OmwAgentEventUp::Cancel;
    let serialized = serde_json::to_value(&cancel).unwrap();
    assert_eq!(serialized, serde_json::json!({ "kind": "cancel" }));
}

#[test]
fn up_approval_decision_round_trips() {
    let evt = OmwAgentEventUp::ApprovalDecision {
        approval_id: "a1".into(),
        decision: "approve".into(),
    };
    assert_round_trip(
        &evt,
        serde_json::json!({
            "kind": "approval_decision",
            "approvalId": "a1",
            "decision": "approve"
        }),
    );
}
