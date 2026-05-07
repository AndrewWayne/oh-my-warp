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

use super::omw_agent_state::{ActiveTerminalHandle, OmwAgentState, PaneSession};
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
                session_id,
                command_id,
                command,
                cwd: _,
            } = event
            {
                // Resolve which pane and reply-channel this exec belongs
                // to. Order matters:
                //   1. Try the per-pane PaneSession that owns
                //      `session_id` (the inline `# foo` flow). Replies
                //      MUST go back over THAT pane's WS, not the
                //      singleton outbound, or the kernel session that
                //      issued bash/exec never sees them.
                //   2. Fall back to the singleton/last-focused
                //      ActiveTerminal (the AI-panel session). This
                //      preserves the original Phase 5b behavior and
                //      keeps the existing `register_active_terminal`
                //      tests passing.
                let routed = match state.pane_session_by_id(&session_id) {
                    Some((view_id, pane_session)) => state
                        .pane_io_clone(view_id)
                        .map(|h| (h, ReplyTarget::Pane(pane_session))),
                    None => state
                        .active_terminal_clone()
                        .map(|h| (h, ReplyTarget::Singleton)),
                };
                let Some((handle, reply)) = routed else {
                    // No live pane for this session. Fire a snapshot
                    // exit immediately so the kernel's tool call
                    // doesn't sit on COMMAND_TIMEOUT for nothing.
                    match &reply_for_missing(&state, &session_id) {
                        ReplyTarget::Pane(s) => {
                            let _ = s.send_command_exit(command_id, None, true);
                        }
                        ReplyTarget::Singleton => {
                            let _ = state.send_command_exit(command_id, None, true);
                        }
                    }
                    continue;
                };
                tokio::spawn(async move {
                    handle_exec(handle, reply, command_id, command).await;
                });
            }
        }
    })
}

/// Where the broker should send `command_data` / `command_exit`
/// replies for a given exec. Picked once at exec time and held by the
/// per-command task — focus changes mid-execution don't redirect the
/// reply channel.
enum ReplyTarget {
    /// Per-pane WS — used by the inline `# foo` flow. Each pane has
    /// its own kernel session and its own outbound mpsc.
    Pane(Arc<PaneSession>),
    /// Singleton outbound — used by the AI panel session and by
    /// existing broker tests.
    Singleton,
}

/// Variant picker for the "no live pane" branch. We can't do an
/// `unwrap_or_else` with a closure that captures `state` (would need
/// MoveOnce semantics), so this small helper prefers the per-pane
/// reply channel when one exists for the session_id, falling back to
/// the singleton otherwise.
fn reply_for_missing(state: &OmwAgentState, session_id: &str) -> ReplyTarget {
    match state.pane_session_by_id(session_id) {
        Some((_, pane_session)) => ReplyTarget::Pane(pane_session),
        None => ReplyTarget::Singleton,
    }
}

/// Drive a single `bash/exec` to completion against the captured pane.
async fn handle_exec(
    handle: ActiveTerminalHandle,
    reply: ReplyTarget,
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
        send_exit(&reply, command_id, None, true);
        return;
    }

    let timeout = tokio::time::sleep(COMMAND_TIMEOUT);
    tokio::pin!(timeout);

    loop {
        tokio::select! {
            _ = &mut timeout => {
                send_exit(&reply, command_id, None, true);
                return;
            }
            chunk = rx.recv() => {
                let Ok(bytes) = chunk else {
                    // Sender dropped (pane closed). Treat as snapshot.
                    send_exit(&reply, command_id, None, true);
                    return;
                };
                let encoded = BASE64_STANDARD.encode(bytes.as_slice());
                send_data(&reply, command_id.clone(), encoded);
                if let Some(code) = detect_osc133_prompt_end(&bytes) {
                    send_exit(&reply, command_id, code, false);
                    return;
                }
            }
        }
    }
}

