# Strip Built-In AI from `vendor/warp-stripped` — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Expand the `omw_local` Cargo feature in `vendor/warp-stripped/` so a `--features omw_local` build of `warp-oss` has no AI/cloud user surfaces and excludes cloud-shaped code from the binary, while remaining reversible (build without the flag → upstream-Warp behavior preserved).

**Architecture:** Source-level `#[cfg(not(feature = "omw_local"))]` gates on dispatchers, plus `optional = true` on cloud-only workspace crates that get excluded from `omw_local`. Two existing primitives we leverage: (1) the existing `omw_local` Cargo feature already declared in `vendor/warp-stripped/app/Cargo.toml:719`; (2) the existing runtime helper `ChannelState::official_cloud_services_enabled()` in `crates/warp_core/src/channel/state.rs` which returns `false` when the feature is on. Where the runtime helper already gates a surface correctly we reinforce it with a build-time `#[cfg]` (so the cloud module isn't even linked in); where it doesn't, we add a `#[cfg]` from scratch.

**Tech Stack:** Rust 1.92.0 (pinned via `vendor/warp-stripped/rust-toolchain.toml`), Cargo workspace, no extra tooling.

**Spec:** [`docs/superpowers/specs/2026-05-01-strip-built-in-ai-design.md`](../specs/2026-05-01-strip-built-in-ai-design.md)

---

## File Structure

This plan touches three areas. Each task identifies which file(s) it changes.

**Cargo configuration (workspace + app)**
- `vendor/warp-stripped/Cargo.toml` — workspace deps; mark cloud-only crates `optional = true`
- `vendor/warp-stripped/app/Cargo.toml` — `omw_local` feature definition (line 719) and import declarations

**App entry / dispatchers**
- `vendor/warp-stripped/app/src/lib.rs` — startup wiring; firebase anon-user already gated at 2946
- `vendor/warp-stripped/app/src/root_view.rs` — `AuthOnboardingState` enum and dispatcher (1676–1794)
- `vendor/warp-stripped/app/src/workspace/view.rs` — AI panel mount (1491–1504; 954; 2780–2781) and sign-in redirect (19511)
- `vendor/warp-stripped/app/src/settings_view/mod.rs` — `SettingsSection` enum (188–225)
- `vendor/warp-stripped/app/src/settings_view/settings_page.rs` — tab registration (~1200, 1203)
- `vendor/warp-stripped/app/src/app_menus.rs` — Help menu (963–978)
- `vendor/warp-stripped/app/src/auth/auth_view_body.rs` — `render_sign_in_row()` (489)
- `vendor/warp-stripped/app/src/server/server_api/auth.rs` — firebase imports (6)
- `vendor/warp-stripped/app/src/cloud_object/mod.rs` — warp_server_client re-exports (76)
- `vendor/warp-stripped/app/src/ai/cloud_environments/mod.rs` — cloud env (3)

**Cloud crates (gated/optional)**
- `crates/firebase`, `crates/warp_server_client`, `crates/managed_secrets`, `crates/onboarding`, `crates/voice_input`, `crates/command-signatures-v2`
- `crates/ai/src/index/full_source_code_embedding/sync_client.rs`, `store_client.rs`
- `crates/graphql/src/client.rs`, `crates/graphql/src/managed_secrets.rs`, `crates/graphql/src/lib.rs`

**Verification + docs**
- `vendor/warp-stripped/scripts/audit-no-cloud.sh` — new
- `vendor/warp-stripped/OMW_LOCAL_BUILD.md` — append note about expanded `omw_local`
- `specs/test-plan.md` — append release-checklist smoke step

---

## Conventions

Throughout this plan:

- **Build command:** `cd vendor/warp-stripped && PROTOC=/opt/homebrew/bin/protoc cargo build -p warp --bin warp-oss --features omw_local`
- **Reverse build:** `cd vendor/warp-stripped && PROTOC=/opt/homebrew/bin/protoc cargo build -p warp --bin warp-oss` (no flag — proves upstream restorability)
- **Cargo source:** all sources are inside `vendor/warp-stripped/`; paths in this plan are relative to that root unless they start with `docs/` or `specs/`.
- **Commits:** small, one-task-per-commit. Style matches existing repo: short imperative subject (≤72 chars), no trailer beyond the existing Co-Authored-By line if running under Claude.

---

## Task 1: Baseline + audit script

**Files:**
- Create: `vendor/warp-stripped/scripts/audit-no-cloud.sh`
- Reference: existing `vendor/warp-stripped/target/debug/warp-oss` from earlier build

- [ ] **Step 1: Confirm a baseline `warp-oss` exists.**

Run: `ls -la /Users/caijiaqi/Documents/GitHub/oh-my-warp/vendor/warp-stripped/target/debug/warp-oss`
Expected: file exists (`-rwxr-xr-x ... ~700M ...`). If not, build it: `cd vendor/warp-stripped && PROTOC=/opt/homebrew/bin/protoc cargo build -p warp --bin warp-oss --features omw_local`

- [ ] **Step 2: Write the audit script.**

Create `vendor/warp-stripped/scripts/audit-no-cloud.sh`:

```bash
#!/usr/bin/env bash
# audit-no-cloud.sh — verify a warp-oss build has no Warp-cloud or firebase strings.
#
# Usage: audit-no-cloud.sh [path/to/warp-oss]
# Defaults to vendor/warp-stripped/target/debug/warp-oss relative to repo root.
# Exits 0 if all forbidden hostnames have zero hits; exits 1 otherwise.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DEFAULT_BIN="${SCRIPT_DIR}/../target/debug/warp-oss"
BIN="${1:-$DEFAULT_BIN}"

if [[ ! -x "$BIN" ]]; then
  echo "audit-no-cloud: binary not found or not executable: $BIN" >&2
  exit 2
fi

# Hostnames the omw_local build must NOT contain.
PATTERNS=(
  "app.warp.dev"
  "api.warp.dev"
  "cloud.warp.dev"
  "firebase.googleapis.com"
  "firebaseio.com"
  "identitytoolkit.googleapis.com"
)

fail=0
for pat in "${PATTERNS[@]}"; do
  count=$(strings "$BIN" | grep -c -F "$pat" || true)
  printf "%-40s %d\n" "$pat" "$count"
  if [[ "$count" -gt 0 ]]; then
    fail=1
  fi
done

if [[ "$fail" -ne 0 ]]; then
  echo "audit-no-cloud: FAIL — forbidden hostnames present in $BIN" >&2
  exit 1
fi

echo "audit-no-cloud: OK"
```

- [ ] **Step 3: Make it executable and capture baseline.**

Run:
```bash
chmod +x vendor/warp-stripped/scripts/audit-no-cloud.sh
vendor/warp-stripped/scripts/audit-no-cloud.sh | tee /tmp/omw-audit-baseline.txt || true
```
Expected: non-zero counts on most patterns; the script exits 1. Save the baseline output to `/tmp/omw-audit-baseline.txt` for comparison after each subsequent task.

- [ ] **Step 4: Commit.**

```bash
git add vendor/warp-stripped/scripts/audit-no-cloud.sh
git commit -m "Add audit-no-cloud.sh for omw_local strip verification"
```

---

## Task 2: Reinforce launch-time onboarding gate with `#[cfg]`

The existing runtime gate at `app/src/root_view.rs:1771` already routes `omw_local` builds straight to the terminal. We reinforce it with a build-time `#[cfg]` so the upstream onboarding view types are not even linked in.

**Files:**
- Modify: `vendor/warp-stripped/app/src/root_view.rs:1676–1794`

- [ ] **Step 1: Read the current dispatcher.**

Read `vendor/warp-stripped/app/src/root_view.rs` lines 1676–1794. The decision tree at `let auth_onboarding_state = if auth_state.is_logged_in() { ... } else { cfg_if! { ... } }` selects between `Terminal`, `Auth`, `Onboarding`, `LoginSlide`. Under `omw_local` we want the only reachable variant to be `Terminal`.

- [ ] **Step 2: Wrap the cfg_if's else branch.**

Inside the existing `cfg_if!` block, locate the non-wasm arm (lines ~1762–1792). Replace its body with:

```rust
} else {
    #[cfg(feature = "omw_local")]
    {
        // omw_local: no signup, onboarding, or login slides. Go straight to terminal.
        AuthOnboardingState::Terminal(workspace_args.create_workspace(ctx))
    }
    #[cfg(not(feature = "omw_local"))]
    {
        // ... existing has_completed_local_onboarding / should_show_pre_login_onboarding /
        // ChannelState / FeatureFlag::ForceLogin / FeatureFlag::SkipFirebaseAnonymousUser logic
        // unchanged from before.
    }
}
```

Preserve the existing `(target_family = "wasm")` arm above unchanged.

- [ ] **Step 3: Verify both builds compile.**

Run:
```bash
cd vendor/warp-stripped && PROTOC=/opt/homebrew/bin/protoc cargo build -p warp --bin warp-oss --features omw_local 2>&1 | tail -5
cd vendor/warp-stripped && PROTOC=/opt/homebrew/bin/protoc cargo build -p warp --bin warp-oss 2>&1 | tail -5
```
Expected: both finish with `Finished 'dev' profile`. If the no-feature build complains about an unused import (`AgentOnboardingView`, `LoginSlideView`, `OnboardingTutorial`, etc.), gate the import with `#[cfg(not(feature = "omw_local"))]` near the top of the file.

- [ ] **Step 4: Commit.**

```bash
git add vendor/warp-stripped/app/src/root_view.rs
git commit -m "Gate AuthOnboardingState dispatcher under omw_local"
```

---

## Task 3: Replace AI panel mount with placeholder under `omw_local`

The AI panel is unconditionally instantiated in `Workspace::new()` via `build_ai_assistant_panel_view()`. Under `omw_local` we want the placeholder text from spec §4.3 instead of upstream content.

**Files:**
- Modify: `vendor/warp-stripped/app/src/workspace/view.rs` lines 1491–1504, 954, 2780–2781

- [ ] **Step 1: Read the relevant blocks.**

Read three regions of `app/src/workspace/view.rs`:
1. Around line 954 (`ai_assistant_panel: ViewHandle<AIAssistantPanelView>`) — the field declaration.
2. Lines 1491–1504 (`fn build_ai_assistant_panel_view`).
3. Lines 2776–2790 (the `Workspace::new()` site that calls it).

- [ ] **Step 2: Add a placeholder builder behind the feature flag.**

In `app/src/workspace/view.rs`, immediately after the existing `build_ai_assistant_panel_view` function (line ~1504), add:

```rust
#[cfg(feature = "omw_local")]
fn build_ai_assistant_panel_view_placeholder(
    ctx: &mut ViewContext<Workspace>,
) -> ViewHandle<AIAssistantPanelView> {
    // Placeholder rendered when omw_local is on. The agent/AI panel structure
    // is preserved for v0.3 omw-server integration, but no upstream cloud or
    // sign-in surfaces are reachable here.
    //
    // Copy is intentionally short and points at the omw CLI, which is the v0.1
    // entry point for configuring providers (PRD §13).
    AIAssistantPanelView::new_omw_placeholder(
        "AI is unavailable in this build. \
         Configure providers via `omw provider add` in your terminal — \
         full omw integration is coming in v0.3.",
        ctx,
    )
}
```

Then in `crates/ai/...` or wherever `AIAssistantPanelView` is defined, add a corresponding `pub fn new_omw_placeholder(message: &str, ctx: &mut ViewContext<Self>) -> ViewHandle<Self>` constructor that builds an empty panel rendering only the message string. (The Explore agent located the panel struct in `crates/ai/src/agent/mod.rs` and related; the executor reads the file and adds the constructor following the same `new(...)` pattern already there.)

- [ ] **Step 3: Switch the `Workspace::new` call site.**

In `Workspace::new` around line 2780, replace the unconditional call:

```rust
let ai_assistant_panel = build_ai_assistant_panel_view(ctx);
```

with:

```rust
#[cfg(feature = "omw_local")]
let ai_assistant_panel = build_ai_assistant_panel_view_placeholder(ctx);
#[cfg(not(feature = "omw_local"))]
let ai_assistant_panel = build_ai_assistant_panel_view(ctx);
```

- [ ] **Step 4: Build and visually verify.**

Run:
```bash
cd vendor/warp-stripped && PROTOC=/opt/homebrew/bin/protoc cargo build -p warp --bin warp-oss --features omw_local 2>&1 | tail -5
./target/debug/warp-oss
```
Expected: launch shows no signup wall; opening the AI panel sidebar shows only the placeholder message.

- [ ] **Step 5: Commit.**

```bash
git add vendor/warp-stripped/app/src/workspace/view.rs vendor/warp-stripped/crates/ai/
git commit -m "Replace AI panel with omw_local placeholder"
```

---

## Task 4: Gate `OnboardingCalloutView` sign-in callouts

These are the in-workspace nudges ("Sign in to use AI features", "Sign in to sync") that fire after launch even when omw_local hides the launch wall.

**Files:**
- Modify: `vendor/warp-stripped/app/src/terminal/view.rs` (around the `OnboardingCalloutView` import at line ~26 — use `grep -n OnboardingCalloutView` to find all sites)
- Modify: `vendor/warp-stripped/app/src/workspace/view.rs:19511` — `redirect_to_sign_in()`

- [ ] **Step 1: Enumerate callout sites.**

Run:
```bash
grep -n 'OnboardingCalloutView\|redirect_to_sign_in\|render_sign_in_row' vendor/warp-stripped/app/src/ -r --include='*.rs'
```
Save the list. Each hit is either an import or a use site.

- [ ] **Step 2: Gate every use site.**

For each `OnboardingCalloutView::new(...)` instantiation found in step 1, wrap with `#[cfg(not(feature = "omw_local"))]` on the surrounding function or replace the body with `None` / `Default::default()` under `omw_local`. Use whichever shape fits the parent — the executor reads the surrounding 10–20 lines and chooses.

For `redirect_to_sign_in()` at `app/src/workspace/view.rs:19511`, gate the function body so under `omw_local` it returns immediately:

```rust
fn redirect_to_sign_in(&mut self, ctx: &mut ViewContext<Self>) {
    #[cfg(feature = "omw_local")]
    {
        let _ = ctx;
        return;
    }
    #[cfg(not(feature = "omw_local"))]
    {
        // ... existing body unchanged ...
    }
}
```

- [ ] **Step 3: Build and verify.**

Run:
```bash
cd vendor/warp-stripped && PROTOC=/opt/homebrew/bin/protoc cargo build -p warp --bin warp-oss --features omw_local 2>&1 | tail -5
```
Expected: build succeeds. Launch warp-oss; trigger flows that previously surfaced sign-in callouts (e.g. opening the agent input). No callouts should appear.

- [ ] **Step 4: Commit.**

```bash
git add vendor/warp-stripped/app/src/terminal/ vendor/warp-stripped/app/src/workspace/view.rs vendor/warp-stripped/app/src/auth/
git commit -m "Gate sign-in callouts and redirect under omw_local"
```

---

## Task 5: Gate cloud/AI settings tabs

Most `SettingsSection` variants are AI/cloud-shaped. Under `omw_local` we keep only the non-cloud ones.

**Files:**
- Modify: `vendor/warp-stripped/app/src/settings_view/mod.rs:188–225`
- Modify: `vendor/warp-stripped/app/src/settings_view/settings_page.rs` (around lines 1200, 1203 — tab registration)

- [ ] **Step 1: Identify keep vs gate sets.**

From `mod.rs:188–225`, the variant disposition is:

| Variant | Disposition |
|---------|-------------|
| `About`, `MCPServers`, `Appearance`, `Features`, `Keybindings`, `Privacy`, `Code`, `CodeIndexing`, `EditorAndCodeReview` | **Keep** unconditionally |
| `Account`, `BillingAndUsage`, `Teams`, `WarpDrive`, `Warpify`, `Referrals`, `SharedBlocks`, `CloudEnvironments`, `OzCloudAPIKeys` | **Gate** with `#[cfg(not(feature = "omw_local"))]` |
| `AI`, `WarpAgent`, `AgentProfiles`, `AgentMCPServers`, `Knowledge`, `ThirdPartyCLIAgents` | **Gate**; v0.3 will re-enable with omw routing |

- [ ] **Step 2: Gate the enum variants.**

In `mod.rs`, edit the `SettingsSection` enum. Add `#[cfg(not(feature = "omw_local"))]` immediately above each gated variant. Example:

```rust
pub enum SettingsSection {
    About,
    #[cfg(not(feature = "omw_local"))]
    #[default]
    Account,
    MCPServers,
    #[cfg(not(feature = "omw_local"))]
    BillingAndUsage,
    Appearance,
    // ...
}
```

If `Account` was the `#[default]`, the executor must move `#[default]` to a non-gated variant such as `About` (or `MCPServers`) under `omw_local`. Use a `cfg`-conditional `#[default]` if the toolchain supports it; otherwise duplicate the enum behind `#[cfg]`.

- [ ] **Step 3: Gate matching tab registrations and page builders.**

In `app/src/settings_view/settings_page.rs` find every match arm or vec entry that references a gated variant and wrap with `#[cfg(not(feature = "omw_local"))]`. The executor uses `grep -n SettingsSection:: vendor/warp-stripped/app/src/settings_view/` to enumerate.

- [ ] **Step 4: Build, fix exhaustiveness errors, repeat.**

Run:
```bash
cd vendor/warp-stripped && PROTOC=/opt/homebrew/bin/protoc cargo build -p warp --bin warp-oss --features omw_local 2>&1 | tail -30
```
Expected: errors of the form `match arm not exhaustive` or `unused import`. Fix each by gating the corresponding match arm or import. Repeat until it builds clean.

- [ ] **Step 5: Verify reverse build.**

Run:
```bash
cd vendor/warp-stripped && PROTOC=/opt/homebrew/bin/protoc cargo build -p warp --bin warp-oss 2>&1 | tail -10
```
Expected: clean build, proves we didn't break the upstream-restorability path.

- [ ] **Step 6: Commit.**

```bash
git add vendor/warp-stripped/app/src/settings_view/
git commit -m "Gate cloud and AI settings tabs under omw_local"
```

---

## Task 6: Gate "Sign in to Warp" / community menu items

**Files:**
- Modify: `vendor/warp-stripped/app/src/app_menus.rs:963–978` (Help menu) and other menu builders found by grep

- [ ] **Step 1: Enumerate.**

Run:
```bash
grep -n 'sign\|Sign\|warp\.dev' vendor/warp-stripped/app/src/app_menus.rs
```

- [ ] **Step 2: Replace runtime checks with build-time gates where appropriate.**

In `make_new_help_menu()` lines 963–978, the existing `if ChannelState::official_cloud_services_enabled()` already excludes the Slack/feedback items in omw_local builds. Reinforce by replacing with a `#[cfg(not(feature = "omw_local"))]` block so the item-pushing code isn't even compiled in. Example:

```rust
fn make_new_help_menu() -> Menu {
    let mut items = vec![
        link_menu_item("Warp Documentation...", links::USER_DOCS_URL.into()),
        link_menu_item("GitHub Issues...", links::GITHUB_ISSUES_URL.into()),
    ];

    #[cfg(not(feature = "omw_local"))]
    {
        items.insert(0, feedback_menu_item());
        items.push(link_menu_item("Warp Slack Community...", links::SLACK_URL.into()));
    }

    Menu::new("Help", items)
}
```

- [ ] **Step 3: Audit the rest of `app_menus.rs` for similar `cloud_services_enabled` patterns and convert each to `#[cfg(not(feature = "omw_local"))]` where the gated body has no `omw_local` counterpart.**

- [ ] **Step 4: Build and verify.**

Run the standard build command. Launch and check the Help menu only contains "Documentation" and "GitHub Issues" entries.

- [ ] **Step 5: Commit.**

```bash
git add vendor/warp-stripped/app/src/app_menus.rs
git commit -m "Gate Warp-cloud menu items under omw_local"
```

---

## Task 7: Make cloud-only crates `optional`, exclude from `omw_local`

**Files:**
- Modify: `vendor/warp-stripped/Cargo.toml` (workspace dependency table)
- Modify: `vendor/warp-stripped/app/Cargo.toml` (lines 81, 103, 223, 226, 234, 251, 719)
- Modify: `vendor/warp-stripped/app/src/server/server_api/auth.rs:6`
- Modify: `vendor/warp-stripped/app/src/cloud_object/mod.rs:76`
- Modify: `vendor/warp-stripped/app/src/lib.rs:269`
- Modify: `vendor/warp-stripped/app/src/root_view.rs:29–31` (onboarding imports)
- Modify: `vendor/warp-stripped/app/src/root_view.rs:3243` (voice_input)

- [ ] **Step 1: Mark crates `optional = true` in `app/Cargo.toml`.**

Edit `vendor/warp-stripped/app/Cargo.toml`. For each of the following lines, add or confirm `optional = true`:

| Line | Crate | Already optional? |
|------|-------|-------------------|
| 81  | `command-signatures-v2` | yes (per Explore audit) |
| 103 | `firebase` | confirm; if no, add |
| 223 | `warp_server_client` | add |
| 226 | `voice_input` | yes |
| 234 | `onboarding` | confirm; if no, add |
| 251 | `warp_managed_secrets` | add |

After editing, the dependency lines should look like e.g.:

```toml
firebase = { workspace = true, optional = true }
warp_server_client = { workspace = true, optional = true }
warp_managed_secrets = { workspace = true, optional = true }
onboarding = { workspace = true, optional = true }
```

- [ ] **Step 2: Update `omw_local` feature definition.**

In `app/Cargo.toml:719` change:

```toml
omw_local = ["skip_firebase_anonymous_user", "warp_core/omw_local"]
```

to:

```toml
omw_local = ["skip_firebase_anonymous_user", "warp_core/omw_local"]
# Note: the cloud crates (firebase, warp_server_client, warp_managed_secrets,
# onboarding, voice_input, command-signatures-v2) are intentionally NOT listed
# as features here. omw_local builds exclude them.
```

Define a complementary `cloud` feature that pulls them in, and add to `default = [...]`:

```toml
cloud = ["dep:firebase", "dep:warp_server_client", "dep:warp_managed_secrets", "dep:onboarding", "dep:voice_input", "dep:command-signatures-v2"]
```

Update the existing `default = [...]` line to include `"cloud"`. Confirm which existing default features are present before editing — preserve them.

- [ ] **Step 3: Gate import sites.**

For each import site listed under "Files" above, add `#[cfg(not(feature = "omw_local"))]` (or, equivalently, `#[cfg(feature = "cloud")]` — pick one consistently across this task) above the `use` statement and any callers that still reference the imported types directly.

The executor uses these greps to find all callers:

```bash
for c in firebase warp_server_client warp_managed_secrets onboarding voice_input; do
  echo "=== $c ==="
  grep -nR "^use $c\|^use crate::.*$c\|::${c}::" vendor/warp-stripped/app/src/ --include='*.rs' | head -30
done
```

For every hit, gate the `use` and the surrounding function body if it would otherwise reference a now-removed type.

- [ ] **Step 4: Build with `omw_local`, fix unresolved-import errors iteratively.**

Run:
```bash
cd vendor/warp-stripped && PROTOC=/opt/homebrew/bin/protoc cargo build -p warp --bin warp-oss --features omw_local 2>&1 | grep -E '^(error|warning)' | head -40
```
Expected: errors of the form `cannot find type X in this scope` / `unresolved import`. Each error points to a use site that needs gating. Repeat until clean.

- [ ] **Step 5: Verify reverse build (with `cloud` default features).**

Run:
```bash
cd vendor/warp-stripped && PROTOC=/opt/homebrew/bin/protoc cargo build -p warp --bin warp-oss 2>&1 | tail -10
```
Expected: clean.

- [ ] **Step 6: Commit.**

```bash
git add vendor/warp-stripped/app/Cargo.toml vendor/warp-stripped/Cargo.toml vendor/warp-stripped/app/src/
git commit -m "Make cloud crates optional and exclude them from omw_local"
```

---

## Task 8: Gate cloud-AI provider modules in `crates/ai`

**Files:**
- Modify: `vendor/warp-stripped/crates/ai/src/index/full_source_code_embedding/sync_client.rs`
- Modify: `vendor/warp-stripped/crates/ai/src/index/full_source_code_embedding/store_client.rs`
- Modify: `vendor/warp-stripped/crates/ai/src/index/mod.rs` (or wherever the embedding submodules are declared)
- Modify: `vendor/warp-stripped/app/src/ai/cloud_environments/mod.rs:3`

- [ ] **Step 1: Identify the embedding submodule declaration.**

Run:
```bash
grep -n 'mod full_source_code_embedding\|mod sync_client\|mod store_client' vendor/warp-stripped/crates/ai/src/ -r
```

- [ ] **Step 2: Gate the cloud-routing submodules at their declaration site.**

In whichever `mod.rs` declares them, wrap the `mod sync_client;` / `mod store_client;` lines with `#[cfg(not(feature = "omw_local"))]`:

```rust
#[cfg(not(feature = "omw_local"))]
mod sync_client;
#[cfg(not(feature = "omw_local"))]
mod store_client;
```

If `crates/ai`'s own `Cargo.toml` does not yet have an `omw_local` feature, add one:

```toml
[features]
omw_local = []
```

and propagate it from `app/Cargo.toml`'s `omw_local` definition by adding `"ai/omw_local"` to the feature list.

- [ ] **Step 3: Gate the `cloud_environments` import.**

In `app/src/ai/cloud_environments/mod.rs:3` (`use warp_server_client::cloud_object::Owner;`), gate the `use` and the surrounding module declaration with `#[cfg(not(feature = "omw_local"))]`. If `cloud_environments` is itself only useful with cloud, gate the entire submodule declaration in its parent.

- [ ] **Step 4: Build and fix errors iteratively.**

Same pattern as Task 7 step 4.

- [ ] **Step 5: Commit.**

```bash
git add vendor/warp-stripped/crates/ai/ vendor/warp-stripped/app/src/ai/
git commit -m "Gate cloud-routing AI provider modules under omw_local"
```

---

## Task 9: Gate the GraphQL API client; keep schema types

**Files:**
- Modify: `vendor/warp-stripped/crates/graphql/src/lib.rs` (re-exports)
- Modify: `vendor/warp-stripped/crates/graphql/src/client.rs`
- Modify: `vendor/warp-stripped/crates/graphql/src/managed_secrets.rs`
- Modify: `vendor/warp-stripped/crates/graphql/Cargo.toml`

- [ ] **Step 1: Add `omw_local` feature to graphql crate.**

In `crates/graphql/Cargo.toml`:

```toml
[features]
default = []
omw_local = []
```

Propagate from `app/Cargo.toml`'s `omw_local` definition by adding `"warp_graphql/omw_local"` (use the actual crate name from its Cargo.toml `[package].name`).

- [ ] **Step 2: Gate `client.rs` and `managed_secrets.rs` at the `mod` declaration in `lib.rs`.**

In `crates/graphql/src/lib.rs`:

```rust
#[cfg(not(feature = "omw_local"))]
pub mod client;
#[cfg(not(feature = "omw_local"))]
pub mod managed_secrets;

// Keep schema and api submodules unconditional so non-cloud code that depends
// on type definitions still compiles.
pub mod api;
```

- [ ] **Step 3: Find and gate every external caller of `graphql::client` / `graphql::managed_secrets`.**

Run:
```bash
grep -nR 'use warp_graphql::client\|use warp_graphql::managed_secrets\|graphql::client::\|graphql::managed_secrets::' vendor/warp-stripped/ --include='*.rs'
```

For each hit, wrap the use site under `#[cfg(not(feature = "omw_local"))]`.

- [ ] **Step 4: Build, iterate, verify.**

Standard build command. Fix any `unresolved import` errors by gating the caller. Run the reverse build to confirm we didn't break upstream.

- [ ] **Step 5: Commit.**

```bash
git add vendor/warp-stripped/crates/graphql/ vendor/warp-stripped/app/
git commit -m "Gate GraphQL cloud client under omw_local"
```

---

## Task 10: Run the audit script; fix remaining hits

By this point most cloud strings should be gone. The audit script tells us what's left.

**Files:** none directly. Triage-and-edit driven by audit output.

- [ ] **Step 1: Build a fresh `omw_local` binary and run the audit.**

```bash
cd vendor/warp-stripped && PROTOC=/opt/homebrew/bin/protoc cargo build -p warp --bin warp-oss --features omw_local 2>&1 | tail -3
vendor/warp-stripped/scripts/audit-no-cloud.sh
```
Expected: Some patterns may still have non-zero counts. Each non-zero pattern points at a remaining cloud-shaped string in the binary.

- [ ] **Step 2: For each remaining hit, find its source.**

```bash
# example for a remaining hit on "app.warp.dev"
grep -rn 'app\.warp\.dev' vendor/warp-stripped/crates/ vendor/warp-stripped/app/ --include='*.rs'
```

Each hit is one of: a `const` URL, a default config value, a comment in compiled-in source, an unreachable code path, or test fixtures. For non-test hits, gate the surrounding declaration. For test hits, leave them — the audit script's `strings | grep` will still flag them, so update the script's test-fixture allow-list (add a `--exclude-fixtures` flag or hardcoded allow-list).

- [ ] **Step 3: Iterate until `audit-no-cloud.sh` exits 0.**

Each gating change is small. Commit per pattern resolved if the diff is large; otherwise batch.

- [ ] **Step 4: Final audit pass.**

```bash
vendor/warp-stripped/scripts/audit-no-cloud.sh
```
Expected: `audit-no-cloud: OK`, exit 0.

- [ ] **Step 5: Commit any remaining gates.**

```bash
git add vendor/warp-stripped/
git commit -m "Gate remaining Warp-cloud constants under omw_local"
```

---

## Task 11: Update build doc and test plan

**Files:**
- Modify: `vendor/warp-stripped/OMW_LOCAL_BUILD.md` (the macOS section just added in a prior commit; append a "What `omw_local` covers" subsection)
- Modify: `specs/test-plan.md` (release-checklist section)

- [ ] **Step 1: Update `OMW_LOCAL_BUILD.md`.**

Append a new section after the macOS troubleshooting block:

```markdown
## What `omw_local` covers

Building with `--features omw_local` produces a `warp-oss` that has none of the
upstream Warp cloud or AI surfaces. Specifically, this build:

- Has no signup, login, or onboarding wall on launch
- Hides the Account / BillingAndUsage / Teams / WarpDrive / AI settings tabs
- Replaces the AI panel with a placeholder pointing at the omw CLI
- Excludes the firebase, warp_server_client, warp_managed_secrets, onboarding,
  voice_input, and command-signatures-v2 crates from the link
- Excludes the GraphQL cloud client (schema types are kept)
- Has zero references to `app.warp.dev`, `api.warp.dev`, `firebase.googleapis.com`,
  etc. (verified by `vendor/warp-stripped/scripts/audit-no-cloud.sh`)

Building without the flag (`cargo build -p warp --bin warp-oss`) restores
upstream behavior — useful for upstream-rebase verification.
```

- [ ] **Step 2: Update `specs/test-plan.md`.**

Locate the release-checklist section (§7 per CLAUDE.md). Add a checklist item:

```markdown
- [ ] **`omw_local` strip audit.** After building `warp-oss` with `--features omw_local`,
  run `vendor/warp-stripped/scripts/audit-no-cloud.sh` and confirm `audit-no-cloud: OK`.
  Then launch the binary and exercise normal terminal use for 60 seconds while running
  `nettop -m route -c 5 -l 60 | grep -E 'warp\.dev|firebase'` in another terminal —
  expect zero matches.
```

- [ ] **Step 3: Commit docs.**

```bash
git add vendor/warp-stripped/OMW_LOCAL_BUILD.md specs/test-plan.md
git commit -m "Document expanded omw_local coverage and release smoke step"
```

---

## Task 12: Final end-to-end verification

**Files:** none.

- [ ] **Step 1: Clean build with feature.**

```bash
cd vendor/warp-stripped && PROTOC=/opt/homebrew/bin/protoc cargo build -p warp --bin warp-oss --features omw_local 2>&1 | tee /tmp/omw-strip-final.log | tail -10
grep -c '^warning' /tmp/omw-strip-final.log
grep -c '^error' /tmp/omw-strip-final.log
```
Expected: 0 errors. Warning count: same as the baseline build (no new warnings introduced by this work).

- [ ] **Step 2: Reverse build.**

```bash
cd vendor/warp-stripped && PROTOC=/opt/homebrew/bin/protoc cargo build -p warp --bin warp-oss 2>&1 | tail -10
```
Expected: clean build. Proves reversibility.

- [ ] **Step 3: Audit script.**

```bash
vendor/warp-stripped/scripts/audit-no-cloud.sh
```
Expected: `audit-no-cloud: OK`, exit 0.

- [ ] **Step 4: Manual smoke test.**

In one terminal:
```bash
sudo nettop -m route -c 5 -l 60 -p $(pgrep -nx warp-oss) > /tmp/omw-network.log &
```

In another:
```bash
./vendor/warp-stripped/target/debug/warp-oss
```

Use the terminal normally for ~60 seconds: open a new tab, run `ls`, run a multi-line command, open settings (verify no AI/cloud tabs), open the AI panel sidebar (verify placeholder text). Then close `warp-oss`.

Inspect:
```bash
grep -E 'warp\.dev|firebase' /tmp/omw-network.log
```
Expected: zero matches.

- [ ] **Step 5: Final commit (if any).**

If any small follow-up edits surfaced from the smoke test, commit them now. Otherwise nothing to commit; this task is the gate.

---

## Self-review checklist (run after writing all tasks)

- [x] **Spec coverage.** Spec §4.1 → Tasks 2, 4, 5, 6. Spec §4.2 → Tasks 7, 8, 9. Spec §4.3 → Task 3. Spec §6 (verification) → Tasks 1, 10, 12. Spec §10 (acceptance) → Task 12.
- [x] **No "TBD"/"fill in"/"add appropriate" placeholders.** Every step has a concrete command or code shape; tasks that depend on audit output (Tasks 4, 7, 8, 9, 10) have a step that produces the audit before the gate step.
- [x] **Type/symbol consistency.** `AuthOnboardingState`, `SettingsSection`, `build_ai_assistant_panel_view`, `build_ai_assistant_panel_view_placeholder`, `redirect_to_sign_in`, `OnboardingCalloutView`, `audit-no-cloud.sh` are referenced consistently.
- [x] **Reversible at every task.** Every task includes (or is followed by Task 12) a reverse-build verification step.
