//! L3a — Phase 5b GUI command broker tests.
//!
//! These tests drive `spawn_command_broker` against a real tokio runtime
//! with synthetic `event_loop_tx` / `pty_reads_tx` channels — no `App` /
//! gpui context is needed because the broker runs entirely on the tokio
//! side. The tests share the `OmwAgentState::shared()` singleton, so
//! run them serially:
//!
//!     cargo test -p warp --features "omw_local test-exports" \
//!         --test omw_agent_command_broker_test -- --test-threads=1
//!
//! The `OSC 133` detection algorithm has its own unit tests inline in
//! `omw_command_broker.rs` (4 cases). These five integration tests cover
//! the loop:
//!
//!   1. `register_active_terminal` stores a handle that
//!      `active_terminal_clone` returns.
//!   2. `ExecCommand` injected on the event bus emits `Message::Input`
//!      (command bytes + CR) on the captured pane's `event_loop_tx`.
//!   3. PTY chunks broadcast on `pty_reads_tx` are emitted upstream as
//!      `OmwAgentEventUp::CommandData` with base64-encoded payload.
//!   4. An OSC 133 prompt-end marker in the PTY stream resolves the
//!      command with `OmwAgentEventUp::CommandExit { exit_code, snapshot:
//!      false }`.
//!   5. Absent any prompt-end marker, the broker times out (we use a
//!      doubled timeout for the test by sending a synthetic byte) and
//!      emits `CommandExit { snapshot: true }`.

#![cfg(all(feature = "omw_local", feature = "test-exports"))]

use std::sync::Arc;
use std::time::Duration;

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use parking_lot::Mutex as PlMutex;
use tokio::sync::mpsc;
use tokio::time::timeout;

use warp::test_exports::terminal_io::{mio_channel, Message};
use warp::test_exports::{
    spawn_command_broker, ActiveTerminalHandle, OmwAgentEventDown, OmwAgentEventUp, OmwAgentState,
};

const RECV_TIMEOUT: Duration = Duration::from_secs(2);

/// Test fixture that wraps an `OmwAgentState`, a fake outbound mpsc
/// (drop-in for the WS task's inbound channel), and a tokio runtime
/// dedicated to driving `outbound_rx.recv()` from the test thread.
struct Fixture {
    outbound_rx: mpsc::Receiver<OmwAgentEventUp>,
    runtime: tokio::runtime::Runtime,
    _broker_task: tokio::task::JoinHandle<()>,
}

impl Fixture {
    fn new() -> Self {
        let state = OmwAgentState::shared();
        // Reset shared state so prior tests in this file don't cross-talk.
        state.test_reset();
        let agent_runtime = state
            .test_ensure_runtime()
            .expect("test_ensure_runtime failed");
        let (out_tx, out_rx) = mpsc::channel::<OmwAgentEventUp>(64);
        state.test_install_outbound(out_tx);
        let broker_task = spawn_command_broker(state.clone(), &agent_runtime);
        // A dedicated current-thread runtime so the test can `block_on` the
        // outbound mpsc without colliding with the agent runtime that owns
        // the broker task.
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build test runtime");
        Self {
            outbound_rx: out_rx,
            runtime,
            _broker_task: broker_task,
        }
    }
}

/// Synthetic local-PTY channel pair the broker can drive. Holds the
/// underlying receiver so the channel stays open across the test (without
/// it, async_broadcast surfaces a `Closed` error to fresh `new_receiver`
/// callers, which the broker then treats as a snapshot exit).
struct FakePane {
    handle: ActiveTerminalHandle,
    event_loop_rx: mio_channel::Receiver<Message>,
    pty_reads_tx: async_broadcast::Sender<Arc<Vec<u8>>>,
    _baseline_pty_rx: async_broadcast::Receiver<Arc<Vec<u8>>>,
}

fn make_handle() -> FakePane {
    let (tx, event_loop_rx) = mio_channel::channel::<Message>();
    let event_loop_tx = Arc::new(PlMutex::new(tx));
    let (pty_reads_tx, baseline_rx) = async_broadcast::broadcast::<Arc<Vec<u8>>>(64);
    let handle = ActiveTerminalHandle {
        view_id: warpui::EntityId::new(),
        event_loop_tx,
        pty_reads_tx: pty_reads_tx.clone(),
    };
    FakePane {
        handle,
        event_loop_rx,
        pty_reads_tx,
        _baseline_pty_rx: baseline_rx,
    }
}

