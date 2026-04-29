---
name: check-scope
description: Verify a branch or PR stays inside PRD §3.1 v1.0 Committed Scope. Use when the user wants to check scope drift before opening a PR, when a contributor asks "is this in scope?", or before merging a non-trivial branch.
tools: Bash, Read, Grep
---

# Check Scope

Reports whether the current branch's changes stay inside the bright line drawn by [PRD §3.1 v1.0 Committed Scope](../../../PRD.md#31-v10-committed-scope) and the §13.x "Beyond v1" list.

## Procedure

1. Determine the diff range. Default: `main...HEAD`. If the user passes `$ARGUMENTS`, treat it as the diff range (e.g. `origin/main...HEAD`, `HEAD~5..HEAD`).
2. Run `git diff --name-only <range>` and `git diff --stat <range>` to enumerate touched paths and rough magnitude.
3. Read [`PRD.md` §3.1](../../../PRD.md#31-v10-committed-scope) and [`PRD.md` §13.x](../../../PRD.md#13x--beyond-v1-vision-not-committed) to extract the committed-scope checklist and the deferred items.
4. For each touched path, classify as:
   - **In scope** — maps to a §3.1 commitment (e.g. `crates/omw-agent/` ↔ "CLI agent + tools + MCP + approval + audit + cost").
   - **Out of scope** — maps to a §13.x Beyond-v1 item (e.g. anything in `apps/native-shim/`, `crates/omw-tsnet-gateway/`, Tier-2 provider crates).
   - **Infrastructural** — `.claude/`, `.github/`, `specs/`, docs, CI config — always allowed.
   - **Unclear** — flag for human review.
5. If any "Out of scope" or "Unclear" items exist, print a drift report with paths grouped by classification and a one-line rationale per group. If all "In scope" or "Infrastructural", print a clean confirmation.
6. Do NOT edit any files. This skill is read-only.

## Output format

```
Scope check: <range>
  In scope (N files):       <one-liner>
  Out of scope (N files):   <list>  ← block / discuss
  Unclear (N files):        <list>  ← needs review
  Infrastructural (N files): <one-liner>

Verdict: PASS | DRIFT_DETECTED
```

## Notes

- A path counted as "Out of scope" is a scope-creep candidate, not necessarily wrong — the user may have made a deliberate `Beyond v1` decision. Flag, don't reject.
- If `PRD.md` §3.1 has changed since the branch diverged, surface that explicitly.
