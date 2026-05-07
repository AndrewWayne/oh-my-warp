use crate::terminal::SizeInfo;
use std::borrow::Cow;

/// Messages that may be sent to the `EventLoop`.
#[derive(Debug)]
pub enum Message {
    /// Data that should be written to the PTY.
    Input(Cow<'static, [u8]>),

    /// Indicates that the `EventLoop` should be shut down.
    Shutdown,

    /// Indicates that the child process has exited.
    ///
    /// Only used on Windows, as we need to pass this information to the
    /// event loop via the channel (and cannot use the child event token).
    #[cfg_attr(not(windows), allow(dead_code))]
    ChildExited,

    /// Instruction to resize the PTY.
    Resize(SizeInfo),

    /// Synthetic bytes to feed through the ANSI parser as if they were
    /// real PTY output — used by the omw inline-agent path
    /// ([`OmwAgentState::send_prompt_inline`]) to render the agent's
    /// streaming response into the focused pane's block list. The
    /// renderer reads from a different channel than `pty_reads_tx`
    /// (which is a tap-only side-broadcast), so the only correct entry
    /// point is the same `parser.parse_bytes` call `pty_read` uses.
    /// We hop onto the event-loop thread to share its terminal lock
    /// + parser state — no synchronization gymnastics needed in
    /// callers.
    #[cfg(feature = "omw_local")]
    InjectBytes(Cow<'static, [u8]>),
}
