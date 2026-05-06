# Inline-Agent Stack — Progress + Remaining Work

- **Date:** 2026-05-06
- **Branch state at write time:** `main`, 10 commits ahead of `origin/main`.
- **Approved plan (source of truth):** `/Users/andrewwayne/.claude/plans/sharded-tickling-sutton.md`
- **Original design report:** [`docs/inline-agent-command-execution-report.md`](./inline-agent-command-execution-report.md)
- **Owner notes:** this document supersedes the plan file as the *current* state record. The plan file remains the contract; this file maps the contract onto today's commit chain and lists the four phases still owed.

## TL;DR

11 commits land Phases 0 → 4c3 of the inline-agent stack. The kernel works, the omw-server bridge works, the policy + audit libraries work, the agent panel state machine in warp-stripped compiles, and the agent's `beforeToolCall` hook is wired through to the approval round-trip. **Four phases remain — all involve either bidirectional protocol work (5a) or warp-stripped UI surgery (5b, 3c, 4c4) that needs manual smoke verification.** The substrate is solid; the next session should resume with Phase 5a server-side, then a manual smoke pass with a qlaybot-config-derived API key, then the UI work.

## Commits landed (Phase → SHA → one-line summary)

```text
ae97710  Phase 4c3   apps/omw-agent: beforeToolCall hook + approval flow
a616b3f  Phase 4c2   apps/omw-agent: TS policy classifier + 3 codex-review fixes
0e5bbbc  Phase 4c1   crates/omw-server: POST /api/v1/audit/append + audit_router
208eb8c  Phase 4b    crates/omw-audit: append-only hash-chained audit writer
db2fdda  Phase 4a    crates/omw-policy: bash-command classifier
30bbbb9  Phase 3b    warp-stripped: OmwAgentState (WS client + runtime)
f19b0f3  Phase 3a    warp-stripped: omw_protocol + omw_transcript modules
5f8041c  Phase 2     crates/omw-server: agent endpoints + AgentProcess bridge
68612a8  Phase 1     apps/omw-agent: pi-agent kernel + stdio JSON-RPC server
889acf8  Phase 0     apps/omw-agent: vendor pi-agent-core + pin pi-ai 0.70.6
```

## Test gates green at HEAD

| Crate / package | Tests | Notes |
|---|---|---|
| `omw-policy` | 14 passing | Pure unit tests; serde round-trip + every classification path. |
| `omw-audit` | 6 passing | 100-entry chain build+verify, tamper detection, cross-day rollover. |
| `omw-server` | 38 passing, 1 ignored | Includes `agent_session.rs` (3 integration), `audit_endpoint.rs` (2), `agent::process::tests` (2), all pre-existing PTY-registry tests. |
| `apps/omw-agent` | 74 passing, 3 skipped | Skipped are `keychain.integration.test.ts` (helper-binary gated). |
| `vendor/warp-stripped` lib build | clean under `omw_local` | `MACOSX_DEPLOYMENT_TARGET=10.14 cargo build -p warp --bin warp-oss --features omw_local`. |
| `vendor/warp-stripped` **lib test** | **broken pre-existing** | ~157 unrelated `settings_view::mod_test.rs` errors; documented at [`docs/v0.4-thin-tmux-style-attach-plan.md:390`](./v0.4-thin-tmux-style-attach-plan.md). My new test files (`omw_protocol_tests.rs`, `omw_transcript_tests.rs`) are well-formed and will run once that breakage is repaired. |

## Architectural decisions pinned (D1–D8)

These came out of the brainstorming + plan agent + user choices and should not drift in subsequent phases without an explicit decision-record update.

