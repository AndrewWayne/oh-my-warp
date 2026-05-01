# Cloud-Strip Plan — Compile and Audit `vendor/warp-stripped/` Without Cloud

Status: **Done 2026-05-01 (commit `aadae83`)** — see §8 Postscript for what executed vs the original plan.
Last updated: 2026-05-01
Owners: TBD
Source: synthesized by Codex (gpt-5.5, xhigh reasoning) with human-curated context, 2026-05-01

This spec defines the work needed to make the `--no-default-features --features omw_local` build of `warp-oss` compile cleanly and pass `scripts/audit-no-cloud.sh`. It is a v0.3 deliverable (see PRD §13 v0.3, [`specs/fork-strategy.md`](./fork-strategy.md) §5) that was deferred in commit `c9d2540` and remains open as of this spec.

---

## 0. Goal

```
cd vendor/warp-stripped
PROTOC=/opt/homebrew/bin/protoc cargo build -p warp --bin warp-oss \
  --no-default-features --features omw_local
scripts/audit-no-cloud.sh target/debug/warp-oss
```

must produce a clean build (zero warnings introduced by this work) AND an audit report with zero forbidden hostnames in the binary.

---

## 1. Context: what's been tried

### 1.1 Earlier strip work (today's branch `omw/strip-built-in-ai`, 11 commits)

UI cloud surfaces gated under `#[cfg(not(feature = "omw_local"))]` at the dispatcher level: signup wall, AI panel, settings tabs, sign-in callouts, cloud menus. Cloud-only crates marked `optional = true` in `vendor/warp-stripped/app/Cargo.toml` and grouped under a new `cloud` Cargo feature listed in `default = […]`. Build invocation in `OMW_LOCAL_BUILD.md` updated to `--no-default-features --features omw_local`.

Acknowledgement in commit `c9d2540`: *"the source-level work to gate every consumer of the cloud crates… is much larger than anticipated (230+ unresolved imports cascading through ~120 files). The source cascade is deferred to follow-up work."*

### 1.2 Failed approach: top-down `#[cfg(feature = "cloud")]` gating

Attempted on 2026-05-01: gate `mod cloud_object;` and `mod drive;` in `app/src/lib.rs`, then mass-add `#[cfg(feature = "cloud")]` to every `use crate::cloud_object`, `use crate::drive`, `use warp_server_client`, `use warp_managed_secrets`, `use onboarding`, `use firebase`, `use voice_input` line across 139 files via a Python pass.

Errors went **up**, not down: 230 → 387 → 1217. Top file `workspace/view.rs` went 12 → 191 errors. Reverted. Tree is back to the c9d2540 deferred state.

**Why it failed.** `cloud_object` is a *backbone*, not a leaf. Cloud DTOs (`CloudObjectSyncStatus`, `NumInFlightRequests`, `CloudObjectMetadata`, `CloudFolderModel`, `SharingAccessLevel`, `Revision`, `Owner`, `ServerId`, …) live in struct fields, function signatures, generic params, and match arms — not just `use` lines. Gating imports exposed every inline reference.

---

## 2. Verdict: hybrid stub-and-port approach

| Subsystem | Approach |
|---|---|
| `warp_server_client` (backbone DTOs) | **Real local compat copy.** Port the real value types into `app/src/cloud_compat/warp_server_client/`. Re-export from there: real crate when `cloud` is on, local copies when `cloud` is off. **Not** stubs — the types are used too widely for panic-stubs to be safe. |
| `firebase` | **Panic / no-op shim.** No-cloud auth exchange returns `UserAuthenticationError::Unexpected`. |
| `warp_managed_secrets` | **Panic / no-op shim.** List/read returns empty; mutate/federation returns disabled error. |
| `voice_input` | **Panic / no-op shim.** `start_listening` returns `AccessDenied`; state stays idle. |
| `onboarding` | **Panic / no-op shim.** Views render `Empty`; constructors no-op. UI dispatcher gates from earlier strip commits already prevent these from being mounted in `omw_local` builds. |