fn send_data(reply: &ReplyTarget, command_id: String, data: String) {
    let result = match reply {
        ReplyTarget::Pane(s) => s.send_command_data(command_id, data),
        ReplyTarget::Singleton => OmwAgentState::shared().send_command_data(command_id, data),
    };
    if let Err(e) = result {
        log::warn!("omw command_broker: send_command_data failed: {e}");
    }
}

fn send_exit(reply: &ReplyTarget, command_id: String, exit_code: Option<i32>, snapshot: bool) {
    let result = match reply {
        ReplyTarget::Pane(s) => s.send_command_exit(command_id, exit_code, snapshot),
        ReplyTarget::Singleton => {
            OmwAgentState::shared().send_command_exit(command_id, exit_code, snapshot)
        }
    };
    if let Err(e) = result {
        log::warn!("omw command_broker: send_command_exit failed: {e}");
    }
}

/// Detect Warp's `CommandFinished` shell-integration message and return the
/// reported exit code.
///
/// Warp's bundled shell hooks (`assets/bundled/bootstrap/{zsh,bash}_body.sh`)
/// emit a hex-encoded JSON payload wrapped in either:
///
///   - **DCS** (macOS, Linux): `ESC P $ d <hex> ST` — bytes
///     `\x1b\x50\x24\x64<hex>\x9c`. This is what the `warp_send_json_message`
///     function picks on non-Windows.
///   - **OSC 9278** (WSL / Windows): `ESC ] 9278 ; d ; <hex> BEL` — bytes
///     `\x1b]9278;d;<hex>\x07`.
///
/// The hex payload decodes to a JSON document whose `hook` field is
/// `"CommandFinished"` for command-end events. The corresponding
/// `value.exit_code` carries the shell's `$?` after the user command.
///
/// Returns:
///   - `Some(Some(code))` — `CommandFinished` JSON found with `exit_code`.
///   - `Some(None)` — `CommandFinished` JSON found but no parseable exit code.
///   - `None` — no Warp shell-integration end-of-command marker in the chunk.
///
/// Other hooks (`InitShell`, `Precmd`, `Preexec`, `InputBuffer`, …) ride the
/// same envelope but with different `hook` values; we deliberately ignore
/// them here so the broker only resolves on a real command boundary.
pub fn detect_osc133_prompt_end(bytes: &[u8]) -> Option<Option<i32>> {
    // Try DCS first (the macOS / Linux path). The DCS payload is bytes
    // between `ESC P $ d` and the ST byte `\x9c`.
    if let Some(payload) = extract_dcs_warp_payload(bytes) {
        if let Some(code) = parse_command_finished_from_hex(payload) {
            return Some(code);
        }
    }
    // Fall back to OSC 9278 (Windows / WSL). Payload is between
    // `ESC ] 9278 ; d ;` and the BEL byte `\x07`.
    if let Some(payload) = extract_osc_9278_warp_payload(bytes) {
        if let Some(code) = parse_command_finished_from_hex(payload) {
            return Some(code);
        }
    }
    None
}

/// Find `\x1b\x50\x24\x64...\x9c` and return the inner bytes.
fn extract_dcs_warp_payload(bytes: &[u8]) -> Option<&[u8]> {
    let prefix: &[u8] = b"\x1b\x50\x24\x64";
    let start = find_subslice(bytes, prefix)? + prefix.len();
    let after = &bytes[start..];
    let end = find_subslice(after, b"\x9c")?;
    Some(&after[..end])
}

/// Find `\x1b]9278;d;...\x07` and return the inner bytes.
fn extract_osc_9278_warp_payload(bytes: &[u8]) -> Option<&[u8]> {
    let prefix: &[u8] = b"\x1b]9278;d;";
    let start = find_subslice(bytes, prefix)? + prefix.len();
    let after = &bytes[start..];
    let end = find_subslice(after, b"\x07")?;
    Some(&after[..end])
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|w| w == needle)
}