- **D1 — agent transport.** Line-delimited JSON-RPC 2.0 over stdio. One Node process per omw-server, multiplexes sessions by `sessionId`.
- **D2 — GUI ↔ omw-server.** A separate long-lived WebSocket the GUI dials at `WS /ws/v1/agent/:sessionId`. JSON frames only — never raw PTY bytes (those are on `/internal/v1/sessions/:id/pty`).
- **D3 — panel rewire.** Parallel `OmwAgentTranscript` model. Do not repurpose the upstream `Transcript` / `Requests` (tied to `ServerApi`/`AIClient`/`RequestStatus`).
- **D4 — audit hash chain.** SHA-256 over `prev_hash_hex || canonical_json(entry_without_hash)`. Per-day file. Cross-day chain seeded from previous file's last `hash`. Genesis = 64 zero hex.
- **D5 — approval blocking.** Per-call `Promise` keyed by `approvalId`; resolved by `approval/decide` request; signal-driven `cancel` if loop aborts mid-wait. Implemented in `apps/omw-agent/src/policy-hook.ts`.
- **D6 — agent runtime.** `omw-server` spawns it via `OMW_AGENT_BIN` env var, falls back to `omw-agent` on `$PATH`. No bundling.
- **D7 — `OmwAgentState` runtime.** Dedicated tokio runtime in `omw-agent-rt` thread, independent of `OmwRemoteState`'s runtime. Lazy on first `start()`.
- **D8 — audit transcript locality.** Assistant final messages written to local-only `~/.local/share/omw/audit/*.jsonl`. PRD §11.2 "no telemetry" is outbound-only; local audit is desired by FR-6.

## Phase status

### Phase 0 — Vendor pi-agent-core (DONE — 889acf8)

`apps/omw-agent/vendor/pi-agent-core/` (5 files, MIT). Pi-ai stays as `@mariozechner/pi-ai@0.70.6` npm dep — **deviation from the plan as written, deliberately**: pi-ai's 924K source has zero kernel-modification need on our side, and vendoring source wouldn't reduce its `@anthropic-ai/sdk` / `openai` / `@aws-sdk` runtime deps. `bash.ts` is *not* vendored — it's deeply tied to pi-coding-agent's TUI internals; we'll write our own thin tool against pi-agent's `BashOperations` interface in Phase 5.

Refresh ritual: `bash apps/omw-agent/scripts/refresh-pi-mono.sh` (pin-guarded; refuses if `vendor/pi-mono` HEAD diverges from the documented commit `fe1381389de87d2620af5d7e46d00f76f4e65274`).

### Phase 1 — Agent stdio JSON-RPC server (DONE — 68612a8)

`node apps/omw-agent/bin/omw-agent.mjs --serve-stdio` runs pi-agent's `agentLoop` over line-delimited JSON-RPC 2.0. Methods: `session/{create,prompt,cancel}`, `approval/decide`. Notifications: `assistant/delta`, `tool/call_started`, `tool/call_finished`, `turn/finished`, `approval/request`, `error`.

Provider support: `openai` and `anthropic` via pi-ai's `getModel` registry; `openai-compatible` and `ollama` via hand-built `Model<"openai-completions">`. Cancel signal propagates through `AgentLoopConfig.signal` → pi-ai stream → OpenAI SDK fetch teardown.

### Phase 2 — omw-server agent bridge (DONE — 5f8041c)

`AgentProcess` owns the Node child, parses stdout JSON-RPC frames, routes responses to `send_method` waiters and notifications to per-`sessionId` broadcast busses. `crashed`-on-EOF synthesizes an `agent/crashed` notification for every active session bus.

Routes: `POST /api/v1/agent/sessions` (returns `{ sessionId }`), `WS /ws/v1/agent/:id` (forwards kernel notifications to client; `prompt`/`cancel` client frames translate to `session/prompt` / `session/cancel`). Compose with `router(registry)` via `axum::Router::merge`.

Test fixture: `tests/fixtures/mock-omw-agent.mjs` is a Node script speaking the same JSON-RPC surface; lets the omw-server suite verify the bridge without dragging pi-ai into `cargo test`.

### Phase 3a — Protocol + transcript (DONE — f19b0f3, compiled-but-dormant)