Audit-green is **not** automatic from crate unlinking. Forbidden hostname literals are still hardcoded in:
- `crates/warp_core/src/channel/config.rs:46` (`WarpServerConfig::production()`, `OzConfig::production()`)
- `app/src/auth/credentials.rs:173` (Firebase URL builders)

These must be `#[cfg]`-eliminated at compile time. Runtime branches via `ChannelState::official_cloud_services_enabled()` are insufficient — `audit-no-cloud.sh` is string-level.

---

## 3. Phased Plan

### Phase 0 — Baseline log

- **Time:** 0.25 day
- **Files:** none
- **Verify:** `cd vendor/warp-stripped && PROTOC=/opt/homebrew/bin/protoc cargo check -p warp --bin warp-oss --no-default-features --features omw_local 2>&1 | tee /tmp/omw-local-baseline.log`. Record `error[E…]` count; expected ~230.

### Phase 1 — `warp_server_client` backbone compat

- **Time:** 1 day
- **Create:**
  - `app/src/cloud_compat/mod.rs`
  - `app/src/cloud_compat/warp_server_client/mod.rs`
  - `app/src/cloud_compat/warp_server_client/ids.rs`
  - `app/src/cloud_compat/warp_server_client/auth/user_uid.rs`
  - `app/src/cloud_compat/warp_server_client/cloud_object.rs`
  - `app/src/cloud_compat/warp_server_client/drive/sharing.rs`
  - `app/src/cloud_compat/warp_server_client/persistence.rs`
- **Edit:**
  - `app/src/lib.rs`
  - `app/src/cloud_object/mod.rs`
  - `app/src/server/ids.rs`
  - `app/src/auth/user.rs`, `auth/user_uid.rs`
  - `app/src/drive/folders/mod.rs`, `drive/sharing/mod.rs`
  - `app/src/persistence/cloud_objects.rs`, `persistence/sqlite.rs`
  - `app/src/ai/cloud_environments/mod.rs`
- **Port (real value types, single identity across the app):** `ClientId`, `SyncId`, `ServerId`, `FolderId`, `UserUid`, `ObjectType`, `ObjectIdType`, `Revision`, `Owner`, `ServerMetadata`, `ServerPermissions`, `CloudObjectMetadata`, `CloudObjectPermissions`, `CloudObjectSyncStatus`, `NumInFlightRequests`, `CloudObjectStatuses`, `CloudObjectEventEntrypoint`, `SharingAccessLevel`, `Subject`, `TeamKind`, `UserKind`. Preserve real local behavior for IDs, serde, SQLite hashing, permissions, metadata, and persistence helpers. `CloudObjectStatuses::render_icon` may return `None`; do not panic.
- **Verify:** `cargo check` shows zero unresolved `warp_server_client` imports. Total error count NOT required to be zero yet.

### Phase 2 — Service-crate shims

- **Time:** 1.5 days
- **Create:**
  - `app/src/cloud_compat/firebase.rs`
  - `app/src/cloud_compat/warp_managed_secrets.rs`
  - `app/src/cloud_compat/voice_input.rs`
  - `app/src/cloud_compat/onboarding.rs`
- **Edit references in:**
  - **Managed secrets:** `app/src/lib.rs`, `app/src/server/server_api.rs`, `app/src/server/server_api/managed_secrets.rs`, `app/src/auth/auth_state.rs`, `app/src/ai/aws_credentials.rs`, `app/src/ai/agent_sdk/{mod,driver,federate,secret}.rs`, `app/src/ai/agent_sdk/driver/cloud_provider/{aws,gcp}.rs`, `app/src/ai/mcp/templatable_installation.rs`.
  - **Voice:** `app/src/editor/view/{voice,mod,element}.rs`, `app/src/terminal/{view,view/action,input,universal_developer_input,alt_screen/alt_screen_element,block_list_element}.rs`, `app/src/ai/blocklist/block.rs`, `app/src/ai/blocklist/agent_view/agent_input_footer/mod.rs`.
  - **Onboarding:** `app/src/ai/onboarding.rs`, `app/src/auth/login_slide.rs`, `app/src/root_view.rs`, `app/src/settings/onboarding.rs`, `app/src/terminal/{view,view/action}.rs`, `app/src/workspace/view/onboarding.rs`, `app/src/lib.rs`.
  - **Firebase:** `app/src/server/server_api/auth.rs`.
