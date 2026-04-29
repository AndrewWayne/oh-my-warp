---
name: spec-consistency
description: Cross-reference PRD ↔ TODO ↔ specs/*.md for drift, broken cross-links, and orphaned bullets. Use when the user asks to verify documentation consistency, after editing PRD or any spec, before publishing the repo, or when proof-reading.
tools: Read, Grep, Bash
---

# Spec Consistency

Cross-checks that the planning docs agree with each other. The most useful command during the proof-reading phase.

## What gets checked

1. **PRD §3.1 ↔ TODO phase headings.** Every commitment in [PRD.md §3.1](../../../PRD.md#31-v10-committed-scope) should map to at least one task in [TODO.md](../../../TODO.md). Every TODO phase exit criterion should reflect §3.1 wording.
2. **PRD §13 ↔ TODO phases.** Phase IDs (`Phase 0`, `v0.1`, `v0.2`, ..., `v1.0`, `Beyond v1`) must match exactly between the two.
3. **specs/*.md cross-references.** Every file referenced in PRD or TODO (e.g. `specs/byorc-protocol.md`, `specs/fork-strategy.md`, `specs/threat-model.md`, `specs/test-plan.md`) must either exist or be clearly tagged as a Phase 0 deliverable yet to be written.
4. **Tier-1 provider list parity.** Provider names referenced in PRD §5.1, PRD §13 (v0.1 exit criteria), TODO v0.1, and `specs/test-plan.md` §4.3 must agree.
5. **Brand parity.** No file under the project root references the literal product name as anything other than `omw`. The codename `oh-my-warp` is allowed in repo paths and §12.1 explanations only.
6. **Test-plan ↔ phases.** `specs/test-plan.md` §8 per-phase commitments must use the same phase IDs as PRD §13 / TODO.

## Procedure

1. Read `PRD.md`, `TODO.md`, and every file under `specs/`.
2. For each check above, gather evidence with `Grep`/`Read`. Build a table of findings.
3. Report:
   - **Drift** — items mentioned in one doc but not another (with line refs).
   - **Broken refs** — links/cross-refs that point to files that don't exist (and aren't documented as TBD).
   - **Orphans** — TODO bullets with no corresponding PRD commitment, or PRD commitments with no TODO.
   - **Brand violations** — uses of `Warp` in product-surface contexts.
4. Do NOT edit any files. This skill is read-only.

## Output format

```
Spec consistency check
  PRD §3.1 ↔ TODO:        OK | DRIFT (N items)
  PRD §13 ↔ TODO phases:  OK | MISMATCH
  Cross-references:        OK | N broken (list)
  Tier-1 provider parity:  OK | DRIFT (where)
  Brand parity:            OK | N violations (path:line)
  Test-plan phases:        OK | MISMATCH

Verdict: CONSISTENT | DRIFT_DETECTED

<itemized findings if any>
```

## Notes

- This skill expects `specs/byorc-protocol.md`, `specs/fork-strategy.md`, `specs/threat-model.md` to be missing during Phase 0 — flag as TBD, not as broken.
- Run after every PRD or spec edit. Cheap and fast.
