---
name: tech-debt-audit
description: Use when the user asks for a debt audit, codebase health check, architecture review, quality assessment of the entire omw repo, or a "what's rotten?" survey. Whole-repo scope, not diff-scoped (use /review for diffs, /simplify for tactical cleanup). Optional argument is a subtree (e.g. "crates/omw-server", "vendor/warp-stripped/app/src/ai_assistant"). User-invoked only — does not auto-trigger.
tools: Bash, Read, Grep, Write, Edit, Agent
disable-model-invocation: true
---

# Tech Debt Audit

Whole-repo, opinionated, file-cited audit. Produces a dated `docs/tech-debt-audit-<YYYY-MM-DD>.md` you can commit and revisit. Designed to find what's actually wrong in the omw delta — not generic best-practice violations dressed up as findings.

The companion skills [`/spec-consistency`](../spec-consistency/SKILL.md) and [`/check-scope`](../check-scope/SKILL.md) cover narrow slices (PRD↔TODO drift, scope drift); this is the comprehensive sweep.

## Operating principles

Find what's actually wrong. Not diplomatic. Not surface-only. Don't pattern-match to generic best practices without grounding in this specific repo. No sycophancy. No "overall the codebase is well-structured" filler.

Cite `file:line` for every concrete finding. Vague claims like "the code generally..." don't count. Read code before judging it — a pattern that looks wrong in isolation may be load-bearing.

**Required output section:** "Things that look bad but are actually fine." If empty, the audit was shallow. Forcing the model to surface calls it considered making and chose not to is what separates a real audit from a checklist regurgitation.

## When to invoke

- Before a major version bump (e.g. v0.0.x → v0.1, v0.x → v1.0).
- When the user explicitly asks for a debt audit, health check, or "what should we clean up?".
- After a quarter (or quarter-equivalent) of feature work has accumulated unaudited.

Do NOT invoke for:
- Pending-change review — use `/review` (diff-scoped) instead.
- Tactical cleanup of one area — use `/simplify` instead.
- Spec drift only — use `/spec-consistency` instead.
- A specific failure or unexpected behavior — use systematic-debugging.

## Scope rules (project-specific — these are the adaptation)

These rules narrow the audit to the omw delta and prevent re-flagging items already triaged. Read them before Phase 1.

1. **Vendor fork rule** ([CLAUDE.md §5](../../../CLAUDE.md), [`specs/fork-strategy.md`](../../../specs/fork-strategy.md) §2). Audit *only* the omw delta inside `vendor/warp-stripped/`:
   - `vendor/warp-stripped/app/src/omw/**`
   - `vendor/warp-stripped/app/src/ai_assistant/omw_*.rs`
   - `vendor/warp-stripped/app/src/settings_view/omw_agent_page*.rs`
   - Any vendor file with the project's incremental-authorship AGPL header attributing omw contributors.

   Upstream Warp files (everything else under `vendor/warp-stripped/`) are NOT debt — they're inherited from upstream and managed via the manual sync procedure. Flagging them produces noise, not findings.

2. **Phase-N gating is not debt.** PRD §13 and TODO.md document a v0.0.x preview cadence with intentionally partial scope. Code that is `#[cfg(feature = "omw_local")]`-gated, `#![allow(dead_code)]`-marked at file level, or stubbed pending a later phase is documented behavior. Don't flag it as dead code unless cross-referenced evidence shows the gate has shipped without the implementation.

3. **Already-triaged items.** Read the most recent `docs/simplify-followups-*.md` before Phase 2. Its §4 "Things that look bad but are actually fine (load-bearing)" enumerates items already investigated and confirmed live. Don't re-flag them. If the audit produces new evidence that overturns one, cite the prior triage and explain why.

4. **Frozen historical artifacts.** `RELEASE_NOTES_v*.md` and `docs/archive/**` are dated snapshots. Don't flag their contents as drift — they're history.

5. **Cross-stack reality.** This repo has two cargo workspaces (root + `vendor/warp-stripped/`) and one npm workspace (`apps/*` with hoisted `node_modules` at repo root). Tooling commands must respect the workspace they're in.