`vendor/warp-stripped/app/src/ai_assistant/{omw_protocol,omw_transcript}.rs` under `#[cfg(feature = "omw_local")]`.

`OmwAgentEventDown` mirrors the JSON-RPC notification frames 1:1 via `#[serde(tag = "method", content = "params")]` — a raw kernel frame round-trips through `serde_json::from_str` directly. Phase 5 bash-broker variants (`ExecCommand`, `CommandData`, `CommandExit`) and Phase 4 `ApprovalRequest` are landed dormant.

`OmwAgentTranscriptModel::apply_event(&OmwAgentEventDown)` is the single mutation surface. `push_user` reserves a streaming Assistant slot. Tool-call cards flip status on `tool/call_finished`. Approval cards flip via `update_approval`.

Tests: 9 protocol round-trip + 11 transcript apply-event sequences, well-formed but currently uncrunchable due to the lib test target breakage.

### Phase 3b — OmwAgentState (DONE — 30bbbb9, compiled-but-unwired)

`vendor/warp-stripped/app/src/ai_assistant/omw_agent_state.rs` — singleton modeled on `omw/remote_state.rs`. Owns: dedicated tokio runtime + `omw-agent-rt` thread, HTTP client (reqwest), control WS (tokio-tungstenite, gated under `omw_local`), `tokio::sync::watch::Sender<OmwAgentStatus>`, broadcast bus for inbound events.

Public API: `shared()`, `status()`, `status_rx()`, `subscribe_events()`, `start(params)`, `stop()`, `send_prompt(text)`, `cancel()`. The codex-review fix at `a616b3f` made every status transition route through `set_status` (not `status_tx.send_replace`) so the cached `inner.status` stays in sync with the watch channel.

`#[allow(dead_code)]` at module level until Phase 3c calls into it.

### Phase 4a — omw-policy (DONE — db2fdda)

