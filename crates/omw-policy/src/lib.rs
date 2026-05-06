//! `omw-policy` — bash-command classifier wired into pi-agent's
//! `beforeToolCall` hook (via omw-server's `policy/classify` JSON-RPC).
//!
//! The classifier is intentionally conservative: anything we don't
//! recognise as obviously read-only falls into `Ask` under the default
//! `AskBeforeWrite` mode. The point is "no silent destructive actions"
//! (PRD §11.2 invariant), not a comprehensive sandbox.
//!
//! ## Modes
//!
//! - `ReadOnly` — only the built-in read-only set runs; everything else
//!   is `Deny` and the agent surfaces a tool error.
//! - `AskBeforeWrite` (default) — read-only set runs immediately;
//!   anything else (destructive built-ins, redirections, pipe-to-shell,
//!   anything we can't classify) prompts the user.
//! - `Trusted` — every command auto-runs. For dev / heavy-trust
//!   environments only; not the default.
//!
//! ## How decisions are reached
//!
//! 1. The first whitespace-delimited token of `cmd` is the head.
//! 2. Multi-word read-only forms (e.g. `git status`) match if the head
//!    matches and the second token equals the listed sub-command.
//! 3. The full command string is scanned for "metacharacters" that
//!    suggest write/exec/network behaviour: `>`, `>>`, `|`, `;`, `&&`,
//!    backticks, `$(...)`, etc. If any are present, the command is *not*
//!    classified as read-only — `AskBeforeWrite` returns `Ask`,
//!    `ReadOnly` returns `Deny`.
//! 4. Destructive built-ins (`rm`, `mv`, `chmod`, …) and any unknown
//!    command go to the same fallback bucket: `Ask` in `AskBeforeWrite`,
//!    `Deny` in `ReadOnly`.
//!
//! Quoting / glob expansion / heredocs are *not* parsed; the classifier
//! treats `cmd` as a string. A user wrapping a destructive op in a quoted
//! `bash -c "rm -rf foo"` still hits the destructive head, and the
//! metacharacter scan catches `>` / `|` etc. inside the quoted string.
//! A truly adversarial agent could probably evade this; that's acceptable
//! because v1's defence is the *user* via the `Ask` prompt, not the
//! classifier.
//!
//! Per [PRD §5.3](../../../PRD.md#53-local-first-agent-platform), this
//! crate is a thin configuration + classification surface — the actual
//! `beforeToolCall` decision lives in `apps/omw-agent/src/policy-hook.ts`
//! and the approval round-trip lives in omw-server.

use serde::{Deserialize, Serialize};

/// Per-call decision returned by [`classify`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Decision {
    /// Run immediately; no prompt.
    Allow,
    /// Prompt the user before running.
    Ask,
    /// Refuse to run; agent surfaces a tool error.
    Deny,
}

/// Approval mode — coarse policy axis configured per omw install.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalMode {
    /// Only built-in read-only commands run; the rest are denied outright.
    ReadOnly,
    /// Read-only commands run immediately; everything else prompts.
    /// Default per PRD §5.3.
    AskBeforeWrite,
    /// Every command runs without prompting. Dev-only.
    Trusted,
}

/// Full classifier configuration. Optional `allow` / `deny` lists let the
/// user override built-in classification on a per-command basis. Patterns
/// are matched as exact head tokens (no regex / no glob — keep it
/// auditable).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyConfig {
    pub mode: ApprovalMode,
    /// Heads that always classify as `Allow` (overrides the built-in
    /// destructive set).
    #[serde(default)]
    pub allow: Vec<String>,
    /// Heads that always classify as `Deny`.
    #[serde(default)]
    pub deny: Vec<String>,
}

impl Default for PolicyConfig {
    fn default() -> Self {
        Self::default_ask_before_write()
    }
}

impl PolicyConfig {
    pub fn default_ask_before_write() -> Self {
        Self {
            mode: ApprovalMode::AskBeforeWrite,
            allow: Vec::new(),
            deny: Vec::new(),
        }
    }

    pub fn read_only() -> Self {
        Self {
            mode: ApprovalMode::ReadOnly,
            allow: Vec::new(),
            deny: Vec::new(),
        }
    }

    pub fn trusted() -> Self {
        Self {
            mode: ApprovalMode::Trusted,
            allow: Vec::new(),
            deny: Vec::new(),
        }
    }
}