- **Stubs:**
  - `firebase`: `FirebaseError`, `FetchAccessTokenResponse`. No-cloud auth exchange returns `UserAuthenticationError::Unexpected`.
  - `warp_managed_secrets`: `ActorProvider`, `ManagedSecretManager`, `ManagedSecretsClient`, `ManagedSecretValue`, `SecretOwner`, `IdentityTokenOptions`, `TaskIdentityToken`, `ManagedSecretConfigs`, `GcpFederationConfig`, `GcpCredentials`. List/read returns empty where safe; create/update/delete/federation returns disabled errors.
  - `voice_input`: `VoiceInput`, `VoiceInputState`, `VoiceInputToggledFrom`, `VoiceSession`, `VoiceSessionResult`, `StartListeningError`. `start_listening` returns `AccessDenied` or disabled error; `await_result` returns `Aborted`; state stays idle.
  - `onboarding`: `OnboardingIntention`, `SessionDefault`, `SelectedSettings`, `UICustomizationSettings`, `AgentDevelopmentSettings`, `AgentAutonomy`, `ProjectOnboardingSettings`, `OnboardingModelInfo`, `OnboardingAuthState`, `AgentOnboardingView/Event/Action`, `OnboardingCalloutView/Event`, `OnboardingKeybindings`, `FinalState`, `OnboardingQuery`, `slides::layout`, `slides::slide_content`. Views render `Empty`; constructors no-op.
- **Verify:** `cargo check` shows zero unresolved `firebase`, `warp_managed_secrets`, `onboarding`, `voice_input` imports.

### Phase 3 — Make no-cloud runtime inert

- **Time:** 1 day
- **Create (optional):** `app/src/server/server_api/no_cloud_object_client.rs`
- **Edit:** `app/src/server/server_api/object.rs`, `app/src/server/server_api.rs`, `app/src/server/cloud_objects/update_manager.rs`, `app/src/settings/cloud_preferences_syncer.rs`, `app/src/lib.rs`.
- **Behavior:**
  - `NoCloudObjectClient: ObjectClient`. `fetch_changed_objects` returns `InitialLoadResponse::default()`. `fetch_environment_last_task_run_timestamps` returns empty map. Mutating object methods return `Err(anyhow!("cloud objects disabled in omw_local"))`.
  - Don't register `Listener` in no-cloud builds, OR ensure it never starts a retry loop.
  - Initialize `CloudModel` with empty objects and `ObjectActions` empty under no-cloud.
  - Mark `UpdateManager::initial_load_complete()` as completed immediately under no-cloud.
  - `CloudPreferencesSyncer` becomes no-op; emits/consumes initial-load complete.
- **Verify:** `cargo check` reports zero compile errors for the target command.

### Phase 4 — Strip forbidden URL strings

- **Time:** 0.5 day
- **Edit:** `crates/warp_core/src/channel/config.rs`, `app/src/auth/credentials.rs`. Optional hardening: `app/src/wasm_nux_dialog.rs`.
- **Behavior:**
  - `WarpServerConfig::production()` and `OzConfig::production()` become `#[cfg(not(feature = "omw_local"))]`. omw builds compile only the local `127.0.0.1:0` defaults.
  - Firebase URL builders in `FirebaseToken` become `#[cfg(feature = "cloud")]`. The no-cloud variant returns disabled/empty and is not called.
  - Do NOT rely on runtime `if ChannelState::official_cloud_services_enabled()` for string elimination.