`Decision { Allow, Ask, Deny }` × `ApprovalMode { ReadOnly, AskBeforeWrite, Trusted }`. `classify(cmd, cfg) -> Decision` with conservative metacharacter scan (`>`, `|`, `;`, `` ` ``, `&&`, `||`, `$()`). Built-in read-only set covers `pwd`, `ls`, `rg`, `cat`, `head`, `tail`, `wc`, `git status`/`diff`/`log`/`show`/`branch`/`remote`/`config`/`blame`. Per-config `allow` / `deny` lists override built-ins; deny wins on conflict (fail closed).

### Phase 4b — omw-audit (DONE — 208eb8c)

`AuditWriter::open(audit_dir)` opens today's `~/.local/share/omw/audit/YYYY-MM-DD.jsonl`. `append(kind, session_id, fields_json) -> hash_hex`. `verify_chain(path, seed_prev_hash) -> Result<head_hash>`. Cross-day rollover seeds from yesterday's last `hash` so the chain is contiguous. Day rollover is **caller-driven** (`reopen()`) — not auto on append — so test pinning works.

Out of scope: redaction, retention, encryption-at-rest. Per PRD §11.4 the last is delegated to disk-level encryption.

### Phase 4c1 — Audit endpoint (DONE — 0e5bbbc)

`POST /api/v1/audit/append { kind, session_id, fields }` → `201 { hash }`. Single `AuditWriter` behind `tokio::sync::Mutex<AuditWriter>` injected via `audit_router(audit)`. PRD §8.3 — single-writer invariant.

### Phase 4c2 — TS policy + codex fixes (DONE — a616b3f)

`apps/omw-agent/src/policy.ts` — TS port of `omw-policy::classify`, mirror tests pinned 1:1 against `crates/omw-policy/tests/classify.rs`.

**Three codex-review fixes folded into the same commit** (these would have broken `omw-agent --serve-stdio` on first run, so they're load-bearing not cosmetic):

1. **[P1]** `@pi-agent-core` path alias is compile-only; Node can't resolve it at runtime. Switched to relative imports `../vendor/pi-agent-core/index.js` in `session.ts`, `serve.ts`, `test/vendor-import.test.ts`.
2. **[P2]** `serve.ts` was acknowledging `session/prompt` before checking `Session.isStreaming`, so a concurrent prompt got `{ok:true}` then a synthetic `error` + `turn/finished`. Now checks synchronously before replying.
3. **[P2]** `OmwAgentState::run_session` was updating `status_tx` directly, leaving `inner.status` stuck on `Starting` after the WS reached `Connected`. Routed every transition through `set_status` (with a `Weak<Self>` upgrade fallback).

### Phase 4c3 — beforeToolCall + approval flow (DONE — ae97710)

`apps/omw-agent/src/policy-hook.ts` exports `makeBeforeToolCallHook(deps)`. The hook:

- `allow` → return `undefined` (pass through).
- `deny` → return `{ block: true, reason: "policy: command denied (<mode>)" }`.
- `ask` → emit `approval/request { approvalId, toolCall }` notification, allocate Promise with resolver in `pendingApprovals`, await; resolves on `approve`/`reject`/`cancel`. AbortSignal mid-wait flips to `cancel`.

`Session` constructor signature changed to `(spec, deps: { getApiKey, notifyApprovalRequest })`. `Session.applyApprovalDecision(approvalId, decision)` is the entry point `serve.ts::handleApprovalDecide` calls when the GUI replies. `Session.cancel()` now also resolves all pending approvals as `cancel` so hooks unblock cleanly.

The hook never fires in production today because no AgentTool is registered. Phase 5a wires the bash AgentTool.

---

## What's left

Four phases. Detailed below. Recommended order: **5a → manual smoke → 5b → 3c → 4c4** so each layer is verified before the next builds on it.

### Phase 5a — `WarpSessionBashOperations` + `bash_broker` (TODO)

**Estimated scope:** ~600 LOC TS + Rust + tests. **One open protocol decision** below.

**Goal.** Pi-agent's bash tool runs a command, the bash adapter sends the command upstream over JSON-RPC, omw-server's broker forwards it to the active GUI control WS, the GUI executes the command in its visible Warp pane, the broker streams the pane's PTY output back to the bash adapter as `bash/data`/`bash/finished`, and the agent loop continues with the captured output as the tool result.

**Open decision before writing code.** The bash tool needs *bidirectional* communication: it sends `bash/exec` upstream and listens for `bash/data` + `bash/finished` downstream. Two patterns:

- **Pattern A — Bidirectional JSON-RPC.** Bash adapter sends `bash/exec` *request* with id; broker dispatches to GUI WS, then sends a single response back when the command exits. Streaming `bash/data` events are notifications routed by `commandId`. Pro: matches request/response semantics for the lifecycle terminator. Con: agent stdio reader (`crates/omw-server/src/agent/process.rs::route_frame`) currently treats every framed-id-message as a *response* and matches against `pending`; it has no notion of "incoming request from agent." Adding that requires a new request-handler dispatch table + outbound-id allocator on the agent side.
- **Pattern B — Correlated notifications.** All bash traffic is notifications, never requests. Bash adapter sends `bash/exec` notification with `commandId`. Broker forwards to GUI as `ExecCommand`. GUI streams `CommandData`/`CommandExit` via the WS. Broker forwards each as `bash/data` / `bash/finished` notifications down to the agent stdio. Bash adapter's `serve.ts` keeps a `Map<commandId, Subscriber>`; new method dispatch in the request loop just for these. Pro: smaller protocol diff (no incoming-request handler in `process.rs`). Con: lifecycle is "wait for `bash/finished` notification keyed by `commandId`" rather than awaiting a single response — slightly more state.

**Recommendation: Pattern B.** Smaller diff against the existing reader; avoids reworking `route_frame`. The dormant variants in `omw_protocol.rs` already match this shape.

**Files to create**

- `apps/omw-agent/src/warp-session-bash.ts` — `createWarpSessionBashOperations({ rpc, terminalSessionId, agentSessionId, toolCallId })`. `BashOperations.exec(command, cwd, { onData, signal, timeout })` allocates `commandId`, registers a per-id subscriber on the existing `serve.ts` server, emits `bash/exec` notification, awaits the final `bash/finished`, returns `{ exitCode }`. 30 s default timeout → `bash/cancel` and resolve with `{ exitCode: null, snapshot: true }` per D8 in the plan.
- `apps/omw-agent/test/warp-session-bash.test.ts` — vitest with mocked stdio: assert the notification sequence and that `onData` is called for each `bash/data` chunk.
- `crates/omw-server/src/agent/bash_broker.rs` — receives `bash/exec` notifications from agent stdout (route in `process.rs::route_frame` extends to dispatch `bash/*` methods to a broker). Looks up the GUI WS for the active `terminalSessionId`. Forwards as `OmwAgentEventDown::ExecCommand { commandId, command, cwd }`. Receives `OmwAgentEventUp::CommandData` / `CommandExit` from the GUI WS, forwards to agent stdin as `bash/data` / `bash/finished` notifications.
- `crates/omw-server/tests/agent_bash.rs` — mock GUI WS + mock agent stdout: round-trip `bash/exec` → `ExecCommand` → simulated `CommandData` x N → `CommandExit` → `bash/finished`. Plus the timeout/snapshot path.

**Files to edit**

- `apps/omw-agent/src/session.ts` — register the bash AgentTool when the loop is constructed: `tools: [createBashTool(cwd, { operations: createWarpSessionBashOperations({ rpc, terminalSessionId, agentSessionId, toolCallId: ... }) })]`. The `toolCallId` part is per-call — the adapter is allocated fresh per invocation. `createBashTool` would be our minimal AgentTool implementation since we're not vendoring `pi-coding-agent/src/core/tools/bash.ts` (see Phase 0 deviation).
- `apps/omw-agent/src/serve.ts` — add inbound handlers for `bash/data` and `bash/finished` notification frames; route to per-`commandId` subscribers maintained by an outbound side helper (separate from `pendingApprovals`).
- `crates/omw-server/src/agent/process.rs::route_frame` — extend the notification path to dispatch `bash/exec` (and any future kernel-emitted bash methods) to the bash broker.
- `crates/omw-server/src/agent/mod.rs` — `pub mod bash_broker;` and re-exports as needed.
- `vendor/warp-stripped/app/src/ai_assistant/omw_protocol.rs` — un-`#[allow(dead_code)]` (or whatever marks them dormant) the `ExecCommand`/`CommandData`/`CommandExit` variants.

**Files to inspect first**

- `vendor/pi-mono/packages/coding-agent/src/core/tools/bash.ts:152-280` — the `BashOperations` extension surface. We don't vendor this file, but our adapter must match the interface shape.
- `vendor/pi-mono/packages/agent/src/types.ts:308-331` — `AgentTool` interface. Our minimal `createBashTool` returns one of these.
- `crates/omw-server/src/agent/process.rs::route_frame` — to plan the dispatch extension.
- `crates/omw-server/src/handlers/agent.rs` — to add `OmwAgentEventUp::CommandData`/`CommandExit` parsing.

**Risk**

- Block-end detection without OSC 133. Per D8 the v1 strategy is 30s timeout → snapshot, exit code unknown, audit `command_snapshot` event. Manual smoke must include both an OSC-133-emitting shell (`zsh` with Warp's bundled hooks, default in Warp panes) and a stock `bash` to confirm both behaviours.
- One agent process for many sessions is the existing assumption. Bash subscribers are keyed by `commandId` (not `sessionId`) so two sessions running bash concurrently don't collide.

**Test gate**

- `npm test -- warp-session-bash` green.
- `cargo test -p omw-server agent_bash` green.
- Full omw-server + apps/omw-agent suites still green.

**Out of scope for 5a**

The GUI side of the broker (Phase 5b). 5a tests must mock the GUI WS.

### Phase 5b — GUI command broker + `register_active_terminal` (TODO, UI surgery)

**Goal.** When `OmwAgentEventDown::ExecCommand` arrives on the GUI control WS, the active warp pane writes the command via the same path Warp's upstream agent uses — `Event::ExecuteCommand` → `terminal_manager_util::write_command` → `PtyController::write_command`. Output is captured by tapping the pane's `pty_reads_tx` (mirror `vendor/warp-stripped/app/src/omw/pane_share.rs:130-220`) and forwarded back as `OmwAgentEventUp::CommandData`. Block end via OSC 133 prompt markers; on timeout, snapshot per D8.

**Files to create**

- `vendor/warp-stripped/app/src/ai_assistant/omw_command_broker.rs` — handler module. On `ExecCommand`: emit `Event::ExecuteCommand` into the registered `event_loop_tx`; subscribe to `pty_reads_tx`; on each chunk send `CommandData`; on OSC 133 detection (or 30 s timeout) send `CommandExit`.

**Files to edit**

- `vendor/warp-stripped/app/src/ai_assistant/omw_agent_state.rs` — add `register_active_terminal(view_id, event_loop_tx, pty_reads_tx, current_size)`. Called by `TerminalView` on focus change. Stores in a `Mutex<Option<...>>` so the broker can find the target pane without a global pane-stack downcast.
- `vendor/warp-stripped/app/src/terminal/view.rs` — small focus-change hook that calls `OmwAgentState::shared().register_active_terminal(...)`. Reuses the same plumbing pattern as v0.4-thin-polish Gap 1 Part C (see TODO.md:117) but only for the *currently focused* pane, which is reachable from `view.rs` without the `Box<dyn TerminalManager>` downcast.

**Why it's risky autonomously**

No tests can verify the UI integration. `cargo build` confirms it compiles, but "the agent runs `ls` and the visible pane shows the output" is a manual-smoke claim. Concurrent user input while the agent owns a command is also an open-by-design issue (D5-adjacent — "trust the user" in v1). Can't be smoke-tested without the user actually typing into the pane.

### Phase 3c — `panel.rs` flip (TODO, UI surgery)

**Goal.** Replace the `is_omw_placeholder` short-circuit in `vendor/warp-stripped/app/src/ai_assistant/panel.rs` with a real render that consumes `OmwAgentTranscriptModel`. `new_omw_panel` (currently `new_omw_placeholder`) calls `OmwAgentState::shared().start(...)` with a session params struct derived from omw config.

**Files to create**

- `vendor/warp-stripped/app/src/ai_assistant/omw_panel.rs` — `render_omw_agent_panel(panel, app)` function returning the panel view tree. Editor for prompt input, scrolling transcript, status footer.

**Files to edit**

- `vendor/warp-stripped/app/src/ai_assistant/panel.rs:135` — add `omw_agent: Option<ModelHandle<OmwAgentTranscriptModel>>` field under `#[cfg(feature = "omw_local")]`.
- `vendor/warp-stripped/app/src/ai_assistant/panel.rs:271` — `new_omw_placeholder` becomes `new_omw_panel`: allocate the omw model + view, call `OmwAgentState::shared().start(...)`, subscribe events via `subscribe_events()`, route through an async-channel bridge into the model's `apply_event`.
- `vendor/warp-stripped/app/src/ai_assistant/panel.rs:1101` — `is_omw_placeholder` short-circuit in `on_focus` becomes focus routing into the omw editor.
- `vendor/warp-stripped/app/src/ai_assistant/panel.rs:1122` — placeholder render block becomes `render_omw_agent_panel(self, app)`. The static `OMW_PLACEHOLDER_TEXT` line goes away.

**Open question for the next session.** Reading provider config for the agent: today `OmwAgentSessionParams` is constructed by the caller. The panel needs a source of truth — likely `~/.config/omw/config.toml` via `omw-config`. There's an open question on whether to pass the qlaybot-style provider list straight through or add a thin omw-config layer first. **Recommend: thin omw-config layer**, because the qlaybot schema embeds full `models: []` arrays we don't need.

**Files to inspect first**

- `vendor/warp-stripped/app/src/omw/remote_state.rs:1-300` — runtime ownership + the `subscribe_status_stream` -> `async_channel` bridge pattern. Mirror this for `subscribe_events_stream`.
- `vendor/warp-stripped/app/src/ai_assistant/panel.rs:120-280` — panel construction + the existing focus + tick infrastructure.
- `vendor/warp-stripped/app/src/ai_assistant/utils.rs` — `markdown_segments_from_text` for assistant message rendering (reuse).

### Phase 4c4 — GUI approval cards (TODO, UI surgery)

**Goal.** Render `OmwAgentMessage::Approval` rows in the panel transcript with **Approve** / **Reject** buttons. Click sends `OmwAgentEventUp::ApprovalDecision { approvalId, decision }` over the WS; `OmwAgentTranscriptModel::update_approval` flips the row's status.

**Files to edit**

- `vendor/warp-stripped/app/src/ai_assistant/omw_transcript.rs` — extend the model's view rendering to emit clickable button elements. Click handler dispatches via `OmwAgentState::shared().send_approval_decision(approval_id, decision)` — note this method needs to be added to `OmwAgentState`.
- `vendor/warp-stripped/app/src/ai_assistant/omw_agent_state.rs` — add `send_approval_decision(approval_id, decision)` method that writes `OmwAgentEventUp::ApprovalDecision` to the outbound mpsc.

**Test gate**

- Manual smoke: `Trusted` mode bypasses the card path; `AskBeforeWrite` (default) with `rm /tmp/foo` produces a card; click Reject → tool error in card → assistant continuation. Audit chain validates.

---

## Codex review baseline

The codex review at `a616b3f` covered Phases 0 → 4c1. Three findings were folded in. Future codex reviews should re-run against the **5a** commit boundary (post-bash-broker) since that's the next major surface change. Suggested invocation:

```sh
codex review --base origin/main --title "Inline-agent stack post-Phase-5a"
```

(Plain prompts to `codex review` are rejected when `--base` is set; pass instructions via `~/.codex/config.toml` or an interactive review session if custom focus is needed.)

## Known issues NOT my work, NOT to fix here

1. **`vendor/warp-stripped` lib test target** — ~157 unrelated `settings_view::mod_test.rs` errors from the upstream cloud-strip merge. Documented at `docs/v0.4-thin-tmux-style-attach-plan.md:390`. My new `omw_protocol_tests.rs` and `omw_transcript_tests.rs` are well-formed and will run once that breakage is repaired.
2. **`damaged/` directory in working tree** — pre-existing untracked, unrelated to the inline-agent stack. From the v0.0.2 release work (commit `b23917e`).
3. **`crates/omw-remote/tests/ws_connect_token.rs::expired_ts_in_ct_rejects_401` and `ws_pty_session.rs::ts_skew_inbound_rejects`** — pre-existing red per [TODO.md](../TODO.md) v0.0.2 follow-ups.

## How to resume

### End-to-end smoke test of what's already landed

The qlaybot config at `~/.qlaybot/config/model.json` carries four providers; the `custom-openai` entry maps cleanly to our `openai-compatible` provider kind. Smoke recipe (illustrative — adjust paths and the helper stub):

1. Build: `cd apps/omw-agent && npm run build`.
2. Stub a keychain helper script that returns the qlaybot key for a `key_ref` (e.g. `omw/test`):
   ```sh
   cat > /tmp/omw-keychain-stub.sh <<'EOF'
   #!/usr/bin/env bash
   if [[ "$1" == "get" && "$2" == "omw/test" ]]; then
     # Read from qlaybot config; do NOT log.
     python3 -c 'import json,sys; print(json.load(open("/Users/andrewwayne/.qlaybot/config/model.json"))["providers"]["custom-openai"]["apiKey"], end="")'
     exit 0
   fi
   exit 1
   EOF
   chmod +x /tmp/omw-keychain-stub.sh
   ```
3. Drive `omw-agent --serve-stdio` against it:
   ```sh
   OMW_KEYCHAIN_HELPER=/tmp/omw-keychain-stub.sh \
     node apps/omw-agent/bin/omw-agent.mjs --serve-stdio
   ```
   Then send `session/create` with `providerConfig.kind: "openai-compatible"`, `key_ref: "omw/test"`, `base_url: "https://bench.physcai.com/openai/v1"`, `model: "gpt-5.5"`. Then `session/prompt`. Expect `assistant/delta` notifications.

This validates Phases 0 + 1 against a real provider. Phase 2 (omw-server bridge) can be smoke-tested with a similar recipe replacing stdin with `wscat` against `WS /ws/v1/agent/:id` after `POST /api/v1/agent/sessions`.

### Continuing implementation

Recommended next-session order:

1. **Phase 5a server-side** (testable Rust + TS).
2. **Manual smoke pass** with the stubbed keychain + qlaybot key. Verify Phases 0 → 5a end-to-end (the bash adapter still has no GUI to talk to, so 5a's tests must use the mock-GUI fixture).
3. **Phase 5b** (warp-stripped GUI command broker + `register_active_terminal`).
4. **Phase 3c** (`panel.rs` flip).
5. **Phase 4c4** (approval cards).
6. **Final manual smoke**: launch `warp-oss --features omw_local` against a running omw-server with a real key. Validate end-to-end: prompt → assistant text → approval card → bash command runs in pane → output streams into card → audit chain verifies.

### TODO.md alignment

When the user is ready to ship, update `TODO.md` v0.4-cleanup:
- Mark `Wire stripped client's agent panel to omw-server → omw-agent` complete (Phase 3c).
- Mark `WarpSessionBashOperations adapter in apps/omw-agent` complete (Phase 5a).
- Note that v0.2 `omw-policy` library and `omw-audit` library are partially complete (4a/4b landed; Activity view + redaction rule engine still deferred).
- Remaining v0.4-cleanup items (Web Controller agent view, approvals tray, diff view, settings page; Audit Activity view in stripped client; ACP wrapper) are explicitly out of this stack's scope per the plan.

---

## Decision log additions during the run

| Date | Decision | Rationale |
|---|---|---|
| 2026-05-06 | Vendor only `pi-agent-core/`, leave `pi-ai` as npm dep, write our own bash AgentTool. | pi-ai's source vendoring wouldn't reduce its heavy SDK runtime deps; bash.ts is too coupled to TUI internals. |
| 2026-05-06 | Phase ordering: policy + audit + approval UI **before** the bash tool. | User-locked; "no silent destructive actions" invariant requires the gate to land before the gun. |
| 2026-05-06 | Phase 3 split into 3a (data) + 3b (state machine) + deferred 3c (panel flip). | Diff-size discipline; risk-isolated per CLAUDE.md §3 surgical-changes. |
| 2026-05-06 | omw-audit `append` does not auto-roll at the day boundary. | Pinned-date tests would silently switch files; rollover is a higher-level concern. |
| 2026-05-06 | Phase 5 protocol: recommend Pattern B (correlated notifications) over bidirectional JSON-RPC. | Smaller diff; agent process reader doesn't need a request-handler dispatch table. **Open: confirm before implementing 5a.** |

---

*End of progress + handoff document. The next session can pick up from §"How to resume" and not re-read the conversation transcript.*
