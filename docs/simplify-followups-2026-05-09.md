# /simplify follow-ups — 2026-05-09

Snapshot of open product decisions and deferred simplifications surfaced by the `/simplify` audit run on HEAD `9def1ac` (now extended through `3df3053`). The audit dispatched three parallel Opus 4.7 investigators (docs / code / wiring); their findings were aggregated and triaged into three landed stages plus this follow-up list.

This file is a working list. When an item is decided or executed, strike it through here or remove it. The audit reports themselves were transient and live only in the conversation transcript.

---

## What landed (for context)

| Stage | Commit | Summary |
|-------|--------|---------|
| 1 | [`292e9db`](#) | Drop dead Cargo deps (`futures`, `rand_core`, `tower` ×2, `tower-http`, `tracing`); delete unused Rust items (`serve_agent_loopback`, `Session` re-export, `ws::pty::ws_handler` stub); delete `apps/web-controller/src/types/shared.ts`; strip stale "Phase 0 placeholder" descriptions. |
| 2 | [`c305f27`](#) | Archive 8 stale docs to `docs/archive/`; fix dangling references (`omw-remote/src/lib.rs`, vendor doc-comments); drop `CONTRIBUTING.md` §Phase-0-caveat; link AGPL audit from `specs/fork-strategy.md` §3. |
| 3 | [`3df3053`](#) | Collapse three `locate_*` helpers into one; rename transcript-side `ApprovalDecision` → `ApprovalCardStatus` to retire the `as ProtocolApprovalDecision` / `as TranscriptApprovalDecision` shims in `omw_panel.rs`. |

---

## 1. Open product questions (decide, then execute)

These four were intentionally skipped — each requires a product call before any code change is safe.

### 1.1 `omw-policy` Rust crate fate

- **Where**: `crates/omw-policy/src/lib.rs` (358 LOC)
- **Smell**: The crate exists, is depended on by `omw-server` (via `Cargo.toml`), and is never `use`d at runtime. The live policy classifier is `apps/omw-agent/src/policy.ts`.
- **Decision needed**: Either (a) wire `omw-policy` into the `beforeToolCall` JSON-RPC round-trip (PRD §5.3 implies this is the v0.4 plan) or (b) delete the crate + its `omw-server` dep.
- **If (b)**: ~360 LOC removed + workspace member dropped + the `ApprovalMode` enum collapses (§3.4 below).
- **If (a)**: this is real work, not cleanup.

### 1.2 `/api/v1/audit/append` endpoint fate

- **Where**: `crates/omw-server/src/lib.rs::audit_router`, `crates/omw-server/src/handlers/audit.rs`
- **Smell**: Endpoint is built, tested, and never mounted in production. PRD §8.3 commits to a single-writer audit log; today nothing posts to it. The `omw_inproc_server::boot` pipeline does NOT compose `audit_router`.
- **Decision needed**: Mount it from `omw_inproc_server::boot` (the change is ~5 lines next to `agent_router`), feature-gate behind an `audit-endpoint` Cargo feature, or delete.
- **If mounted**: PRD §8.3 invariant becomes real instead of paper-only. Need a producer (kernel? vendor? CLI?) to wire up too.

### 1.3 `/internal/v1` PTY router fate

- **Where**: `crates/omw-server/src/lib.rs::router` + `handlers/{sessions,input,ws_pty}.rs` (~430 LOC)
- **Smell**: Public router with zero runtime callers. Vendor consumes `SessionRegistry` via `register_external` (a struct method); `omw-remote` builds its own `make_router`. Tests are the only consumers.
- **Decision needed**: Is `/internal/v1` a future surface for non-`omw-remote` clients (CLI tools, MCP gateway), or genuinely dead?
  - If future-surface: add even one production caller so the contract stops bit-rotting in isolation.
  - If dead: delete `pub fn router`, the three `handlers/*` files, and their tests. `SessionRegistry` itself stays — that's the live API.

### 1.4 Production `/tmp/omw-debug.log` writes

- **Where**: `crates/omw-server/src/lib.rs::omw_debug` + ~11 `eprintln!("[omw-debug] …")` sites in `crates/omw-remote/src/server.rs` and friends.
- **Smell**: Production builds (v0.0.3 release) write debug log lines to `/tmp/omw-debug.log` unconditionally. ~9836 lines on the audit machine. Was added in `0c50c26 inline-agent: end-to-end fixes for # prompt path` to debug the GUI's `# hi` 502 bug — that bug shipped fixed in v0.0.3.
- **Decision needed**: Was this intentionally left on for v0.0.3, or did it slip through? Almost certainly the latter.
- **Action when answered**: Gate behind `OMW_DEBUG_LOG=1` env var or `cfg(debug_assertions)`. If the user wants observability in production, document `/tmp/omw-debug.log` in install instructions and rotate it.

---

## 2. Drift in canonical docs (editorial, not blocking)

### 2.1 PRD.md §13 phased-roadmap

- Still frames `v0.4-thin` / `v0.4-thin-polish` / `v0.4-cleanup` as the forward roadmap.
- TODO.md §137-160 documents that v0.0.3 already shipped most of v0.4-cleanup (inline-agent + audit + Settings → Agent UX).
- **Risk**: contributors planning against PRD §13 will plan against a roadmap that no longer matches TODO.md.
- **Action**: rewrite PRD §13 to match the `v0.0.x` preview cadence currently in TODO.md. Substantive editorial work; defer until either (a) v0.4-thin-polish is formally retired or (b) the next semantic-version bump.

---

## 3. Deferred code simplifications (judgment calls)

Each of these would help, but none was zero-risk enough to land in the three Stage commits. Listed roughly by impact.

### 3.1 Provider-name string vocabulary appears in 5 places

- **Where**: `crates/omw-cli/src/lib.rs:181-194` (`ProviderKindArg::as_kebab`), `crates/omw-config/src/schema.rs:243-247` (`ProviderConfig::kind_str`), `crates/omw-cli/src/db.rs:153-157`, `apps/omw-agent/src/session.ts:38`, `apps/omw-agent/src/keychain.ts:205`.
- **Smell**: The 4-string vocabulary `"openai" / "anthropic" / "openai-compatible" / "ollama"` is hand-mapped 5×. Both `kind_str()` and `as_kebab()` have identical body shapes.
- **Tradeoffs**: Two unification routes — (a) `From<&ProviderConfig> for ProviderKindArg` (Rust side) + a hand-mirrored TS string-literal type, kept in sync via CI cross-check; or (b) generate a shared `providers.toml` → both Rust enums and TS string-literal types, single source of truth at the cost of a build step.
- **Why deferred**: pure judgment call between (a) and (b); needs user preference.

### 3.2 `omw-remote::lib.rs` `pub use` block trim

- **Where**: `crates/omw-remote/src/lib.rs:26-38`.
- **Smell**: Wiring auditor flagged 17 dead re-exports; my brace-import-aware re-grep cut that to 6. Truly external-zero items: `DeviceId, CapParseError, schema_version, PairRedeemResponse, PairTokenHash, PairParseError`.
- **Tradeoffs**: Demoting them to module-private cleans IDE autocomplete + docs.rs surface, but the win is small and a future external caller (e.g. an MCP client) would have to re-export.
- **Why deferred**: low-impact; do during a future "API surface freeze" pass before v0.1 if at all.

### 3.3 `OmwAgentState::remove_pane_session` has no caller

- **Where**: `vendor/warp-stripped/app/src/ai_assistant/omw_agent_state.rs:507`.
- **Smell**: `pub fn` with no external caller; only `clear_all_pane_sessions` is invoked from production paths.
- **Tradeoffs**: Either delete or demote to `pub(crate)` until a real lifecycle case shows up. File-level `#![allow(dead_code)]` covers it for now.
- **Why deferred**: pre-emptive lifecycle hook; the right call is "delete when Phase 3c lands and we know what the lifecycle actually needs", not "delete blindly".

### 3.4 Two `ApprovalMode` enums (Rust)

- **Where**: `crates/omw-config/src/schema.rs:278-287` + `crates/omw-policy/src/lib.rs:62-72`.
- **Smell**: Same enum (`ReadOnly / AskBeforeWrite / Trusted`), same serde rename, same default — duplicated in two crates. Comment in schema.rs:274 already says "Mirrors `omw_policy::ApprovalMode`".
- **Tradeoffs**: Either re-export `omw_policy::ApprovalMode` from `omw-config`, or fold one direction. Simpler path is omw-config re-exports omw-policy's, but that adds a hard dep direction.
- **Why deferred**: blocked by §1.1. If `omw-policy` gets deleted, this collapses for free.

### 3.5 `inline_prompt_repro.rs` test file naming

- **Where**: `crates/omw-server/tests/inline_prompt_repro.rs`.
- **Smell**: Doc comment says "Reproduce the GUI's `# hi` 502 bug." That bug shipped fixed in v0.0.3 (commit `0c50c26`). The file is now a regression net, not a repro.
- **Tradeoffs**: Rename to `inline_prompt_regression.rs` and prune the "what does the GUI do that the simple test doesn't" prose. Keep the tests themselves — they're a real regression net.
- **Why deferred**: cosmetic; doesn't affect the test gate.

### 3.6 `ws_concurrent.rs` test is `#[ignore]`'d

- **Where**: `crates/omw-server/tests/ws_concurrent.rs`.
- **Smell**: Intentionally skipped pending v0.4-thin subscription model.
- **Tradeoffs**: Either delete (then re-add when v0.5 starts) or convert to `#[cfg(feature = "ws-concurrent")]` so it compiles but doesn't run.
- **Why deferred**: low-impact; do when v0.5 starts.

### 3.7 Stale "v0.4-thin tmux-style attach plan" caveat

- **Where**: `docs/archive/v0.4-thin-tmux-style-attach-plan.md:389-392`.
- **Smell**: The doc was archived. It carries a tactical note: `cargo test -p warp --features omw_local --lib` fails with ~157 unrelated `settings_view::mod_test.rs` errors from the upstream merge.
- **Tradeoffs**: Migrate to a `TODO.md` known-issues line or verify it was fixed by a recent upstream sync.
- **Why deferred**: requires actually running the test command + auditing the results; out of scope for a doc-archive pass.

---

## 4. Things investigated and confirmed load-bearing (skip these in future audits)

These looked like cruft but were verified to be live. Saving the next auditor the round-trip.

- `OmwAgentState` `#![allow(dead_code)]` — Phase 3b ships compiled-but-unused; Phase 3c flips it on. Don't remove the allow.
- `omw-server::ExternalSessionSpec` / `register_external` — production API used by `vendor/warp-stripped/app/src/omw/pane_share.rs`.
- `omw-remote::Capability::AgentRead/AgentWrite/AuditRead` — minted in `pair_redeem` for forward-compat. Removing would invalidate already-issued capability tokens.
- `omw-pty/src/lib.rs` (607 LOC) — three threads each necessary (writer/watcher/reader + drop-safety). Not a "cut" target.
- `omw-keychain-helper` lib + main split — required for in-process tests verifying Threat-Model invariant I-1 ("secret only on stdout").
- `commands/agent.rs` + `commands/agent_runner.rs` — REPL loop vs per-turn spawner. Right shape; don't merge.
- 42× `async fn` without local `await` — axum requires `async fn` for its `Handler` trait. Not a smell.
- `AgentCrashed` event variant — produced by `omw-server::agent::process.rs:197` when the kernel dies.
- `ApprovalDecision::Cancel` (protocol) — produced by `apps/omw-agent/src/policy-hook.ts:35` on modal dismiss.
- `ExecCommand / CommandData / CommandExit` protocol variants — TS bash adapter ↔ Rust `BashBroker` is a live wire path; vendor's `omw_transcript::apply_event` legitimately drops them because they're kernel-bash control events, not transcript-visible.
- `omw-cli/Cargo.toml` axum dep — used by `commands/remote.rs:195` to call `axum::serve` against `omw_remote::make_router`. Could be moved into `omw-remote` (it already has `pub async fn serve`!) but that's a wire-format change worth its own design pass.