## Procedure

### Phase 1: Orient

Do not skip. Forming opinions before understanding the system produces bad audits.

1. Read [`PRD.md`](../../../PRD.md), [`TODO.md`](../../../TODO.md), [`CLAUDE.md`](../../../CLAUDE.md), every file under [`specs/`](../../../specs/), and the most recent `docs/tech-debt-audit-*.md` and `docs/simplify-followups-*.md` if either exists.
2. Read the manifests: root `Cargo.toml`, `vendor/warp-stripped/Cargo.toml`, `apps/omw-agent/package.json`, `apps/web-controller/package.json`, root `package.json`.
3. Map the directory structure. Major modules: `crates/omw-{acp,audit,cli,config,keychain,keychain-helper,policy,pty,remote,server}`, `apps/{omw-agent,web-controller}`, `vendor/warp-stripped/app/src/{omw,ai_assistant/omw_*,settings_view/omw_agent_page*}`.
4. Run `git log --oneline -200` and `git log --stat --since="3 months ago"` to see where churn concentrates. The repo is young (~5 weeks at first audit); 6 months is too long.
5. List the top 20 largest files by line count and the top 20 most-modified files in the last 3 months. **Exclude** `vendor/warp-stripped/` files that don't match the omw delta predicate from scope rule §1. The intersection of "large" and "frequently modified" is where debt usually hides.
6. Use `TaskCreate` to publish a phase plan so the user sees progress.

Write a 1–2 paragraph mental model of the omw delta architecture *and* the fork posture (in-tree vs upstream) before proceeding. If your model contradicts PRD.md or specs/fork-strategy.md, flag it — that itself is a finding.

### Phase 2: Audit across these dimensions

Use `Grep`, `Read`, and the tooling listed in §"Per-stack tooling" below. Cite `path/to/file.ext:LINE` on every finding.

1. **Architectural decay** — circular deps, layering violations, god files (>800 LOC for owned code; >1500 LOC for vendor omw_* files because some Warp upstream conventions warrant size), god functions, duplicated logic across 3+ sites where an abstraction exists or should, abstractions with one user, dead code (unused exports, unreachable branches, stale commented-out blocks). Cross-reference scope rule §3 before flagging dead code.

2. **Consistency rot** — multiple ways of doing the same thing across the omw delta. Common candidates:
   - Provider-name vocabulary (`"openai" / "anthropic" / "openai-compatible" / "ollama"`) — should appear in one place per stack, not five.
   - JSON-RPC method strings vs typed enums.
   - HTTP/WS handler patterns across `crates/omw-server` and `crates/omw-remote`.
   - Path canonicalization (recurring class — see omw-config commit `f5a8cc4`).
   - Provider/keychain wrapper patterns.

3. **Type & contract debt**:
   - TS: `any`, `unknown`, `as any`, `// @ts-ignore`, `// @ts-expect-error`, untyped JSON parsing at trust boundaries.
   - Rust: `serde_json::Value` flowing through 3+ layers without ever being typed; protocol JSON shapes lacking matching Rust struct + TS interface.
   - Cross-stack: protocol shapes defined twice (Rust enum + TS string-literal) where one drifted. The `ApprovalDecision` rename in commit `3df3053` is the canonical example.
   - omw-remote endpoints lacking the contract test + fuzz target gate from CLAUDE.md §5.

4. **Test debt**:
   - High-churn omw_* files in `vendor/warp-stripped/` lacking `#[cfg(test)]` coverage.
   - `#[ignore]`'d Rust tests and skipped vitest tests with no tracking issue.
   - Tests asserting wire-format strings (`assert_eq!(method, "session/create")`) instead of typed enums — these break silently when the wire format moves.
   - High-churn TS modules with no `*.test.ts` neighbor.