/// Drain the event_loop_rx until we see a Message::Input, returning its
/// bytes. Times out after 2 seconds.
fn recv_input(rx: &mio_channel::Receiver<Message>) -> Option<Vec<u8>> {
    let deadline = std::time::Instant::now() + RECV_TIMEOUT;
    loop {
        match rx.try_recv() {
            Ok(Message::Input(bytes)) => return Some(bytes.into_owned()),
            Ok(_) => continue,
            Err(_) => {
                if std::time::Instant::now() >= deadline {
                    return None;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }
}

#[test]
fn register_active_terminal_stores_handle() {
    let _fx = Fixture::new();
    let state = OmwAgentState::shared();
    let pane = make_handle();
    let stored_id = pane.handle.view_id;
    state.register_active_terminal(pane.handle);
    let cloned = state.active_terminal_clone().expect("handle should be set");
    assert_eq!(cloned.view_id, stored_id);
    state.clear_active_terminal();
    assert!(state.active_terminal_clone().is_none());
}

#[test]
fn exec_command_emits_input_message_on_event_loop() {
    let _fx = Fixture::new();
    let state = OmwAgentState::shared();
    let pane = make_handle();
    state.register_active_terminal(pane.handle.clone());

    state.test_inject_event(OmwAgentEventDown::ExecCommand {
        session_id: "sess-1".into(),
        command_id: "cmd-1".into(),
        command: "echo hi".into(),
        cwd: Some("/tmp".into()),
    });

    let input = recv_input(&pane.event_loop_rx).expect("expected Message::Input from broker");
    // The broker sends the command bytes + a CR for shell submission.
    assert_eq!(input, b"echo hi\r");
}

#[test]
fn pty_reads_emit_command_data_upstream() {
    let mut fx = Fixture::new();
    let state = OmwAgentState::shared();
    let pane = make_handle();
    state.register_active_terminal(pane.handle.clone());

    state.test_inject_event(OmwAgentEventDown::ExecCommand {
        session_id: "sess-1".into(),
        command_id: "cmd-1".into(),
        command: "echo hi".into(),
        cwd: None,
    });
    // Wait briefly so the broker spawns its handle_exec task and
    // subscribes to pty_reads before we publish chunks.
    std::thread::sleep(Duration::from_millis(50));

    let chunk = b"hello".to_vec();
    let _ = pane.pty_reads_tx.try_broadcast(Arc::new(chunk.clone()));

    let event = fx
        .runtime
        .block_on(async {
            timeout(RECV_TIMEOUT, fx.outbound_rx.recv())
                .await
                .expect("no event within timeout")
                .expect("outbound channel closed")
        });
    match event {
        OmwAgentEventUp::CommandData {
            command_id, data,
        } => {
            assert_eq!(command_id, "cmd-1");
            assert_eq!(BASE64_STANDARD.decode(&data).unwrap(), chunk);
        }
        other => panic!("expected CommandData, got {other:?}"),
    }
}

#[test]
fn osc133_prompt_end_emits_command_exit_with_exit_code() {
    let mut fx = Fixture::new();
    let state = OmwAgentState::shared();
    let pane = make_handle();
    state.register_active_terminal(pane.handle.clone());

    state.test_inject_event(OmwAgentEventDown::ExecCommand {
        session_id: "sess-1".into(),
        command_id: "cmd-1".into(),
        command: "false".into(),
        cwd: None,
    });
    std::thread::sleep(Duration::from_millis(50));

    // Send the actual end-of-command marker Warp's shell hooks emit on
    // macOS / Linux: a DCS-wrapped hex-encoded JSON `CommandFinished`
    // message (see `assets/bundled/bootstrap/zsh_body.sh`'s
    // `warp_send_json_message` function). The earlier test stub used a
    // synthetic OSC 133 sequence that no real shell ever emitted, so
    // the broker's detector was untested against production traffic.
    let json = "{\"hook\":\"CommandFinished\",\"value\":{\"exit_code\":1}}";
    let mut chunk: Vec<u8> = Vec::new();
    chunk.extend_from_slice(b"\x1b\x50\x24\x64");
    for byte in json.as_bytes() {
        chunk.extend_from_slice(format!("{byte:02x}").as_bytes());
    }
    chunk.push(0x9c);
    let _ = pane.pty_reads_tx.try_broadcast(Arc::new(chunk));

    // First event is CommandData (the broker forwards every chunk before
    // checking for the marker); second is CommandExit.
    let mut saw_exit = false;
    for _ in 0..4 {
        let event = fx
            .runtime
            .block_on(async {
                timeout(RECV_TIMEOUT, fx.outbound_rx.recv()).await
            });
        match event {
            Ok(Some(OmwAgentEventUp::CommandExit {
                command_id,
                exit_code,
                snapshot,
            })) => {
                assert_eq!(command_id, "cmd-1");
                assert_eq!(exit_code, Some(1));
                assert!(!snapshot);
                saw_exit = true;
                break;
            }
            Ok(Some(OmwAgentEventUp::CommandData { .. })) => continue,
            Ok(Some(other)) => panic!("unexpected event: {other:?}"),
            Ok(None) => panic!("outbound channel closed"),
            Err(_) => panic!("timed out waiting for CommandExit"),
        }
    }
    assert!(saw_exit, "expected CommandExit before timeout");
}

#[test]
fn no_active_terminal_emits_immediate_snapshot_exit() {
    let mut fx = Fixture::new();
    let state = OmwAgentState::shared();
    // Do NOT register an active terminal — broker should fast-path to
    // snapshot:true.
    state.clear_active_terminal();

    state.test_inject_event(OmwAgentEventDown::ExecCommand {
        session_id: "sess-1".into(),
        command_id: "cmd-1".into(),
        command: "anything".into(),
        cwd: None,
    });

    let event = fx
        .runtime
        .block_on(async {
            timeout(RECV_TIMEOUT, fx.outbound_rx.recv())
                .await
                .expect("no event within timeout")
                .expect("outbound channel closed")
        });
    match event {
        OmwAgentEventUp::CommandExit {
            command_id,
            exit_code,
            snapshot,
        } => {
            assert_eq!(command_id, "cmd-1");
            assert!(exit_code.is_none());
            assert!(snapshot, "expected snapshot:true with no active pane");
        }
        other => panic!("expected CommandExit, got {other:?}"),
    }
}
