# Agent GUI Wiring + Agent Settings Tab — Design

- **Date:** 2026-05-06
- **Status:** approved (brainstorm-only; awaiting writing-plans handoff)
- **Predecessor:** [`docs/inline-agent-stack-progress-2026-05-06.md`](../../inline-agent-stack-progress-2026-05-06.md)
- **Original report:** [`docs/inline-agent-command-execution-report.md`](../../inline-agent-command-execution-report.md)
- **Resumes from:** Phase 4c3 complete (commit `ae97710`). Remaining phases: 5a/5b/3c/4c4.

## 0. Scope

Five additions, landing in this order. Each step is verified by its tests before the next begins.

1. `omw-config` schema additions + `toml_edit` round-trip writer.
2. New `OmwAgent` settings tab in `vendor/warp-stripped/app/src/settings_view/`.
3. `OmwAgentState` reads `omw-config` at session start; approval mode flows through `session/create`.
4. **Phase 3c** — `panel.rs` flip from placeholder to a real `OmwAgentTranscriptModel`-backed render.
5. **Phase 4c4** — approval cards inside the transcript with Approve/Reject buttons.
6. **Phase 5a** — bash broker server-side (`omw-server` + `apps/omw-agent` adapter).
7. **Phase 5b** — bash broker GUI-side (`omw_command_broker.rs` + `register_active_terminal`).

End state: end-to-end automated coverage. Manual smoke is reserved for visual layout review only — never for verifying correctness.

Out of scope: max-turns cap (skipped), env-var key refs (deferred), retention/redaction (PRD §11), v0.2's `[routing]` block (forward-compat reservation only).

## 1. Architecture

### 1.1 `omw-config` additions

`crates/omw-config/src/schema.rs` grows two top-level blocks. Both already pass the existing forward-compat test (`unknown_top_level_table_is_tolerated_for_forward_compat`); this change moves them from "tolerated unknown" to "first-class typed."

```toml
# new in v0.2
[approval]
mode = "ask_before_write"   # "read_only" | "ask_before_write" | "trusted"

[agent]
enabled = true
```

Schema:

```rust
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(default)]
pub struct Config {
    pub version: SchemaVersion,
    pub default_provider: Option<ProviderId>,
    pub providers: BTreeMap<ProviderId, ProviderConfig>,
    pub approval: ApprovalConfig,   // NEW
    pub agent: AgentConfig,         // NEW
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(default)]
pub struct ApprovalConfig {
    pub mode: ApprovalMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalMode {
    ReadOnly,
    AskBeforeWrite,
    Trusted,
}

impl Default for ApprovalMode {
    fn default() -> Self { Self::AskBeforeWrite }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(default)]
pub struct AgentConfig {
    pub enabled: bool,
}

impl Default for AgentConfig {
    fn default() -> Self { Self { enabled: true } }
}
```

`serde(default)` on every struct keeps `Config::default()` and existing test fixtures unbroken. `ApprovalMode` deliberately mirrors `omw_policy::ApprovalMode` enum names; the wire format is the kebab-case JSON shape used in `apps/omw-agent/src/policy.ts:11`.

### 1.2 `omw-config` writer

New module `crates/omw-config/src/writer.rs`. Public API:

```rust
/// Atomically save a Config to the given path, preserving user comments,
/// key order, and unknown tables (e.g. a v0.3 [routing] block) wherever
/// possible. Writes to <path>.tmp + rename. Creates parent dirs if missing.
pub fn save_atomic(path: &Path, cfg: &Config) -> Result<(), ConfigError>;
```

Implementation strategy:

1. Read the existing file via `toml_edit::DocumentMut::parse`. If it doesn't exist, start from an empty document.
2. **Field-level (not table-level) updates.** For each managed leaf key (e.g. `providers.openai-prod.kind`, `providers.openai-prod.default_model`), set or overwrite the value via `doc[...]`-style indexing. Never replace a whole `[providers.x]` table — that would erase any user-authored subfield (e.g. a future `temperature = 0.7` not yet in our schema). The writer touches only keys it owns.
3. For each *removed* managed entity (e.g. a provider deleted in the form), call `doc["providers"].as_table_mut().remove(id)`. For a removed top-level managed block (e.g. a future opt-out of `[approval]`), the writer leaves it untouched — removal is a v0.3 problem.
4. Write `doc.to_string()` to `<path>.tmp`, then `rename` to `<path>`.