/// Classify a bash-style command string against `cfg`.
pub fn classify(cmd: &str, cfg: &PolicyConfig) -> Decision {
    let trimmed = cmd.trim();
    if trimmed.is_empty() {
        // Empty command: nothing to run, treat as Deny so the agent
        // doesn't get a free turn doing nothing.
        return Decision::Deny;
    }

    let head = head_token(trimmed);
    let head_lower = head.to_ascii_lowercase();

    // Per-config overrides win first.
    if cfg.deny.iter().any(|p| p.eq_ignore_ascii_case(&head_lower)) {
        return Decision::Deny;
    }
    if cfg.allow.iter().any(|p| p.eq_ignore_ascii_case(&head_lower)) {
        return Decision::Allow;
    }

    if cfg.mode == ApprovalMode::Trusted {
        return Decision::Allow;
    }

    let metachars = has_shell_metacharacters(trimmed);
    let read_only = !metachars && is_read_only(&head_lower, trimmed);

    if read_only {
        return Decision::Allow;
    }

    match cfg.mode {
        ApprovalMode::ReadOnly => Decision::Deny,
        ApprovalMode::AskBeforeWrite => Decision::Ask,
        ApprovalMode::Trusted => Decision::Allow, // unreachable (handled above)
    }
}

/// First whitespace-delimited token. Empty string maps to "".
fn head_token(cmd: &str) -> &str {
    cmd.split_whitespace().next().unwrap_or("")
}

/// Detects shell metacharacters that imply write / chain / exec / network
/// behaviour. Conservative — false positives degrade to `Ask`, which is
/// strictly better than missing a destructive operation.
fn has_shell_metacharacters(cmd: &str) -> bool {
    // Look-up only on the bytes themselves; we don't model quoting (a
    // quoted `>` still hits this match — that's the conservative choice).
    for &b in cmd.as_bytes() {
        match b {
            b'>' | b'|' | b';' | b'`' => return true,
            _ => {}
        }
    }
    if cmd.contains("$(") {
        return true;
    }
    if cmd.contains("&&") || cmd.contains("||") {
        return true;
    }
    false
}

