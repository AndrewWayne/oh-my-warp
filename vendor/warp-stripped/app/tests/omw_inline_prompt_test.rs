//! Pure unit tests for the `# `-prefix inline-agent prompt parser.
//! Exercises the grammar rules from
//! `docs/archive/inline-agent-command-execution-report.md` §4.2 in isolation.
//!
//! The parser lives in `app/src/terminal/input.rs::parse_inline_agent_prompt`
//! and is exposed through `warp::test_exports` because the input module
//! itself is too large to load via the broken lib test target.

#![cfg(all(feature = "omw_local", feature = "test-exports"))]

use warp::test_exports::parse_inline_agent_prompt;

#[test]
fn hash_space_at_column_zero_returns_prompt_body() {
    assert_eq!(
        parse_inline_agent_prompt("# explain the test failure"),
        Some("explain the test failure")
    );
}

#[test]
fn hash_space_with_trailing_whitespace_is_trimmed() {
    assert_eq!(
        parse_inline_agent_prompt("# write the function   "),
        Some("write the function")
    );
}

#[test]
fn double_hash_falls_through_to_shell() {
    // `## …` is the documented escape hatch for a literal shell comment.
    assert_eq!(
        parse_inline_agent_prompt("## this is a literal shell comment"),
        None
    );
}

#[test]
fn hash_immediately_followed_by_non_space_falls_through() {
    // `#123` (e.g. a git commit message containing a ticket number) must
    // reach the shell unmodified.
    assert_eq!(parse_inline_agent_prompt("#123 fix bug"), None);
}

#[test]
fn hash_at_end_of_command_falls_through() {
    // Standard shell comment usage — never intercepted.
    assert_eq!(parse_inline_agent_prompt("echo foo # bar"), None);
}

#[test]
fn empty_body_falls_through() {
    // `# ` with nothing after it isn't a prompt.
    assert_eq!(parse_inline_agent_prompt("# "), None);
}

#[test]
fn multiline_buffer_falls_through() {
    // Heredocs and continuation prompts must not be intercepted.
    assert_eq!(
        parse_inline_agent_prompt("# this looks like a prompt\nbut it's two lines"),
        None
    );
}