Library decision: `toml_edit = "0.22"` (already widely used in Rust ecosystem). Adds one dep to `omw-config`'s `Cargo.toml`.

Tests pin:
- `[providers.x]` round-trip: edit kind, save, re-read, byte-equal save.
- Comment preservation: TOML with `# my notes` survives a save.
- Unknown table preservation: `[routing]` block survives a save where only `[providers]` was touched.
- Atomicity: kill-mid-write produces no half-written file at `path` (verified by killing the process between tempfile-write and rename).

### 1.3 New `OmwAgent` settings page

New variant in `vendor/warp-stripped/app/src/settings_view/mod.rs`:

```rust
pub enum SettingsSection {
    // ... existing ...
    #[cfg(feature = "omw_local")]
    OmwAgent,
}

impl Display for SettingsSection {
    // ...
    #[cfg(feature = "omw_local")]
    SettingsSection::OmwAgent => write!(f, "Agent"),
}

fn is_visible_in_omw_local_mode(&self) -> bool {
    matches!(self,
        // existing ones ...
        | Self::OmwAgent  // NEW
    )
}
```

Nav registration in `SettingsView::new` (around `mod.rs:1287`):

```rust
#[cfg(feature = "omw_local")]
SettingsNavItem::Page(SettingsSection::OmwAgent),
```

Page module `vendor/warp-stripped/app/src/settings_view/omw_agent_page.rs` (new file, ~600 LOC, mirrors the `appearance_page.rs` shape).

State machine:

```rust
pub struct OmwAgentPageState {
    /// Loaded from omw-config at construction; rewritten on Apply.
    saved_config: omw_config::Config,
    /// Mutable form copy; diverges from saved_config until Apply or Discard.
    form: OmwAgentForm,
    /// Per-provider transient state for unsaved API key edits.
    pending_secrets: BTreeMap<ProviderId, String>,
    is_dirty: bool,
    last_save_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OmwAgentForm {
    pub agent_enabled: bool,
    pub approval_mode: ApprovalMode,
    pub default_provider: Option<ProviderId>,
    pub providers: Vec<ProviderRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRow {
    pub id: String,                       // editable; validated on Apply
    pub kind: ProviderKindForm,
    pub model: String,
    pub base_url: String,
    pub key_ref_token: String,            // current "keychain:omw/<id>" token (or empty)
    pub api_key_input: String,            // pasted value; cleared on Apply
}

pub enum OmwAgentPageAction {
    ToggleEnabled,
    SetApprovalMode(ApprovalMode),
    AddProvider,
    RemoveProvider(usize),
    SetProviderId(usize, String),
    SetProviderKind(usize, ProviderKindForm),
    SetProviderModel(usize, String),
    SetProviderBaseUrl(usize, String),
    SetProviderApiKey(usize, String),
    SetDefault(usize),
    Apply,
    Discard,
}
```

Pure functions extracted for L1 unit tests (no `App` context required):

```rust
pub fn form_from_config(cfg: &Config) -> OmwAgentForm;
pub fn validate_form(form: &OmwAgentForm) -> Result<(), Vec<FormError>>;
pub fn form_to_config(
    form: &OmwAgentForm,
    persisted_secrets: &BTreeMap<ProviderId, KeyRef>,
) -> Result<Config, FormError>;
pub fn apply_action(state: &mut OmwAgentPageState, action: OmwAgentPageAction);
```

Apply pipeline (`apply_action(Apply)`):

1. `validate_form(&state.form)` → returns errors if any (duplicate ids, invalid base URL, default points to missing id).
2. For each `(id, secret)` in `state.pending_secrets`: call `omw_keychain::set(&KeyRef::new(format!("omw/{id}"))?, &secret)`.
3. Build `Config` from form + resolved key_refs.
4. `omw_config::writer::save_atomic(&path, &cfg)`.
5. On success: `state.saved_config = cfg.clone(); state.form = form_from_config(&cfg); state.pending_secrets.clear(); state.is_dirty = false`.
6. On error: store in `state.last_save_error`; do not mutate `saved_config`.