/// Built-in read-only classification. Returns true if this `head` (and
/// optional sub-command) is on the safe-by-default list. Caller has
/// already excluded commands containing shell metacharacters.
fn is_read_only(head: &str, full_cmd: &str) -> bool {
    // Single-token read-only commands.
    const READ_ONLY_HEADS: &[&str] = &[
        "pwd", "date", "ls", "rg", "cat", "head", "tail", "wc", "echo", "true", "false", "uptime",
        "whoami", "hostname", "uname", "tty", "id", "groups", "which", "type", "env",
    ];
    if READ_ONLY_HEADS.contains(&head) {
        return true;
    }

    // Two-token read-only forms: `git status`, `git diff`, `git log`,
    // `git show`, `git branch`, `git remote`. Anything else under `git`
    // is *not* read-only by default (push, pull, commit, reset, …).
    if head == "git" {
        if let Some(sub) = full_cmd.split_whitespace().nth(1) {
            const GIT_READ_ONLY: &[&str] = &[
                "status", "diff", "log", "show", "branch", "remote", "config", "blame",
            ];
            return GIT_READ_ONLY.contains(&sub);
        }
        return false;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ask_default() -> PolicyConfig {
        PolicyConfig::default_ask_before_write()
    }

    #[test]
    fn empty_command_is_denied() {
        assert_eq!(classify("", &ask_default()), Decision::Deny);
        assert_eq!(classify("   ", &ask_default()), Decision::Deny);
    }

    #[test]
    fn read_only_singletons_allow() {
        let cfg = ask_default();
        for cmd in ["pwd", "date", "ls", "ls -la", "rg foo", "cat README.md"] {
            assert_eq!(classify(cmd, &cfg), Decision::Allow, "{cmd}");
        }
    }

    #[test]
    fn destructive_singletons_ask_in_default() {
        let cfg = ask_default();
        for cmd in [
            "rm -rf /",
            "mv a b",
            "chmod 0777 file",
            "chown user file",
            "kill 123",
        ] {
            assert_eq!(classify(cmd, &cfg), Decision::Ask, "{cmd}");
        }
    }

    #[test]
    fn unknown_command_asks() {
        let cfg = ask_default();
        // Random binary that isn't on either list.
        assert_eq!(classify("frobnitz --do-something", &cfg), Decision::Ask);
    }

    #[test]
    fn git_status_and_diff_allow_other_git_subcommands_ask() {
        let cfg = ask_default();
        assert_eq!(classify("git status", &cfg), Decision::Allow);
        assert_eq!(classify("git diff", &cfg), Decision::Allow);
        assert_eq!(classify("git log --oneline", &cfg), Decision::Allow);
        assert_eq!(classify("git push origin main", &cfg), Decision::Ask);
        assert_eq!(classify("git reset --hard HEAD", &cfg), Decision::Ask);
        assert_eq!(classify("git commit -m 'x'", &cfg), Decision::Ask);
    }

    #[test]
    fn shell_metacharacters_force_ask_even_for_safe_heads() {
        let cfg = ask_default();
        // ls is safe alone; redirection pushes it to Ask.
        assert_eq!(classify("ls > out.txt", &cfg), Decision::Ask);
        assert_eq!(classify("cat foo | sh", &cfg), Decision::Ask);
        assert_eq!(classify("echo hi; rm bar", &cfg), Decision::Ask);
        assert_eq!(classify("true && rm -rf /", &cfg), Decision::Ask);
        assert_eq!(classify("date `evilcmd`", &cfg), Decision::Ask);
        assert_eq!(classify("date $(evilcmd)", &cfg), Decision::Ask);
    }

    #[test]
    fn read_only_mode_denies_anything_not_on_safe_list() {
        let cfg = PolicyConfig::read_only();
        assert_eq!(classify("pwd", &cfg), Decision::Allow);
        assert_eq!(classify("git status", &cfg), Decision::Allow);
        assert_eq!(classify("rm foo", &cfg), Decision::Deny);
        assert_eq!(classify("git push", &cfg), Decision::Deny);
        assert_eq!(classify("ls > out", &cfg), Decision::Deny);
        assert_eq!(classify("frobnitz", &cfg), Decision::Deny);
    }

    #[test]
    fn trusted_mode_always_allows() {
        let cfg = PolicyConfig::trusted();
        assert_eq!(classify("pwd", &cfg), Decision::Allow);
        assert_eq!(classify("rm -rf /", &cfg), Decision::Allow);
        assert_eq!(classify("git push --force", &cfg), Decision::Allow);
        assert_eq!(classify("anything; goes", &cfg), Decision::Allow);
    }

    #[test]
    fn config_deny_list_overrides_safe_classification() {
        let cfg = PolicyConfig {
            mode: ApprovalMode::AskBeforeWrite,
            allow: Vec::new(),
            deny: vec!["pwd".into()],
        };
        assert_eq!(classify("pwd", &cfg), Decision::Deny);
        // ls is still safe.
        assert_eq!(classify("ls", &cfg), Decision::Allow);
    }

    #[test]
    fn config_allow_list_overrides_destructive_classification() {
        let cfg = PolicyConfig {
            mode: ApprovalMode::AskBeforeWrite,
            allow: vec!["rm".into()],
            deny: Vec::new(),
        };
        assert_eq!(classify("rm foo", &cfg), Decision::Allow);
        // unknown still asks.
        assert_eq!(classify("frobnitz", &cfg), Decision::Ask);
    }

    #[test]
    fn deny_list_wins_over_allow_list() {
        // If both contain the same head (operator misconfig), deny wins —
        // matches "fail closed" defaults.
        let cfg = PolicyConfig {
            mode: ApprovalMode::AskBeforeWrite,
            allow: vec!["rm".into()],
            deny: vec!["rm".into()],
        };
        assert_eq!(classify("rm foo", &cfg), Decision::Deny);
    }

    #[test]
    fn case_insensitive_head_matching() {
        let cfg = ask_default();
        assert_eq!(classify("LS", &cfg), Decision::Allow);
        assert_eq!(classify("Ls -la", &cfg), Decision::Allow);
    }

    #[test]
    fn serde_round_trip_decision() {
        let allow = serde_json::to_string(&Decision::Allow).unwrap();
        assert_eq!(allow, "\"allow\"");
        let parsed: Decision = serde_json::from_str("\"deny\"").unwrap();
        assert_eq!(parsed, Decision::Deny);
    }

    #[test]
    fn serde_round_trip_mode() {
        let mode = serde_json::to_string(&ApprovalMode::AskBeforeWrite).unwrap();
        assert_eq!(mode, "\"ask_before_write\"");
        let parsed: ApprovalMode = serde_json::from_str("\"trusted\"").unwrap();
        assert_eq!(parsed, ApprovalMode::Trusted);
    }
}
