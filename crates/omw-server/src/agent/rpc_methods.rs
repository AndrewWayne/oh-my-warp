//! Closed vocabulary of omw's agent JSON-RPC *control-contract* method names
//! crossing the omw-server <-> omw-agent boundary.
//!
//! Single source of truth: `specs/agent-rpc-methods.txt` (a golden test below
//! asserts parity). Matching TS constants live in
//! `apps/omw-agent/src/rpc-methods.ts`.
//!
//! Scope: control contract only. Kernel observability events relayed from
//! pi-mono (`assistant/delta`, `tool/*`, `turn/*`, `error`) are the kernel's
//! vocabulary and are intentionally excluded — see issue #13.

pub const AGENT_CRASHED: &str = "agent/crashed";
pub const APPROVAL_DECIDE: &str = "approval/decide";
pub const APPROVAL_REQUEST: &str = "approval/request";
pub const BASH_CANCEL: &str = "bash/cancel";
pub const BASH_DATA: &str = "bash/data";
pub const BASH_EXEC: &str = "bash/exec";
pub const BASH_FINISHED: &str = "bash/finished";
pub const SESSION_CANCEL: &str = "session/cancel";
pub const SESSION_CREATE: &str = "session/create";
pub const SESSION_PROMPT: &str = "session/prompt";

/// Every control-contract method this crate declares a constant for.
pub const AGENT_RPC_METHODS: &[&str] = &[
    AGENT_CRASHED,
    APPROVAL_DECIDE,
    APPROVAL_REQUEST,
    BASH_CANCEL,
    BASH_DATA,
    BASH_EXEC,
    BASH_FINISHED,
    SESSION_CANCEL,
    SESSION_CREATE,
    SESSION_PROMPT,
];

#[cfg(test)]
mod tests {
    use super::AGENT_RPC_METHODS;

    /// Parse the checked-in source-of-truth list (skip `#` comments + blanks).
    fn source_of_truth() -> Vec<String> {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../specs/agent-rpc-methods.txt"
        );
        std::fs::read_to_string(path)
            .expect("read specs/agent-rpc-methods.txt")
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn source_of_truth_is_sorted_and_unique() {
        let methods = source_of_truth();
        let mut sorted = methods.clone();
        sorted.sort();
        assert_eq!(
            methods, sorted,
            "specs/agent-rpc-methods.txt must be sorted"
        );
        let unique: std::collections::BTreeSet<_> = methods.iter().collect();
        assert_eq!(unique.len(), methods.len(), "duplicate method in the list");
    }

    #[test]
    fn every_declared_constant_is_in_the_source_of_truth() {
        let methods = source_of_truth();
        for m in AGENT_RPC_METHODS {
            assert!(
                methods.iter().any(|f| f == m),
                "method {m:?} is declared in Rust but missing from specs/agent-rpc-methods.txt",
            );
        }
    }
}