- **Verify:** `cargo build -p warp --bin warp-oss --no-default-features --features omw_local` succeeds; `scripts/audit-no-cloud.sh target/debug/warp-oss` reports all counts 0.

### Phase 5 — Regression guardrails

- **Time:** 0.5–1 day
- **Edit:** `OMW_LOCAL_BUILD.md`, `specs/fork-strategy.md`, CI scripts.
- **Verify:**
  - `cargo tree -p warp --no-default-features --features omw_local | rg 'firebase|warp_server_client|warp_managed_secrets|onboarding|voice_input'` returns zero hits.
  - Default-build guard: `cargo check -p warp --bin warp-oss` (without `--no-default-features`) still works — confirms `cloud` re-export still functions.
  - Final audit re-run.

---

## 4. Time Estimate

| Target | Estimate |
|---|---|
| Build clean + audit green for `--no-default-features --features omw_local` | **4 engineering days** |
| Above + keep default cloud build clean (compat re-exports real crates when `cloud` is on) | **5–6 days** |
| Above + expand audit beyond the current 6 hostname patterns (e.g. `securetoken.googleapis.com`, `oz.warp.dev`) | **~1 week** |

---

## 5. What NOT to do

- Do **not** gate `mod cloud_object` or `mod drive` wholesale at `app/src/lib.rs`. That is the path that produced the 1217-error cascade on 2026-05-01.
- Do **not** mass-`#[cfg]` `use` statements. Hides imports but exposes every inline type use.
- Do **not** define duplicate "minimal" `ServerId` / `CloudObjectMetadata` / `Owner` types in multiple modules. Single type identity across the app.
- Do **not** keep official URLs behind runtime `if` branches and assume the audit will pass — the audit is string-level and sees compiled literals.
- Do **not** re-enable the `cloud` feature just to make `omw_local` compile. That defeats the binary audit and the whole point of the strip.

---

## 6. Open Questions for Human Decision

1. **Persisted cloud objects.** Should no-cloud builds ignore old persisted cloud objects on disk, or migrate/delete them? Recommendation: ignore for now; avoid destructive migration.
2. **`CloudPreferencesSyncer`.** Goes purely local now, or waits for `omw-server` (v0.3 work)?
3. **Voice input.** Removed entirely, or re-routed to a local STT backend later?
4. **Default cloud build.** Must it remain supported alongside the `omw_local` strip? Recommendation: yes; the compat modules then re-export real crates when the `cloud` feature is enabled.
5. **Audit hostname coverage.** The current 6 patterns miss `securetoken.googleapis.com` and `oz.warp.dev`. Expand now (Phase 5) or as a follow-up?

---

## 7. Methodology Note

This plan was synthesized by OpenAI Codex (gpt-5.5 model, xhigh reasoning effort, read-only sandbox over the umbrella repo). The brief gave Codex: project context, the failed top-down gating attempt with error counts, the cargo feature configuration, the stub-types hypothesis, and pointers at the heaviest files. Codex pushed back on the pure-stub hypothesis and proposed the hybrid backbone-port + service-stub approach above. Human-curated; reviewed before commit.

---

## 8. Postscript — What Actually Shipped (2026-05-01, commit `aadae83`)

Status at start of execution: 230 errors under the target build. Codex projected ~4 engineering days of work across 5 phases, dominated by porting ~20 backbone DTOs (`CloudObjectMetadata`, `ServerId`, …) into a local compat module (Phase 1) and writing ~50 service stubs (Phase 2).

**Actual cost: ~5 hours and 5 small file edits.** The plan's foundational premise — that the optional crates needed source-level removal — turned out to be wrong on inspection. Phases 1, 2, and 3 collapsed into Cargo.toml edits.

### Diagnosis

The five "cloud" crates marked `optional = true` in commit `c9d2540` are not what they sound like:

