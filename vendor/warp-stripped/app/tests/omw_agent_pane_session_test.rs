//! Per-pane session isolation tests.
//!
//! Verifies that each pane carries its own [`PaneSession`] keyed by
//! `view_id`, so prompts in different panes don't share a kernel
//! session (and therefore don't share conversation context). Uses
//! `test_install_pane_session` to inject synthetic sessions without
//! spinning up a real kernel + WS — the synthetic outbound mpsc
//! captures the events `send_prompt_inline_for_pane` would forward to
//! the WS, and the returned `event_tx` lets the test simulate inbound
//! frames per-pane.
//!
//! Run with:
//!     cargo test -p warp --features "omw_local test-exports" \
//!         --test omw_agent_pane_session_test -- --test-threads=1

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
    let (pty_reads_tx, _pty_rx) = async_broadcast::broadcast::<Arc<Vec<u8>>>(64);
    let handle = ActiveTerminalHandle {
        view_id: warpui::EntityId::new(),
        event_loop_tx,
        pty_reads_tx,
    };
    (handle, event_loop_rx)
}

fn await_recv<T: Send + 'static>(
    rx: &mut mpsc::Receiver<T>,
    timeout: Duration,
) -> Option<T> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        match rx.try_recv() {
            Ok(item) => return Some(item),
            Err(_) => std::thread::sleep(Duration::from_millis(10)),
        }
    }
    None
}

#[test]
fn two_panes_get_distinct_session_ids() {
    let state = OmwAgentState::shared();
    state.test_reset();
    let _ = state.test_ensure_runtime().expect("runtime");

    let (handle_a, _) = make_pane();
    let (handle_b, _) = make_pane();
    assert_ne!(
        handle_a.view_id, handle_b.view_id,
        "different panes must have different EntityIds"
    );

    let (out_a_tx, _out_a_rx) = mpsc::channel::<OmwAgentEventUp>(32);
    let (out_b_tx, _out_b_rx) = mpsc::channel::<OmwAgentEventUp>(32);

    state.test_install_pane_session(handle_a.view_id, out_a_tx);
    state.test_install_pane_session(handle_b.view_id, out_b_tx);

    let session_a = state
        .pane_session(handle_a.view_id)
        .expect("pane A session installed");
    let session_b = state
        .pane_session(handle_b.view_id)
        .expect("pane B session installed");

    assert_ne!(
        session_a.session_id, session_b.session_id,
        "each pane's PaneSession carries its own session_id"
    );
}

#[test]
fn prompt_in_pane_a_only_routes_to_pane_a_outbound() {
    let state = OmwAgentState::shared();
    state.test_reset();
    let _ = state.test_ensure_runtime().expect("runtime");

    let (handle_a, _rx_a) = make_pane();
    let (handle_b, _rx_b) = make_pane();

    let (out_a_tx, mut out_a_rx) = mpsc::channel::<OmwAgentEventUp>(32);
    let (out_b_tx, mut out_b_rx) = mpsc::channel::<OmwAgentEventUp>(32);

    state.test_install_pane_session(handle_a.view_id, out_a_tx);
    state.test_install_pane_session(handle_b.view_id, out_b_tx);

    state
        .send_prompt_inline_for_pane("hello from pane A".to_string(), handle_a.clone())
        .expect("send_prompt_inline_for_pane on A should succeed");

    // Pane A should receive the Prompt frame.
    let recv_a = await_recv(&mut out_a_rx, RECV_TIMEOUT)
        .expect("pane A should receive its own prompt");
    match recv_a {
        OmwAgentEventUp::Prompt { prompt } => assert_eq!(prompt, "hello from pane A"),
        other => panic!("expected Prompt on A, got {other:?}"),
    }

    // Pane B's outbound MUST be silent — different session, different
    // context. This is the entire reason for the per-pane refactor.
    let recv_b = await_recv(&mut out_b_rx, Duration::from_millis(100));
    assert!(
        recv_b.is_none(),
        "pane B must NOT receive pane A's prompt; got {recv_b:?}"
    );
}

#[test]
fn pane_pump_only_renders_events_from_its_own_session() {
    let state = OmwAgentState::shared();
    state.test_reset();
    let _ = state.test_ensure_runtime().expect("runtime");

    let (handle_a, mut event_loop_rx_a) = make_pane();
    let (handle_b, mut event_loop_rx_b) = make_pane();

    let (out_a_tx, mut _out_a_rx) = mpsc::channel::<OmwAgentEventUp>(32);
    let (out_b_tx, mut _out_b_rx) = mpsc::channel::<OmwAgentEventUp>(32);

    let (event_tx_a, _) = state.test_install_pane_session(handle_a.view_id, out_a_tx);
    let (event_tx_b, _) = state.test_install_pane_session(handle_b.view_id, out_b_tx);

    // Kick off the pump for pane A only.
    state
        .send_prompt_inline_for_pane("ping".to_string(), handle_a.clone())
        .expect("send_prompt_inline_for_pane A");
    // Consume the echo line so it doesn't false-match the assertion below.
    drain_inject_bytes_until(&mut event_loop_rx_a, b"# ping", RECV_TIMEOUT);

    // Inject an AssistantDelta on pane B's bus. Pane A's pump must
    // NOT pick it up — that's the whole point of per-pane.
    let _ = event_tx_b.send(OmwAgentEventDown::AssistantDelta {
        session_id: "test-B".into(),
        delta: "from-pane-B".into(),
    });
    let leaked = drain_inject_bytes_until(
        &mut event_loop_rx_a,
        b"from-pane-B",
        Duration::from_millis(150),
    );
    assert!(
        !leaked,
        "pane A must not render events from pane B's session"
    );

    // Now inject on pane A's bus — pane A's pump must render it.
    let _ = event_tx_a.send(OmwAgentEventDown::AssistantDelta {
        session_id: "test-A".into(),
        delta: "from-pane-A".into(),
    });
    let saw = drain_inject_bytes_until(&mut event_loop_rx_a, b"from-pane-A", RECV_TIMEOUT);
    assert!(saw, "pane A's pump must render events from pane A's session");

    // And pane B must not see pane A's reply (B never got a prompt).
    let _leaked = drain_inject_bytes_until(
        &mut event_loop_rx_b,
        b"from-pane-A",
        Duration::from_millis(50),
    );
    // (event_loop_rx_b not actively pumped, so no leak expected — this
    // call mostly drains any spurious bytes for cleanup.)
}

/// Drain `mio_channel` Message frames, looking for any
/// `Message::InjectBytes` whose contents contain `needle`. Returns
/// `true` on hit, `false` on timeout.
fn drain_inject_bytes_until(
    rx: &mut mio_channel::Receiver<Message>,
    needle: &[u8],
    timeout: Duration,
) -> bool {
    let deadline = Instant::now() + timeout;
    let mut acc: Vec<u8> = Vec::new();
    while Instant::now() < deadline {
        match rx.try_recv() {
            Ok(Message::InjectBytes(bytes)) => {
                acc.extend_from_slice(&bytes);
                if windowed_contains(&acc, needle) {
                    return true;
                }
            }
            Ok(_) => continue,
            Err(_) => std::thread::sleep(Duration::from_millis(10)),
        }
    }
    false
}

fn windowed_contains(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || haystack.len() < needle.len() {
        return false;
    }
    haystack.windows(needle.len()).any(|w| w == needle)
}