5. **Dependency hygiene**:
   - Unused deps in `[dependencies]` (Cargo) and `dependencies` (package.json). Root + vendor cargo workspaces audited separately.
   - Same dep declared in both Cargo workspaces with different versions — runs the risk of a mismatched ABI when the vendor side links omw crates.
   - npm workspace hoist mismatches: `apps/<x>/package.json` declares dep, but the bundle script picks up the hoisted root `node_modules/`. This was the v0.0.3-rev2 ship bug.
   - Cargo deps declared `optional = true` with no `[features]` block to enable them — they never get built (Stage 1 of the May 2026 /simplify pass found three such cases).

6. **Error handling & observability**:
   - Production `eprintln!` and `/tmp/*.log` writes that bypass the log facade. Production debug-log writes are an open question in `docs/simplify-followups-2026-05-09.md` §1.4 — re-evaluate state on each audit run.
   - Swallowed `Result`s (`let _ = …;` on a fallible call where the error is actionable).
   - Inconsistent error shapes returned from omw-server / omw-remote routes.
   - Missing structured logs on critical paths (agent boot, pairing, capability redeem).

7. **Brand & vendor delta hygiene** (per CLAUDE.md §5):
   - Stray capitalized `Warp` in product-surface code, docs, or UI strings (allowed in source-attribution, `LICENSE`, `oh-my-warp` codename, `vendor/warp-stripped/` upstream files only).
   - omw_* files inside `vendor/warp-stripped/` lacking the AGPL-3.0 header that attributes incremental authorship to omw contributors.
   - Edits to upstream Warp files (everything outside the omw delta in `vendor/warp-stripped/`) that weren't part of an upstream sync — those should be in omw_* files instead.

8. **Spec ↔ code drift**:
   - PRD claims (especially §3.1 v1.0 Committed Scope and §13 phased roadmap) that don't match what the code actually does.
   - `specs/*.md` references to files that don't exist and aren't documented as TBD.
   - Phase-ID mismatches between PRD §13 and TODO.md headings.
   - The `/spec-consistency` skill output is an input here — fold its findings in rather than re-deriving them.