Crucially, the **keychain write happens before the TOML write**. A crash mid-Apply leaves the keychain populated with an entry that nothing in TOML references; the next session has stale-but-orphan entries (handled by `omw_keychain::list_omw` cleanup, not part of this work).

### 1.4 `OmwAgentState` reads `omw-config`

Existing: `start(params: OmwAgentSessionParams) -> Result<(), String>`. Caller-supplies params.

After this work: a thin convenience layer that reads `omw-config` and constructs params:

```rust
impl OmwAgentState {
    pub fn start_with_config(self: &Arc<Self>) -> Result<(), String> {
        let cfg = omw_config::Config::load().map_err(|e| e.to_string())?;
        if !cfg.agent.enabled {
            return Err("Agent is disabled in settings".into());
        }
        let provider_id = cfg.default_provider
            .as_ref()
            .ok_or("No default provider configured")?;
        let provider = cfg.providers.get(provider_id)
            .ok_or_else(|| format!("default_provider `{provider_id}` not found"))?;
        let params = resolve_params(provider, cfg.approval.mode);
        self.start(params)
    }
}
```

`OmwAgentSessionParams` grows one field:

```rust
pub struct OmwAgentSessionParams {
    pub provider_kind: String,
    pub key_ref: Option<String>,
    pub base_url: Option<String>,
    pub model: String,
    pub system_prompt: Option<String>,
    pub cwd: Option<String>,
    pub approval_mode: Option<String>,    // NEW: "read_only" | "ask_before_write" | "trusted"
}
```

The omw-server `POST /api/v1/agent/sessions` body and the kernel's `session/create.policy.mode` field both mirror this directly. (The kernel already accepts `policy: PolicyConfig` per `apps/omw-agent/src/policy.ts:13` — no kernel change needed.)

omw-server's `crates/omw-server/src/handlers/agent.rs::create_session_handler` parses the new field and forwards it as `policy: { mode: <value> }` in the JSON-RPC `session/create` params. One small edit; existing `agent_session.rs` integration tests pin the round-trip.

### 1.5 Panel render (Phase 3c)

Per progress doc §"Phase 3c", with one revision: the panel calls `OmwAgentState::shared().start_with_config()` rather than constructing `OmwAgentSessionParams` itself. Settings tab is the source of truth.

New file: `vendor/warp-stripped/app/src/ai_assistant/omw_panel.rs` (~400 LOC).

