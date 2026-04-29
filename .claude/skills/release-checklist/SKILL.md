---
name: release-checklist
description: Walk the pre-release checklist from specs/test-plan.md §7 for a target version. Use when the user is preparing a release (tagging v0.1, v0.2, ..., v1.0), wants to verify release readiness, or asks "are we ready to ship?". Argument is the target version.
tools: Bash, Read, Grep
---

# Release Checklist

Walks the pre-release checklist defined in [`specs/test-plan.md` §7](../../../specs/test-plan.md) for the version passed as `$ARGUMENTS`. Reports per-item status. Read-only.

## Procedure

1. If `$ARGUMENTS` is empty, prompt for a target version (e.g. `v0.1`, `v0.4`, `v1.0`). Do not pick a default.
2. Read `specs/test-plan.md` §7 (pre-release checklist) and §8 (per-phase commitments) to get the version-specific checks.
3. For each item in the checklist:
   - **All Tier A green on the release commit** — run `git log --oneline -1` and check the latest CI status if accessible, else prompt the user to confirm.
   - **Tier B green for past 7 nights** — read CI history if accessible, else prompt.
   - **Manual `omw provider test`** — print: "Run manually with maintainer's real keys. Confirm green."
   - **Homebrew clean-install (v1.0+)** — print VM checklist.
   - **Real-Tailscale + real-phone smoke (v0.4+)** — print manual checklist.
   - **Upstream rebase smoke green for past 7 nights** — read CI history.
   - **No open sev-1 tracking issues** — `gh issue list --label sev-1 --state open` if `gh` available.
   - **CHANGELOG.md updated** — `git log` since last tag vs CHANGELOG diff.
   - **External review sign-off** — read `specs/byorc-protocol.md` (v0.4) or v1.0 review record if present.
4. For each item, mark: ✓ pass, ✗ fail, ? needs-human-confirm.
5. Print a summary verdict.
6. Do NOT cut tags, push, or run release tooling. This skill is advisory only.

## Output format

```
Release checklist — target: <version>

Phase commitments (test-plan.md §8 for <version>):
  [✓|✗|?] <commitment>

Pre-release checklist (test-plan.md §7):
  [✓|✗|?] All Tier A green on release commit
  [✓|✗|?] Tier B green past 7 nights
  [✓|✗|?] Manual provider test (Tier-1 × 4)
  [✓|✗|?] Homebrew clean-install (v1.0+)
  [✓|✗|?] Real-Tailscale + phone smoke (v0.4+)
  [✓|✗|?] Upstream rebase smoke past 7 nights
  [✓|✗|?] No open sev-1 tracking issues
  [✓|✗|?] CHANGELOG updated
  [✓|✗|?] External review sign-off

Verdict: READY | NOT READY (N items failing)
```

## Notes

- During Phase 0 / v0.1 this skill will report mostly `?` because CI infra isn't built yet. That's expected.
- If a checklist item is N/A for the target version (e.g. Homebrew for v0.1), mark it explicitly N/A rather than ✓ or ✗.
