//! Integration tests for the `# `-prefix inline-pump path
//! ([`OmwAgentState::send_prompt_inline`]).
//!
//! Drives the function against a synthetic `ActiveTerminalHandle` (capturing
//! pty_reads_tx) and a fake outbound mpsc (capturing the prompt that would
//! go to the kernel). Then injects synthetic kernel events back onto the
//! broadcast bus via `test_inject_event` and asserts the expected ANSI
//! framing reaches the pane.
//!
//! These tests share the `OmwAgentState::shared()` singleton, so run them
//! serially:
//!
//!     cargo test -p warp --features "omw_local test-exports" \
//!         --test omw_agent_inline_pump_test -- --test-threads=1

#![cfg(all(feature = "omw_local", feature = "test-exports"))]

use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex as PlMutex;
use tokio::sync::mpsc;

use warp::test_exports::terminal_io::{mio_channel, Message};
use warp::test_exports::{
    ActiveTerminalHandle, OmwAgentEventDown, OmwAgentEventUp, OmwAgentState,
};

const RECV_TIMEOUT: Duration = Duration::from_millis(2000);

fn make_pane() -> (
    ActiveTerminalHandle,
    mio_channel::Receiver<Message>,
) {
    let (tx, event_loop_rx) = mio_channel::channel::<Message>();
    let event_loop_tx = Arc::new(PlMutex::new(tx));
    // pty_reads_tx is still part of the handle (used by the bash
    // broker for OSC133 detection), but the inline-pump no longer
    // writes to it — synthetic agent text now goes through
    // `Message::InjectBytes` on event_loop_tx so the renderer's actual
    // ANSI parsing pipeline picks it up. We keep a held receiver on
    // pty_reads_tx so the channel stays open against
    // `async_broadcast::Sender::try_broadcast` ergonomics, but tests
    // assert against the event_loop_rx instead.
    let (pty_reads_tx, _pty_reads_rx) = async_broadcast::broadcast::<Arc<Vec<u8>>>(64);
    let handle = ActiveTerminalHandle {
        view_id: warpui::EntityId::new(),
        event_loop_tx,
        pty_reads_tx,
    };
    (handle, event_loop_rx)
}

/// Drain `Message::InjectBytes` frames from the event_loop receiver
/// until a chunk containing `needle` is seen, or the deadline expires.
/// Returns the concatenated bytes for diagnostics on failure. Other
/// `Message` variants (Input / Resize / Shutdown) are skipped — the
/// inline-pump only emits InjectBytes.
fn await_bytes_containing(
    rx: &mio_channel::Receiver<Message>,
    needle: &[u8],
    timeout: Duration,
) -> Result<Vec<u8>, Vec<u8>> {
    let deadline = Instant::now() + timeout;
    let mut acc: Vec<u8> = Vec::new();
    while Instant::now() < deadline {
        match rx.try_recv() {
            Ok(Message::InjectBytes(bytes)) => {
                acc.extend_from_slice(&bytes);
                if windowed_contains(&acc, needle) {
                    return Ok(acc);
                }
            }
            Ok(_) => continue,
            Err(_) => std::thread::sleep(Duration::from_millis(10)),
        }
    }
    Err(acc)
}

fn windowed_contains(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || haystack.len() < needle.len() {
        return false;
    }
    haystack.windows(needle.len()).any(|w| w == needle)
}

struct InlinePumpFixture {
    state: Arc<OmwAgentState>,
    outbound_rx: mpsc::Receiver<OmwAgentEventUp>,
}

impl InlinePumpFixture {
    fn new() -> Self {
        let state = OmwAgentState::shared();
        // Drop any prior session so this test sees a clean outbound + no
        // residual broker. Importantly, this also drops the prior
        // event_tx subscribers list — but the singleton's event_tx itself
        // is constructed once and survives test_reset (by design — see
        // omw_agent_state.rs:113-115).
        state.test_reset();
        let _runtime = state
            .test_ensure_runtime()
            .expect("test_ensure_runtime failed");
        let (out_tx, out_rx) = mpsc::channel::<OmwAgentEventUp>(64);
        state.test_install_outbound(out_tx);
        Self {
            state,
            outbound_rx: out_rx,
        }
    }

    fn recv_outbound(&mut self, timeout: Duration) -> Option<OmwAgentEventUp> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            match self.outbound_rx.try_recv() {
                Ok(up) => return Some(up),
                Err(_) => std::thread::sleep(Duration::from_millis(10)),
            }
        }
        None
    }
}

#[test]
fn send_prompt_inline_forwards_prompt_to_outbound() {
    let mut fx = InlinePumpFixture::new();
    let (handle, _rx) = make_pane();

    fx.state
        .send_prompt_inline("hello?".to_string(), handle)
        .expect("send_prompt_inline should succeed when outbound is wired");

    let received = fx
        .recv_outbound(RECV_TIMEOUT)
        .expect("expected an OmwAgentEventUp::Prompt on the outbound mpsc");
    match received {
        OmwAgentEventUp::Prompt { prompt } => assert_eq!(prompt, "hello?"),
        other => panic!("expected Prompt, got {other:?}"),
    }
}