/// Decode a hex-encoded JSON payload and, if it's a `CommandFinished` hook,
/// return its `exit_code` (or `Some(None)` when the field is missing).
fn parse_command_finished_from_hex(hex_bytes: &[u8]) -> Option<Option<i32>> {
    // Hex-decode (case-insensitive, two-char nibbles, ignore any
    // accidental whitespace from the shell `od`/`tr` pipeline).
    let mut out: Vec<u8> = Vec::with_capacity(hex_bytes.len() / 2);
    let mut hi: Option<u8> = None;
    for &b in hex_bytes {
        if b.is_ascii_whitespace() {
            continue;
        }
        let nibble = match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => 10 + (b - b'a'),
            b'A'..=b'F' => 10 + (b - b'A'),
            _ => return None,
        };
        match hi {
            None => hi = Some(nibble),
            Some(h) => {
                out.push((h << 4) | nibble);
                hi = None;
            }
        }
    }
    if hi.is_some() {
        return None; // odd-length hex
    }
    let json_str = std::str::from_utf8(&out).ok()?;
    let value: serde_json::Value = serde_json::from_str(json_str).ok()?;
    if value.get("hook").and_then(|v| v.as_str()) != Some("CommandFinished") {
        return None;
    }
    let exit_code = value
        .get("value")
        .and_then(|v| v.get("exit_code"))
        .and_then(|v| v.as_i64())
        .and_then(|n| i32::try_from(n).ok());
    Some(exit_code)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: build the wire bytes the shell emits via DCS for a given
    /// CommandFinished payload — mirrors `warp_send_json_message` on
    /// non-Windows.
    fn dcs_command_finished(exit_code: i32) -> Vec<u8> {
        let json = format!(
            "{{\"hook\":\"CommandFinished\",\"value\":{{\"exit_code\":{exit_code}}}}}"
        );
        let mut out = Vec::new();
        out.extend_from_slice(b"\x1b\x50\x24\x64");
        for byte in json.as_bytes() {
            out.extend_from_slice(format!("{byte:02x}").as_bytes());
        }
        out.push(0x9c);
        out
    }

    fn osc_9278_command_finished(exit_code: i32) -> Vec<u8> {
        let json = format!(
            "{{\"hook\":\"CommandFinished\",\"value\":{{\"exit_code\":{exit_code}}}}}"
        );
        let mut out = Vec::new();
        out.extend_from_slice(b"\x1b]9278;d;");
        for byte in json.as_bytes() {
            out.extend_from_slice(format!("{byte:02x}").as_bytes());
        }
        out.push(0x07);
        out
    }

    #[test]
    fn detects_dcs_command_finished_zero() {
        let bytes = dcs_command_finished(0);
        assert_eq!(detect_osc133_prompt_end(&bytes), Some(Some(0)));
    }

    #[test]
    fn detects_dcs_command_finished_nonzero() {
        let bytes = dcs_command_finished(127);
        assert_eq!(detect_osc133_prompt_end(&bytes), Some(Some(127)));
    }

    #[test]
    fn detects_osc_9278_command_finished() {
        let bytes = osc_9278_command_finished(2);
        assert_eq!(detect_osc133_prompt_end(&bytes), Some(Some(2)));
    }

    #[test]
    fn ignores_init_shell_dcs_message() {
        // Same envelope but `hook = "InitShell"` — must NOT terminate
        // the broker's wait loop.
        let json = "{\"hook\":\"InitShell\",\"value\":{\"session_id\":1}}";
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"\x1b\x50\x24\x64");
        for b in json.as_bytes() {
            bytes.extend_from_slice(format!("{b:02x}").as_bytes());
        }
        bytes.push(0x9c);
        assert_eq!(detect_osc133_prompt_end(&bytes), None);
    }

    #[test]
    fn no_marker_returns_none() {
        let bytes = b"plain output, no marker";
        assert_eq!(detect_osc133_prompt_end(bytes), None);
    }

    #[test]
    fn embedded_in_larger_chunk() {
        // The DCS marker may arrive interleaved with command stdout;
        // the broker must still extract it.
        let mut bytes = b"hello world\n".to_vec();
        bytes.extend_from_slice(&dcs_command_finished(0));
        bytes.extend_from_slice(b"\nuser@host:~$ ");
        assert_eq!(detect_osc133_prompt_end(&bytes), Some(Some(0)));
    }
}
