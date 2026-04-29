---
name: triage-rebase
description: Triage a failed nightly upstream Warp rebase. Classify each conflict by patch-series owner (omw/local-mode, omw/branding, etc.) and propose resolution. Use when the upstream rebase CI fails, when the user mentions a rebase conflict, or when reading an upstream-conflict issue.
tools: Bash, Read, Grep
---

# Triage Rebase

Reads the current rebase state and classifies each conflict against the patch-series defined in [`specs/fork-strategy.md`](../../../specs/fork-strategy.md). Suggests an owner and resolution path per conflict.

## Procedure

1. Run `git status` to confirm we're mid-rebase. If not, print: `"No active rebase. Run inside vendor/warp-fork/ during a rebase."` and exit.
2. Run `git diff --name-only --diff-filter=U` to list unmerged paths.
3. Read `specs/fork-strategy.md` to get the patch-series map. Expected series:
   - `omw/main` — integration branch (no patches, just merges)
   - `omw/local-mode` — `with_local_server` feature, identity, settings, agent panel wiring
   - `omw/branding` — binary rename, icon, color palette, wordmark replacements
   - (others as added in `specs/fork-strategy.md`)
4. For each unmerged path, classify by which patch series owns it:
   - Files under `crates/warp/src/local_server*` or `crates/warp/src/agent_panel/` → `omw/local-mode`
   - Files matching `*.icns`, `*.svg` for branding, or strings matching the brand wordmark → `omw/branding`
   - Files outside any known patch series → unowned (likely a clean upstream change we can take as-is)
5. For each conflict, run `git log --oneline omw/<series>` to find the most recent commit on that series and propose: "rebase against this commit" or "drop this hunk if upstream supersedes it."
6. Output a triage report.
7. Do NOT resolve conflicts automatically. This skill is advisory.

## Output format

```
Rebase triage — base: <upstream commit>, our HEAD: <our commit>

Conflicts (N):
  omw/local-mode (N files):
    - <path>          → resolution hint
  omw/branding (N files):
    - <path>          → resolution hint
  Unowned (N files):
    - <path>          → likely take upstream

Suggested order:
  1. Resolve omw/local-mode conflicts first (these are our largest patch surface).
  2. Resolve omw/branding (mostly mechanical — string/asset swaps).
  3. Take unowned files from upstream unless they touch a CLAUDE.md rule.

If stuck: file an upstream-conflict issue with this triage report attached.
```

## Notes

- During Phase 0, `specs/fork-strategy.md` may not exist yet. If missing, print: `"specs/fork-strategy.md not found — patch series unknown. Manual triage required."` and exit.
- Don't run `git rebase --continue` or `git rebase --abort` from this skill. Those are human decisions.
