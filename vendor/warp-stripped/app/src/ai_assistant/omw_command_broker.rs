//! Phase 5b — GUI command broker.
//!
//! Subscribes to [`OmwAgentState::subscribe_events`] and consumes
//! [`OmwAgentEventDown::ExecCommand`] frames. For each, captures the
//! currently-focused terminal pane (`ActiveTerminalHandle`), pushes
//! `Message::Input(command + CR)` onto its `event_loop_tx`, taps PTY
//! reads via `pty_reads_tx.new_receiver()`, and emits `command_data` /
//! `command_exit` frames back through [`OmwAgentState::send_command_data`]
//! / [`OmwAgentState::send_command_exit`].
//!
//! Per-call capture: the handle is cloned at `bash/exec` time and held
//! by the per-command task. If the user shifts focus to a different pane
//! mid-execution, the original pane keeps streaming output back to the
//! kernel — focus changes only affect *future* `bash/exec` arrivals.
//!
//! Termination: OSC 133 prompt-end ([`detect_osc133_prompt_end`]) closes
//! the loop with the reported exit code; otherwise a 30-second timeout
//! fires and we report `snapshot: true` so the kernel's tool call resolves
//! cleanly.

#![cfg(feature = "omw_local")]

use std::borrow::Cow;
use std::sync::Arc;
use std::time::Duration;

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use tokio::task::JoinHandle;

use super::omw_agent_state::{ActiveTerminalHandle, OmwAgentState};
use super::omw_protocol::OmwAgentEventDown;
use crate::terminal::writeable_pty::Message;

/// Per-command timeout. After this many seconds with no OSC 133 prompt-end,
/// the broker emits a `snapshot: true` `command_exit` so the kernel's tool
/// call resolves and the agent can keep going.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);

/// Spawn the command broker on the supplied tokio runtime. Returns a
/// `JoinHandle` the caller drops on session teardown (panel unmount).
///
/// Idempotent against multiple calls only insofar as the caller drops the
/// previous handle — calling twice without dropping spawns two independent
/// loops that will both pull from `subscribe_events`, doubling outbound
/// `command_data` frames per command. Don't do that.
pub fn spawn_command_broker(
    state: Arc<OmwAgentState>,
    runtime: &tokio::runtime::Handle,
) -> JoinHandle<()> {
    // Subscribe synchronously here, before the spawn. Otherwise events
    // sent between `runtime.spawn(...)` returning and the spawned task's
    // first `subscribe_events()` call would be silently dropped (the
    // L3a integration tests caught this race).
    // Subscribe synchronously here, before the spawn. Otherwise events
    // sent between `runtime.spawn(...)` returning and the spawned task's
    // first `subscribe_events()` call would be silently dropped (the
    // L3a integration tests caught this race).
    let mut events = state.subscribe_events();
    runtime.spawn(async move {
        while let Ok(event) = events.recv().await {
            if let OmwAgentEventDown::ExecCommand {
                command_id,
                command,
                cwd: _,
                ..
            } = event
            {
                // Snapshot the focused pane at exec time. None → no live
                // pane, fire snapshot:true immediately so the kernel's
                // tool call doesn't hang for the full COMMAND_TIMEOUT.
                let handle = match state.active_terminal_clone() {
                    Some(h) => h,
                    None => {
                        let _ = state.send_command_exit(command_id, None, true);
                        continue;
                    }
                };
                let state_for_task = state.clone();
                tokio::spawn(async move {
                    handle_exec(state_for_task, handle, command_id, command).await;
                });
            }
        }
    })
}

/// Drive a single `bash/exec` to completion against the captured pane.
async fn handle_exec(
    state: Arc<OmwAgentState>,
    handle: ActiveTerminalHandle,
    command_id: String,
    command: String,
) {
    // Subscribe to PTY reads BEFORE writing the command so we don't drop
    // the prompt-end marker for a fast-completing command.
    let mut rx = handle.pty_reads_tx.new_receiver();

    // Inject the command into the pane. A trailing CR is needed for the
    // shell to actually execute (matching pty_controller's `COMMAND_ENTER`).
    let mut bytes = command.into_bytes();
    bytes.push(b'\r');
    let send_result = handle
        .event_loop_tx
        .lock()
        .send(Message::Input(Cow::Owned(bytes)));
    if let Err(e) = send_result {
        log::warn!("omw command_broker: event_loop_tx send failed: {e}");
        let _ = state.send_command_exit(command_id, None, true);
        return;
    }

    let timeout = tokio::time::sleep(COMMAND_TIMEOUT);
    tokio::pin!(timeout);

    loop {
        tokio::select! {
            _ = &mut timeout => {
                let _ = state.send_command_exit(command_id, None, true);
                return;
            }
            chunk = rx.recv() => {
                let Ok(bytes) = chunk else {
                    // Sender dropped (pane closed). Treat as snapshot.
                    let _ = state.send_command_exit(command_id, None, true);
                    return;
                };
                let encoded = BASE64_STANDARD.encode(bytes.as_slice());
                let _ = state.send_command_data(command_id.clone(), encoded);
                if let Some(code) = detect_osc133_prompt_end(&bytes) {
                    let _ = state.send_command_exit(command_id, code, false);
                    return;
                }
            }
        }
    }
}

/// Detect OSC 133 prompt-end (`ESC ] 133 ; D ; <code> BEL`).
///
/// Returns `Some(Some(code))` when a code is present, `Some(None)` when
/// the marker has no exit code, and `None` when no marker is present in
/// the chunk. Warp's bundled shell hooks emit this at command end.
pub fn detect_osc133_prompt_end(bytes: &[u8]) -> Option<Option<i32>> {
    let s = String::from_utf8_lossy(bytes);
    let needle = "\x1b]133;D";
    if let Some(idx) = s.find(needle) {
        let tail = &s[idx + needle.len()..];
        if let Some(end) = tail.find('\x07') {
            let inner = &tail[..end];
            if inner.is_empty() {
                return Some(None);
            }
            if let Some(stripped) = inner.strip_prefix(';') {
                return Some(stripped.parse::<i32>().ok());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_osc133_with_exit_code_zero() {
        let bytes = b"hello\x1b]133;D;0\x07world";
        assert_eq!(detect_osc133_prompt_end(bytes), Some(Some(0)));
    }

    #[test]
    fn detects_osc133_with_exit_code_127() {
        let bytes = b"hello\x1b]133;D;127\x07";
        assert_eq!(detect_osc133_prompt_end(bytes), Some(Some(127)));
    }

    #[test]
    fn detects_osc133_without_exit_code() {
        let bytes = b"\x1b]133;D\x07";
        assert_eq!(detect_osc133_prompt_end(bytes), Some(None));
    }

    #[test]
    fn no_osc133_returns_none() {
        let bytes = b"plain output, no marker";
        assert_eq!(detect_osc133_prompt_end(bytes), None);
    }
}