9. **BYORC contract surface** (per CLAUDE.md §5):
   - New endpoints in `crates/omw-remote/` lacking a contract test ([`specs/test-plan.md`](../../../specs/test-plan.md) §1.2) or a fuzz target ([§3.1](../../../specs/test-plan.md#31)).
   - Protocol variants in `omw_protocol.rs` defined but never produced or consumed (cross-check producers in TS adapters and consumers in Rust handlers).
   - Capability scopes minted in `pair_redeem` but never enforced server-side.

If a category has nothing material, write "Nothing material" and move on. Don't pad.

### Phase 3: Deliverable

Write to `docs/tech-debt-audit-<YYYY-MM-DD>.md` (today's date). The dated-filename pattern matches `docs/agpl-compliance-audit-2026-05-01.md` and `docs/simplify-followups-*.md` — successive audits append rather than overwrite.

Sections (in this order):

- **Executive summary** — max 10 bullets, ranked by impact. Include severity counts (e.g. "2 Critical, 8 High, 24 Medium, 12 Low") and the largest debt concentration ("most findings in `crates/omw-server/handlers/`").
- **Architectural mental model** — your read of the omw delta + the fork posture (in-tree vs upstream sync).
- **Findings table** — columns: `ID | Category | File:Line | Severity (Critical/High/Medium/Low) | Effort (S/M/L) | Description | Recommendation`. Aim for 20–60 findings; padding past that is noise. The repo is small.
- **Top 5 "if you fix nothing else, fix these"** — concrete diff sketches or refactor outlines, not vague advice. Reference finding IDs.
- **Quick wins** — Low effort × Medium+ severity, as a checklist with finding IDs.
- **Things that look bad but are actually fine** — calls you considered flagging and chose not to, with reasoning. **REQUIRED.** If empty, the audit was shallow. Cross-reference the prior `docs/simplify-followups-*.md` §4 and add new entries.
- **Open questions for the maintainer** — things you couldn't tell were debt vs. intentional. Don't assert; ask.

## Per-stack tooling

Detect the stack from the manifest. Run the relevant tools. Run independent commands in parallel.

**Rust (root workspace):**
```
cargo check --workspace --tests             # baseline + warnings
cargo clippy --workspace --tests            # idiom drift
cargo machete                                # unused deps (install if missing: cargo install cargo-machete)
```

**Rust (vendor workspace — separate workspace, must `cd`):**
```
cd vendor/warp-stripped
cargo check -p warp --features omw_local --bin warp-oss
cargo clippy -p warp --features omw_local --bin warp-oss
cargo machete                                # vendor's own deps
```

**TypeScript (`apps/web-controller` and `apps/omw-agent`):**
```
cd apps/web-controller && npx tsc -b && npx knip && npx depcheck
cd apps/omw-agent && npx tsc -b && npx knip && npx depcheck
```

**Cross-stack:** Invoke the project's `/spec-consistency` skill and fold its output into Phase 2 dimension 8.

If a tool isn't installed, note it in the audit and move on. Do NOT install dev tools globally without permission. Skip `cargo audit` and `npm audit` — CVE scanning is covered by CI and is not what a debt audit is for.

## Subagent dispatch (large-pass mode)

Default: single-agent run. The repo is bounded.

For a deeper pass (or if the user explicitly asks for parallelism), dispatch parallel Opus subagents — same shape used in the May 2026 `/simplify` pass at HEAD `9def1ac`:

- **Agent 1 — Rust crates** (`crates/`, root cargo workspace).
- **Agent 2 — TypeScript apps** (`apps/omw-agent`, `apps/web-controller`).
- **Agent 3 — omw delta in vendor** (`vendor/warp-stripped/app/src/{omw,ai_assistant/omw_*}`, `vendor/warp-stripped/Cargo.toml`).

Each gets: the scope rules above (especially §1 vendor delta predicate), the 9 dimensions, the citation requirement, and a 100-finding cap. The main agent merges, dedupes, ranks. The `/simplify` follow-ups doc has a worked example of agent prompts.

## Repeat-run mode

If `docs/tech-debt-audit-*.md` already exists, read the most recent one before Phase 2.

- For each prior finding still in the codebase, mark it `UNCHANGED` (or update if circumstances shifted).
- For each prior finding that's been fixed, mark it `RESOLVED` and cite the commit SHA + file/line evidence.
- For each new finding surfaced this run, tag it `NEW`.
- Save as a new dated file (`docs/tech-debt-audit-<today>.md`). Do not overwrite — the audits are a time series.

## Output format

```
docs/tech-debt-audit-<YYYY-MM-DD>.md generated.

Executive summary:
  2 Critical | 8 High | 24 Medium | 12 Low (46 findings)
  Largest concentration: crates/omw-server/handlers/ (1 of 2 Critical, 4 of 8 High)
  RESOLVED since last audit: 7. NEW: 13. UNCHANGED: 26.

Top 5:
  F001 — <one-line>
  F002 — ...
  ...

Looks bad but is fine: <count> entries (cross-referenced docs/simplify-followups-*.md §4 + N new)
Open questions: <count>

Verdict: REVIEW REQUIRED — see docs/tech-debt-audit-<YYYY-MM-DD>.md
```

## Notes

- This is a static audit, not a security audit. The threat model lives in [`specs/threat-model.md`](../../../specs/threat-model.md); the BYORC pairing/auth invariants live in [`specs/byorc-protocol.md`](../../../specs/byorc-protocol.md). Refer rather than re-derive.
- It will not catch business-logic bugs.
- It cannot perfectly distinguish intentional simplicity from accidental simplicity. The "Open questions" section exists for exactly this reason.
- The skill explicitly forbids:
  - **Recommending rewrites.** Recommend specific, scoped changes.
  - **Padding categories.** "Nothing material" is a valid entry.
  - **Sycophancy.** No "overall the codebase is well-structured" filler.
  - **Auditing upstream Warp** files outside the omw delta predicate. They're not omw's debt.
- For very large later versions of the repo (>200k LOC), even subagent dispatch can produce shallow results. Scope to a module: `/tech-debt-audit crates/omw-server`.