Contains: `render_omw_agent_panel(panel: &Panel, app: &AppContext) -> Box<dyn Element>`. Subscribes to `OmwAgentState::shared().subscribe_events()` once at panel construction; routes through an `async_channel` bridge into the `OmwAgentTranscriptModel::apply_event` mutation surface (mirrors `omw/remote_state.rs`'s subscribe-status-stream pattern).

Edits in `panel.rs`:
- Add `omw_agent: Option<ModelHandle<OmwAgentTranscriptModel>>` field (cfg-gated).
- `new_omw_placeholder` → `new_omw_panel`: allocates the omw model + view; calls `start_with_config()`; stores the bridge subscription.
- `panel.rs:1101` (focus short-circuit): focus routes to omw editor.
- `panel.rs:1122` (placeholder render): becomes `render_omw_agent_panel(self, app)`.
- Static `OMW_PLACEHOLDER_TEXT` line goes away.

If `start_with_config` returns `Err` (e.g. no providers configured), render an inline empty-state with a "Open Agent settings" button → dispatches `SettingsViewEvent` to navigate to `OmwAgent`.

Per D15, the running session continues with the old config after a settings Apply. The panel renders a one-line banner ("Settings changed — restart agent to apply") with a "Restart" button when the loaded `Config` diverges from the in-flight session's params. Click `Restart` → `OmwAgentState::stop()` → `start_with_config()`. The banner is dismissed when the config converges.

### 1.6 Approval cards (Phase 4c4)

`OmwAgentTranscriptModel::apply_event(OmwAgentEventDown::ApprovalRequest { .. })` already creates an `OmwAgentMessage::Approval` row. This work adds the *render* with two clickable buttons.

Click handlers dispatch to `OmwAgentState::send_approval_decision(approval_id, decision)`:

```rust
impl OmwAgentState {
    pub fn send_approval_decision(
        &self,
        approval_id: String,
        decision: ApprovalDecision,
    ) -> Result<(), String> {
        let outbound = self.inner.lock().outbound.clone()
            .ok_or("No active agent session")?;
        outbound
            .blocking_send(OmwAgentEventUp::ApprovalDecision { approval_id, decision })
            .map_err(|e| e.to_string())
    }
}
```

The model's `update_approval(approval_id, status)` flips the card's status when the kernel responds (which it does by sending the next stream of frames; there's no explicit ACK).

### 1.7 Bash broker — server side (Phase 5a)

Pattern B per progress doc: bash/exec, bash/data, bash/finished are all **JSON-RPC notifications** correlated by `commandId`. Smaller diff than bidirectional request/response.

New files:
- `apps/omw-agent/src/warp-session-bash.ts` — `createWarpSessionBashOperations({ rpc, terminalSessionId, agentSessionId, toolCallId })`.
- `apps/omw-agent/test/warp-session-bash.test.ts` — vitest with mocked stdio.
- `crates/omw-server/src/agent/bash_broker.rs` — Rust broker.
- `crates/omw-server/tests/agent_bash.rs` — round-trip integration test.

Edits:
- `apps/omw-agent/src/session.ts` — register the bash AgentTool.
- `apps/omw-agent/src/serve.ts` — handle inbound `bash/data` and `bash/finished`.
- `crates/omw-server/src/agent/process.rs::route_frame` — dispatch `bash/exec` to the broker.
- `crates/omw-server/src/agent/mod.rs` — `pub mod bash_broker`.
- `vendor/warp-stripped/app/src/ai_assistant/omw_protocol.rs` — un-`#[allow(dead_code)]` the `ExecCommand`/`CommandData`/`CommandExit` variants.

### 1.8 Bash broker — GUI side (Phase 5b)

Per progress doc §"Phase 5b", with the file list unchanged.

New file: `vendor/warp-stripped/app/src/ai_assistant/omw_command_broker.rs`.

Edits:
- `omw_agent_state.rs` — add `register_active_terminal(view_id, event_loop_tx, pty_reads_tx, current_size)` method.
- `terminal/view.rs` — focus-change hook calls `OmwAgentState::shared().register_active_terminal(...)`.

Block-end detection: OSC 133 prompt-end emits `CommandExit { exit_code: Some(n) }`. After 30 s with no OSC 133, emits `CommandExit { snapshot: true }`. Audit gets a `command_snapshot` event in the latter case.

## 2. Data flow

### 2.1 Settings save

```
[user clicks "Apply"]
  → OmwAgentPageAction::Apply
  → validate_form(&form)
  → for (id, secret) in pending_secrets:
        omw_keychain::set(KeyRef::new(format!("omw/{id}"))?, &secret)
  → cfg = form_to_config(&form, &resolved_key_refs)?
  → omw_config::writer::save_atomic(&config_path, &cfg)
  → state.saved_config = cfg; state.form = form_from_config(&cfg); state.pending_secrets.clear()
```

Watcher fires; `OmwAgentState::on_config_update` invalidates cached params (next `start_with_config` reloads).

### 2.2 Session start

```
panel.new_omw_panel(...)
  → OmwAgentState::shared().start_with_config()
      → Config::load() → resolve default_provider → ProviderConfig + approval_mode
      → POST omw-server /api/v1/agent/sessions
          body: { providerConfig: {kind, model, base_url, key_ref}, approvalMode }
        → 200 { sessionId }
      → connect WS /ws/v1/agent/:sessionId
      → status: Idle → Starting → Connecting → Connected
  → panel async-channel bridge subscribes events
```

### 2.3 Prompt round-trip

```
[user types in panel editor, presses Enter]
  → OmwAgentState::send_prompt(text)
  → outbound mpsc → WS frame { type: "prompt", text }
  → omw-server forwards as kernel session/prompt
  → kernel agentLoop streams provider response:
        ← assistant/delta { text }      // 1..N
        ← tool/call_started { toolCallId, name, params }
        ← tool/call_finished { toolCallId, result }
        ← turn/finished
  → omw-server fans each notification to the per-session broadcast bus
  → panel async-channel bridge → OmwAgentTranscriptModel::apply_event
```

### 2.4 Approval round-trip

```
[kernel hits a write-class command, policy returns Ask]
  ← approval/request { approvalId, toolCall: {name, params} }
  → panel renders ApprovalCard { status: Pending }
[user clicks Approve]
  → OmwAgentState::send_approval_decision(approvalId, Approve)
  → outbound mpsc → WS { type: "approval_decision", approvalId, decision: "approve" }
  → omw-server → kernel approval/decide
  → kernel resolves the pending Promise → tool runs (or doesn't)
  ← (continues with tool/call_started, tool/call_finished, ...)
  → transcript: card.update_approval(approvalId, Approved)
```

### 2.5 Bash round-trip (Pattern B)

```
[kernel BashOperations.exec("ls", cwd, opts)]
  → adapter allocates commandId, registers per-id subscriber on serve.ts
  → notification: bash/exec { commandId, command, cwd, terminalSessionId }
  → omw-server bash_broker
      → looks up GUI WS for terminalSessionId
      → forward as OmwAgentEventDown::ExecCommand { commandId, command, cwd }
  → GUI omw_command_broker (registered active terminal)
      → emits Event::ExecuteCommand on registered event_loop_tx
      → pane runs command; PTY echoes chunks
      → broker taps pty_reads_tx, emits per-chunk:
        OmwAgentEventUp::CommandData { commandId, bytes }
      → on OSC 133 prompt-end (or 30s timeout):
        OmwAgentEventUp::CommandExit { commandId, exit_code | snapshot:true }
  → omw-server forwards each as:
        bash/data { commandId, bytes }
        bash/finished { commandId, exitCode | snapshot:true }
  → adapter resolves exec promise → kernel uses tool result
```

`commandId` is the only correlator in flight — concurrent bash calls don't collide.

## 3. Settings UI shape

```
┌─ Agent ────────────────────────────────────────┐
│ [☑] Enable agent                              │
│                                                │
│ Approval mode                                  │
│  ◯ Read only                                   │
│  ● Ask before write                            │
│  ◯ Trusted                                     │
│                                                │
│ Providers                          [+ Add]     │
│ ┌────────────────────────────────────────────┐ │
│ │ ● [openai-prod]                  Default   │ │
│ │   Kind: openai           ▼                 │ │
│ │   Model: gpt-4o                            │ │
│ │   API key: ••••••••••••••  [Set new]       │ │
│ │                            [Remove]        │ │
│ └────────────────────────────────────────────┘ │
│ ┌────────────────────────────────────────────┐ │
│ │ ○ [ollama-local]                           │ │
│ │   Kind: ollama           ▼                 │ │
│ │   Base URL: http://127.0.0.1:11434         │ │
│ │   Model: llama3.1:8b                       │ │
│ │   API key: (none)        [Set]             │ │
│ │                          [Set as default]  │ │
│ │                          [Remove]          │ │
│ └────────────────────────────────────────────┘ │
│                                                │
│ Status: Saved.                                 │
│ [Apply changes]    [Discard]                   │
└────────────────────────────────────────────────┘
```

Invariants enforced by `validate_form`:
- Provider IDs match `ProviderId` grammar (alphanumeric + `_-`).
- No duplicate IDs.
- Default provider points to an existing row.
- `openai-compatible` rows require non-empty base URL.
- `openai`, `anthropic`, and `openai-compatible` rows require an API key (existing key_ref OR a pending paste).

Visibility rules per provider kind:
- `openai`, `anthropic`: hide base URL field.
- `openai-compatible`: show all four (kind + model + base URL + key).
- `ollama`: show all four; key is optional.

## 4. Test plan

### 4.1 L1 — Pure unit tests

| Module | Tests | Surface |
|---|---|---|
| `omw-config` schema | 6 | `[approval]` and `[agent]` round-trip; defaults; missing-block tolerated; unknown-kind rejected |
| `omw-config::writer` | 6 | Round-trip; comment preservation; unknown-table preservation; atomicity; create-parent-dir; permissions |
| `omw_agent_page` reducer | 8 | Each `OmwAgentPageAction` variant; default-when-removing-default; Discard resets |
| `omw_agent_page` validators | 6 | Invalid id; duplicate ids; invalid base URL; default-points-to-missing; missing key for openai/anthropic |
| `omw_agent_page` form↔cfg | 4 | Empty config round-trip; full round-trip with multiple providers; ollama-with-no-key; default flag |

All independent of `App` context; fast (sub-second total).

### 4.2 L2 — Config round-trip tests

In `crates/omw-config/tests/round_trip.rs` (new). Pins a fixture file at each step:

```toml
# v0.2 example fixture
version = 1
default_provider = "openai-prod"

# user-authored comment about why we use openai-prod
[approval]
mode = "ask_before_write"

[agent]
enabled = true

[providers.openai-prod]
kind = "openai"
key_ref = "keychain:omw/openai-prod"
default_model = "gpt-4o"

[routing]   # v0.3 forward-compat block; should survive a write
default = "openai-prod"
```

After `save_atomic(load(fixture))`, assert: file is byte-equal modulo trailing newline normalisation; the `[routing]` block survives; the comment survives.

### 4.3 L3a — `App::test` interaction tests

Files in `vendor/warp-stripped/app/tests/` (new test-target binaries; sidesteps the broken lib test target per progress doc):

`omw_agent_settings_test.rs` (6 tests):
- `mounting_renders_with_loaded_config`
- `clicking_add_provider_appends_form_row`
- `editing_provider_kind_dropdown_dispatches_action`
- `clicking_apply_with_invalid_form_shows_error_does_not_save`
- `clicking_apply_writes_to_temp_config_path` (sets `OMW_CONFIG` to a `tempfile::NamedTempFile`)
- `clicking_discard_resets_form_to_saved`

`omw_agent_panel_test.rs` (7 tests):
- `panel_mount_with_no_providers_renders_empty_state`
- `panel_mount_with_valid_config_starts_omw_agent_state`
- `start_with_config_resolves_default_provider_from_toml_fixture` (sets `OMW_CONFIG`, asserts the resolved `OmwAgentSessionParams`)
- `inbound_assistant_delta_appends_to_transcript`
- `inbound_tool_call_started_renders_tool_card`
- `tool_call_finished_flips_card_status`
- `prompt_editor_enter_sends_outbound_prompt_frame`

`omw_agent_approval_test.rs` (4 tests):
- `approval_request_renders_card_pending`
- `clicking_approve_sends_approval_decide_approve`
- `clicking_reject_sends_approval_decide_reject`
- `update_approval_flips_card_status`

`omw_agent_command_broker_test.rs` (5 tests):
- `register_active_terminal_stores_handle`
- `exec_command_emits_execute_command_event`
- `pty_reads_emit_command_data_upstream`
- `osc133_prompt_end_emits_command_exit_with_exit_code`
- `30s_timeout_emits_command_exit_with_snapshot_true`

Each integration test uses `App::test((), |mut app| async move { ... })` per the `code_review_view_tests.rs` pattern. Required `pub use` re-exports for items that are currently `pub(crate)`:

```rust
// vendor/warp-stripped/app/src/lib.rs (or root lib module)
#[cfg(any(test, feature = "test-util"))]
pub mod test_exports {
    pub use crate::ai_assistant::{
        OmwAgentEventDown, OmwAgentEventUp, OmwAgentState,
        OmwAgentTranscriptModel,
    };
    pub use crate::settings_view::omw_agent_page::{
        OmwAgentForm, OmwAgentPageAction, OmwAgentPageState,
    };
}
```

Gate behind `cfg(any(test, feature = "test-util"))` so the surface stays internal in shipped binaries.

### 4.4 L3b — Live `omw-server` tests

Extend `crates/omw-server/tests/agent_session.rs` with two new tests (these test the *server* — the body it forwards to the kernel — not the GUI's TOML resolution, which is L3a):
- `session_create_forwards_provider_config_to_kernel` — POST `/api/v1/agent/sessions` with a fixed body, assert mock-omw-agent receives `session/create` with matching `providerConfig`.
- `session_create_forwards_policy_mode_to_kernel` — same, but assert `policy.mode` matches the body's `approvalMode`.

New file `crates/omw-server/tests/agent_bash.rs` (5 tests):
- `bash_exec_notification_forwarded_as_exec_command_to_gui`
- `command_data_from_gui_forwarded_as_bash_data_to_kernel`
- `command_exit_from_gui_forwarded_as_bash_finished_to_kernel`
- `concurrent_bash_calls_routed_by_command_id`
- `bash_exec_with_no_active_gui_returns_error`

All use the existing `tests/fixtures/mock-omw-agent.mjs` fixture pattern.

### 4.5 What this covers

Together, L1+L2+L3 validates every interaction the user would otherwise verify by clicking through the app:

| Interaction | Verified by |
|---|---|
| Click in settings tab → form state changes | L3a `omw_agent_settings_test::editing_provider_kind_dropdown_dispatches_action` |
| Click "Apply" → config persisted to disk | L3a `..._writes_to_temp_config_path` + L1 reducer + L2 round-trip |
| Save invalid form → error shown, no save | L3a `..._invalid_form_shows_error_does_not_save` |
| Panel mount → session starts | L3a `panel_mount_with_valid_config_starts_omw_agent_state` |
| TOML config → resolved session params | L3a `start_with_config_resolves_default_provider_from_toml_fixture` |
| HTTP body → kernel `session/create` | L3b `session_create_forwards_provider_config_to_kernel` + `..._forwards_policy_mode_...` |
| Inbound assistant delta → transcript renders | L3a `inbound_assistant_delta_appends_to_transcript` |
| Click Approve in card → outbound decision sent | L3a `clicking_approve_sends_approval_decide_approve` |
| Inbound ExecCommand → terminal writes | L3a `exec_command_emits_execute_command_event` |
| PTY reads → upstream CommandData | L3a `pty_reads_emit_command_data_upstream` |
| OSC 133 → upstream CommandExit | L3a `osc133_prompt_end_emits_command_exit_with_exit_code` |

Manual smoke (running the built debug app) is reserved exclusively for visual layout review. It is no longer a *correctness* check.

## 5. Phasing + file list

| # | Step | New / edited | Tests |
|---|---|---|---|
| 1 | omw-config schema additions | `crates/omw-config/src/schema.rs` (edit), `src/lib.rs` (edit) | L1 ×6 |
| 2 | omw-config writer | `crates/omw-config/src/writer.rs` (new), `Cargo.toml` (+`toml_edit`), `src/lib.rs` (edit re-exports), `tests/round_trip.rs` (new) | L1 ×6, L2 ×4 |
| 3 | omw-server agent session approval-mode passthrough | `crates/omw-server/src/handlers/agent.rs` (edit) | L3b ×2 |
| 4 | OmwAgentSessionParams + start_with_config | `vendor/warp-stripped/app/src/ai_assistant/omw_agent_state.rs` (edit), test_exports module | (covered by L3a in step 6) |
| 5 | OmwAgent settings page | `vendor/warp-stripped/app/src/settings_view/omw_agent_page.rs` (new ~600 LOC), `mod.rs` (edit ~30 LOC) | L1 ×18 |
| 6 | Settings page App::test integration | `vendor/warp-stripped/app/tests/omw_agent_settings_test.rs` (new) | L3a ×6 |
| 7 | Phase 3c — panel.rs flip | `vendor/warp-stripped/app/src/ai_assistant/omw_panel.rs` (new ~400 LOC), `panel.rs` (edit) | L3a ×6 |
| 8 | Phase 4c4 — approval cards | `omw_transcript.rs` (edit), `omw_agent_state.rs` (edit `send_approval_decision`) | L3a ×4 |
| 9 | Phase 5a — bash broker (server + adapter) | `apps/omw-agent/src/warp-session-bash.ts` (new), `test/warp-session-bash.test.ts` (new), `src/session.ts` (edit), `src/serve.ts` (edit), `crates/omw-server/src/agent/bash_broker.rs` (new), `process.rs` (edit), `mod.rs` (edit), `tests/agent_bash.rs` (new), `omw_protocol.rs` (un-dead-code) | TS ×10, L3b ×5 |
| 10 | Phase 5b — bash broker (GUI) | `vendor/warp-stripped/app/src/ai_assistant/omw_command_broker.rs` (new), `omw_agent_state.rs` (edit), `terminal/view.rs` (edit) | L3a ×5 |

Total: ~25-30 new test cases pinning the new surface. Each step's tests must be green before the next step begins.

## 6. Decisions log

Locked during this brainstorm:

| ID | Decision | Rationale |
|---|---|---|
| D9 | omw-config grows `[approval]` and `[agent]` blocks; `toml_edit` round-trip writer | Schema reservation already in place; `toml::to_string` mangles user comments |
| D10 | API key UX: paste in tab; tab calls `omw_keychain::set`; TOML stores only `keychain:<key_ref>` | omw-keychain already exposes `get/set/delete/list_omw`; I-1 invariant forbids plaintext on disk |
| D11 | New `OmwAgent` settings section under `cfg(feature = "omw_local")` | Mirrors existing nav pattern; pure-function reducer enables L1 unit tests |
| D12 | Headless interaction tests live in `vendor/warp-stripped/app/tests/` (integration target) using `App::test` | Sidesteps the broken `settings_view::mod_test.rs` lib test target; `App::test` already used by `code_review_view_tests.rs` |
| D13 | Phase order: settings infra → panel flip → approval cards → bash broker (server) → bash broker (GUI) | Each step verifiable by L1-L3 before the next builds on it |
| D14 | Approval mode default: `AskBeforeWrite` | Pinned in progress doc D5 |
| D15 | Settings tab "Apply" requires explicit click; running session continues with old config | Avoids killing in-flight work the user didn't ask to interrupt |
| D16 | Multi-provider editor: inline list, not modal | Matches `environments_page.rs` pattern; less click-through |
| D17 | Out of scope for v1: max-turns cap, env-var key refs, retention/redaction, v0.3 `[routing]` block | Each is independently scopable later; none blocks the inline-agent stack |
| D18 | API key paste field clears on Apply | Avoids re-saving the same secret twice; the keychain is already canonical |

## 7. Risks

| Risk | Severity | Mitigation |
|---|---|---|
| `App::test` may need additional singleton setup beyond what `code_review_view_tests.rs` shows for the omw_agent flows | Medium | `initialize_test_app` is the existing setup helper; copy and minimally extend per test file. If a singleton is missing for our paths, add it as a `cfg(test)` mock in the affected module. |
| `toml_edit` round-trip may not preserve every formatting nuance (line-end style, tab vs spaces) | Low | Define byte-equality only modulo trailing-newline normalisation; document this in the writer module |
| OSC 133 not emitted by user's shell | Medium | 30s timeout + snapshot is the documented fallback per progress doc D8; manual smoke verifies on both `zsh` (default) and `bash` |
| `settings_view::mod_test.rs` repair becomes urgent | Low | Already deferred to a separate PR per the progress doc; integration tests sidestep the issue |
| `register_active_terminal` race during focus change | Medium | Mutex<Option<...>> wrapper; concurrent registrations atomically replace |

## 8. References

- Progress doc: [`docs/inline-agent-stack-progress-2026-05-06.md`](../../inline-agent-stack-progress-2026-05-06.md)
- Original report: [`docs/inline-agent-command-execution-report.md`](../../inline-agent-command-execution-report.md)
- omw-config v0.1: `crates/omw-config/src/schema.rs`
- omw-policy v0.1: `crates/omw-policy/src/lib.rs`
- omw-keychain v0.1: `crates/omw-keychain/src/lib.rs`
- App::test reference: `vendor/warp-stripped/app/src/code_review/code_review_view_tests.rs:339`
- Panel placeholder location: `vendor/warp-stripped/app/src/ai_assistant/panel.rs:1101,1122`
- TODO.md scope: `TODO.md` v0.4-cleanup