| Crate | Actual content | Forbidden URL strings? |
|---|---|---|
| `warp_server_client` | Pure value types and IDs (`ClientId`, `ServerId`, `CloudObjectMetadata`, …). Deps: anyhow, serde, uuid, persistence — no network. | None |
| `firebase` | ~10 lines of Firebase request/response struct shapes. Deps: anyhow + serde only. | None |
| `warp_managed_secrets` | Local secret-management types + crypto (HPKE, tink). No network deps. | None |
| `voice_input` | Audio capture (cpal, hound, rubato). Local STT plumbing. | None |
| `onboarding` | UI views for the onboarding wizard. Has one `https://www.warp.dev/terms-of-service` literal but `www.warp.dev` doesn't match any of the audit's six hostname patterns. | None matching audit |

The actual forbidden hostnames live in three specific places, all in *non-optional* crates:

- `crates/warp_core/src/channel/config.rs:46-49` — `app.warp.dev`, `rtc.app.warp.dev`, `sessions.app.warp.dev`, the Firebase API key.
- `crates/warp_core/src/channel/config.rs:87` — `oz.warp.dev`.
- `app/src/auth/credentials.rs:170,173` — `securetoken.googleapis.com`, `identitytoolkit.googleapis.com`.
- `warp-command-signatures` (git dep) with the `embed-signatures` feature on — embeds `firebase.json` CLI completion data containing `firebaseio.com` strings. Pulled in by both `warp` and `warp_completer`.

### Execution

Five edits across five files:

1. `vendor/warp-stripped/app/Cargo.toml` — un-optional `firebase`, `warp_server_client`, `warp_managed_secrets`, `voice_input`, `onboarding`. The `cloud` feature now only gates `warp-command-signatures/embed-signatures`. Side fix: the `voice_input` feature stub became `[]` (was an invalid `dep:voice_input` ref).
2. `vendor/warp-stripped/crates/warp_completer/Cargo.toml` — drop the unconditional `embed-signatures` feature. The app crate now controls embedding via its `cloud` feature.
3. `vendor/warp-stripped/crates/warp_core/src/channel/config.rs` — `WarpServerConfig::production()` and `OzConfig::production()` bodies are gated under `#[cfg(not(feature = "omw_local"))]`. omw_local builds get an `omw_local()` redirect, so the URL string literals never reach the binary.
4. `vendor/warp-stripped/app/src/auth/credentials.rs` — `FirebaseToken::access_token_url()` body gated identically; omw_local stub returns `String::new()`.
5. `vendor/warp-stripped/crates/remote_server/src/install_remote_server.sh` — the script is `include_str!()`'d into the binary, so even comment text ends up in `.rodata`. Replaced the example URL `e.g. https://app.warp.dev/download/cli` in a header comment with a generic description.

### What still doesn't pass an *expanded* audit

The audit script (`scripts/audit-no-cloud.sh`) currently checks six hostname patterns. Two patterns Codex flagged as missing are:

- `securetoken.googleapis.com` — already cleared (gated via `credentials.rs`).
- `oz.warp.dev` — still present 5 times in the binary, from UI surfaces in `app/src/workspace/view/launch_modal/oz_launch.rs`, `workspace/view/openwarp_launch_modal/view.rs`, `terminal/view/ambient_agent/tips.rs`, `ai/agent_management/cloud_setup_guide_view.rs`. Tagging these correctly under `#[cfg(not(feature = "omw_local"))]` is straightforward but was deferred — they are not in the current audit's contract.

Adding `oz.warp.dev` to the audit list (and gating those five UI sites) is a clean follow-up.

### Lesson

`cargo` features named after subsystems (`cloud`, `firebase`) can be misleading. Before deciding a crate needs a stub-types port, look at:

1. The crate's *Cargo.toml* — does it actually depend on network or remote-service code?
2. The crate's *source* — does it contain hostname/URL string literals?

If the answer to both is no, the crate is safe to link unconditionally; the strip belongs at the URL-string sites in *consumer* crates, not at the crate boundary. This was the difference between 4 days of work and 5 hours.
