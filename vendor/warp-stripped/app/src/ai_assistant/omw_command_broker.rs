//! Phase 5b — GUI command broker. Currently ships only the OSC 133
//! detection algorithm; the spawn_command_broker async loop and
//! TerminalView integration are deferred to a follow-up (the active
//! terminal's `event_loop_tx`/`pty_reads_rx` plumbing into the warp-stripped
//! pane is a substantial refactor).

#![cfg(feature = "omw_local")]

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
