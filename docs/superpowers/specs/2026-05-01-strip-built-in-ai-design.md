# Strip Built-In AI from `vendor/warp-stripped` — Design

- **Date:** 2026-05-01
- **Topic:** Expanding the `omw_local` Cargo feature so the warp-oss build has no AI/cloud user surfaces and excludes cloud-shaped code paths from the binary.
- **Owner:** Jiaqi Cai
- **Related:** [PRD §3.1](../../../PRD.md#31-v10-committed-scope), [PRD §13 v0.3](../../../PRD.md), [specs/fork-strategy.md](../../../specs/fork-strategy.md), [vendor/warp-stripped/OMW_LOCAL_BUILD.md](../../../vendor/warp-stripped/OMW_LOCAL_BUILD.md), [CLAUDE.md §5](../../../CLAUDE.md#5-project-specific-rules)

## 1. Motivation

Today the `warp-oss` binary built from `vendor/warp-stripped/` shows the upstream
Warp signup/onboarding wall on launch and exposes account/AI menus that point at
`app.warp.dev`. The existing `omw_local` Cargo feature only stubs cloud server
URLs and skips firebase anonymous-user creation; it does **not** disable any of
the user-facing signup/AI/account surfaces or remove the cloud-client code from
the binary. The omw product roadmap (PRD §3.1) puts a forked Warp client at v0.3
that *re-introduces* an agent panel routed through `omw-server`/`omw-agent`. This
design covers v0.1 — getting to a clean stripped client with no built-in AI —
without painting v0.3 into a corner.

## 2. Decisions (from brainstorming)

| Axis | Choice |
|------|--------|
| Scope | UI surfaces **and** dead code paths (gates the cloud-shaped modules out of the binary). |
| Strategy | Source-level `#[cfg(not(feature = "omw_local"))]` gates plus `optional = true` in workspace Cargo.toml for cloud-only crates. Reversible. |
| Agent / AI panel UX | Visible but rendering a placeholder pointing at the future omw integration. |
| Approach | Single top-down comprehensive pass — one design, one PR. |
| Edit location | `vendor/warp-stripped/` only. Per CLAUDE.md §5, `vendor/warp-fork/` is read-only from this repo. |

## 3. Architecture

The change is purely additive to the existing `omw_local` feature. After this
work:

- `omw_local` ON (the default for `cargo build -p warp --bin warp-oss --features omw_local`)
  → no AI/cloud user surfaces visible; cloud-shaped crates not linked into the binary.
- `omw_local` OFF → upstream-Warp behavior is preserved (modulo the existing
  branding/scope strips already in `vendor/warp-stripped`).

Build invocation does not change.

## 4. What gets gated

### 4.1 UI surfaces

Gated at the dispatcher that renders them:

- Onboarding / signup window and any "Sign up to use Warp" walls
- Account / login / Warp Drive settings tabs
- "Sign in to Warp" entries in Help, Profile, Command Palette
- Cloud-sync settings, "Resume on warp.dev" affordances
- Voice input UI

### 4.2 Code modules

Gated at module declaration **or** marked `optional = true` in workspace deps and
excluded from `omw_local`:

- `crates/firebase`
- `crates/warp_server_client`
- `crates/managed_secrets`
- `crates/onboarding`
- `crates/voice_input`
- `crates/command-signatures-v2`
- Within `crates/ai/`: cloud-AI provider modules and request-routing to
  `*.warp.dev`. The provider trait/abstraction layer **stays** so v0.3 can plug
  `omw-server` in.
- Within `crates/graphql/`: gate the API client. Schema types are kept if other
  non-cloud code depends on them.

The exact crate list is the brainstorming-stage estimate; the implementation
plan (writing-plans phase) audits the full graph and may add or drop members.

### 4.3 Agent / AI panel placeholder

When `omw_local` is on, the agent / AI panel renders an inline message:

> AI is unavailable in this build. Configure providers via `omw provider add`
> in your terminal — full omw integration is coming in v0.3.

Exact copy and any link/button details are an open question (§7).

## 5. Error handling

Code paths that previously called gated modules become no-ops or return
"feature unavailable":

- Keyboard shortcuts for AI/agent actions are still registered but show a
  one-line status-bar message.
- `warp://` deep links targeting AI surfaces are logged and ignored — no crash.
- Tests that exercise gated code are themselves gated, so they don't run under
  `omw_local`.

There is no graceful-fallback to a different provider. That is v0.3 work.

## 6. Verification

Three layers, in order of cost:

1. **Build-level.**
   - `cargo build -p warp --bin warp-oss --features omw_local` succeeds.
   - `cargo build` (no flag) also still succeeds — proves upstream-Warp
     restorability is intact.
2. **Static audit script.**
   - A shell or small Rust binary at `vendor/warp-stripped/scripts/audit-no-cloud.sh`
     that greps the linked `warp-oss` binary for `app.warp.dev`, `api.warp.dev`,
     `firebase.googleapis.com`, `warp.dev`, asserting zero hits outside test
     fixtures.
   - Wire into CI as a job that runs after the warp-oss build step.
3. **Manual smoke test.**
   - Recorded as a step in `specs/test-plan.md` for the release checklist.
   - Launch `warp-oss`, observe: no signup wall on first launch; no AI / sign-in
     CTAs in any menu; settings opens with no AI/cloud sections; during a
     60-second exercise of normal terminal flow, `nettop -m route` shows zero
     outbound packets to `*.warp.dev` or `firebase.*` domains.

## 7. Open questions

- **Placeholder copy.** Exact wording and any link/button. Defer until the
  implementation plan.
- **Exact crate boundary for `crates/ai`.** Some submodules (e.g. provider trait,
  prompt formatting) are useful for v0.3; others (cloud routing) are not. The
  audit during the implementation plan will draw the line.
- **Whether to also disable upstream Warp's hundred-plus `ai_*` / `agent_*` /
  `cloud_*` Cargo feature flags.** Likely no — those are upstream's
  experiment-flag system; touching them increases rebase pain. Source-level
  `omw_local` gates are sufficient.

## 8. Reversibility

All changes are `#[cfg(not(feature = "omw_local"))]` gates and `optional = true`
in workspace Cargo.toml. No source deletion. Building without `omw_local`
restores the upstream-stripped behavior. A future v0.3 PR can selectively
un-gate the agent panel and re-route it through `omw-server` without redoing
the demolition work.

## 9. Out of scope

- Editing `vendor/warp-fork/` (forbidden by CLAUDE.md §5; that work happens in
  the sibling `oh-my-warp/warp-fork` repo and is mirrored back when the v0.3
  fork-rebase pipeline is wired up).
- Wiring `omw-server` / `omw-agent` into the GUI — that is v0.3.
- Branding (renaming `warp-oss` → `omw`, palette swap, icon) — that is v0.3.
- Touching Warp's analytics / Sentry crash reporting — covered separately if
  needed; not part of "remove built-in AI."

## 10. Acceptance criteria

The work is done when:

1. `cargo build -p warp --bin warp-oss --features omw_local` succeeds with
   zero warnings introduced by this change.
2. `cargo build -p warp --bin warp-oss` (no `omw_local`) also succeeds, proving
   reversibility.
3. The static audit script reports zero `*.warp.dev` / `firebase.*` strings in
   the `omw_local` binary outside test fixtures.
4. Manual smoke test (`specs/test-plan.md` step): launch shows no signup wall,
   no AI CTAs in menus, no outbound traffic to Warp/firebase during a 60s
   normal-use exercise.
5. The agent panel renders the placeholder text instead of the upstream sign-in
   prompt.