#[test]
fn send_prompt_inline_echoes_user_text_to_pty() {
    let mut fx = InlinePumpFixture::new();
    let (handle, mut rx) = make_pane();

    fx.state
        .send_prompt_inline("what is 2+2".to_string(), handle)
        .expect("send_prompt_inline OK");
    // Drain the outbound so the test isolation is clean for the next case.
    let _ = fx.recv_outbound(RECV_TIMEOUT);

    let needle = b"# what is 2+2";
    let collected = await_bytes_containing(&mut rx, needle, RECV_TIMEOUT)
        .unwrap_or_else(|got| panic!("echo not seen; got {} bytes: {:?}", got.len(), String::from_utf8_lossy(&got)));
    assert!(windowed_contains(&collected, needle));
}

#[test]
fn pump_renders_assistant_delta_into_pty() {
    let mut fx = InlinePumpFixture::new();
    let (handle, mut rx) = make_pane();

    fx.state
        .send_prompt_inline("anything".to_string(), handle)
        .expect("send_prompt_inline OK");
    let _ = fx.recv_outbound(RECV_TIMEOUT);

    // Drain echo bytes so we don't false-match.
    let _ = await_bytes_containing(&mut rx, b"# anything", RECV_TIMEOUT);

    fx.state.test_inject_event(OmwAgentEventDown::AssistantDelta {
        session_id: "sess-test".into(),
        delta: "the answer is 4".into(),
    });

    let needle = b"the answer is 4";
    let collected = await_bytes_containing(&mut rx, needle, RECV_TIMEOUT)
        .unwrap_or_else(|got| panic!("delta not pumped; got {} bytes: {:?}", got.len(), String::from_utf8_lossy(&got)));
    assert!(windowed_contains(&collected, needle));
}

#[test]
fn pump_renders_tool_call_framing() {
    let mut fx = InlinePumpFixture::new();
    let (handle, mut rx) = make_pane();

    fx.state
        .send_prompt_inline("run a command".to_string(), handle)
        .expect("send_prompt_inline OK");
    let _ = fx.recv_outbound(RECV_TIMEOUT);
    let _ = await_bytes_containing(&mut rx, b"# run a command", RECV_TIMEOUT);

    fx.state.test_inject_event(OmwAgentEventDown::ToolCallStarted {
        session_id: "sess-test".into(),
        tool_call_id: "tc-1".into(),
        tool_name: "bash".into(),
        args: serde_json::json!({ "command": "echo hi" }),
    });

    let needle = b"tool: bash";
    let collected = await_bytes_containing(&mut rx, needle, RECV_TIMEOUT)
        .unwrap_or_else(|got| panic!("tool framing not pumped; got: {:?}", String::from_utf8_lossy(&got)));
    assert!(windowed_contains(&collected, needle));
    assert!(windowed_contains(&collected, b"`echo hi`"));
}

#[test]
fn pump_renders_error_event_in_red() {
    let mut fx = InlinePumpFixture::new();
    let (handle, mut rx) = make_pane();

    fx.state
        .send_prompt_inline("trigger error".to_string(), handle)
        .expect("send_prompt_inline OK");
    let _ = fx.recv_outbound(RECV_TIMEOUT);
    let _ = await_bytes_containing(&mut rx, b"# trigger error", RECV_TIMEOUT);

    fx.state.test_inject_event(OmwAgentEventDown::Error {
        session_id: Some("sess-test".into()),
        message: "kernel exploded".into(),
    });

    let needle = b"omw agent error: kernel exploded";
    let collected = await_bytes_containing(&mut rx, needle, RECV_TIMEOUT)
        .unwrap_or_else(|got| panic!("error not pumped; got: {:?}", String::from_utf8_lossy(&got)));
    assert!(windowed_contains(&collected, needle));
    // Should also include the red ANSI prefix.
    assert!(windowed_contains(&collected, b"\x1b[31m"));
}

#[test]
fn pump_terminates_on_turn_finished() {
    let mut fx = InlinePumpFixture::new();
    let (handle, mut rx) = make_pane();

    fx.state
        .send_prompt_inline("end-of-turn check".to_string(), handle)
        .expect("send_prompt_inline OK");
    let _ = fx.recv_outbound(RECV_TIMEOUT);
    let _ = await_bytes_containing(&mut rx, b"# end-of-turn check", RECV_TIMEOUT);

    fx.state.test_inject_event(OmwAgentEventDown::TurnFinished {
        session_id: "sess-test".into(),
        cancelled: false,
    });

    let needle = b"[turn finished]";
    let collected = await_bytes_containing(&mut rx, needle, RECV_TIMEOUT)
        .unwrap_or_else(|got| panic!("turn-finished marker not pumped; got: {:?}", String::from_utf8_lossy(&got)));
    assert!(windowed_contains(&collected, needle));
}

#[test]
fn send_prompt_inline_fails_with_clear_error_when_outbound_missing() {
    let state = OmwAgentState::shared();
    state.test_reset();
    // Don't install_outbound — should produce a deterministic error
    // matching the silence path users would otherwise hit.
    let _ = state.test_ensure_runtime().expect("runtime");

    let (handle, _rx) = make_pane();

    let err = state
        .send_prompt_inline("nope".to_string(), handle)
        .expect_err("expected error when outbound is None");
    assert!(
        err.contains("no active agent session"),
        "unexpected error: {err}"
    );
}
