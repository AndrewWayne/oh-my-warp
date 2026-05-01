# Strip Residual Signup / Warp-Brand UI from `vendor/warp-stripped` — Design

- **Date:** 2026-05-01
- **Topic:** Eliminate the remaining user-facing signup CTAs, "Welcome to Warp" surfaces, and `warpdotdev/warp` URLs that survived the cloud-strip cascade and are still visible in the `omw-warp-oss` preview build.
- **Owner:** Jiaqi Cai
- **Related:** [2026-05-01 strip-built-in-ai design](./2026-05-01-strip-built-in-ai-design.md), [PRD §3.1](../../../PRD.md#31-v10-committed-scope), [specs/fork-strategy.md §2](../../../specs/fork-strategy.md), [CLAUDE.md §5](../../../CLAUDE.md#5-project-specific-rules), [TODO.md](../../../TODO.md)

## 1. Motivation

The `omw-local-preview-v0.0.1` build (`omw-warp-oss.app`) was supposed to ship without cloud or AI surfaces. After install we still see:

1. **Inline banner** above the prompt asking the user to sign up — clicking the button drops them at a `127.0.0.1` callback (the dead-end OAuth redirect, since the cloud URLs were emptied).
2. **Settings → Account page** showing a "Sign up" CTA with a free-tier badge and an "Upgrade" link.
3. **Settings → About page** that only shows logo + version + "Copyright 2026 Warp" — not useful as an "about this build" surface.
4. **Help menu** with a broken "Warp Documentation..." entry (`USER_DOCS_URL == ""`) and a "GitHub Issues..." entry pointing at upstream `warpdotdev/Warp`.
5. **Get Started tab** ("New Tab → Get Started") titled `"Welcome to Warp"` with the `"The Agentic Development Environment"` tagline.
6. **OpenWarp launch modal** (`"Warp is now open-source"` + `"Visit the repo"` → `https://github.com/warpdotdev/warp`) — currently *unreachable* in omw_local because it is gated on a fully-onboarded auth state, but the strings and the upstream URL are compiled into the binary and the modal *would* fire if the auth gate ever flipped.
7. **Compiled-in dead strings** in auth/billing flows that name "Warp" but are not reachable for an anonymous omw_local user (`auth_view_body`, `needs_sso_link_view`, `auth_override_warning_body`, `build_plan_migration_modal`, `workspace/home.rs`, the wasm tab-bar warp logo).
8. **Constants** — `util/links.rs::GITHUB_ISSUES_URL` still hard-codes upstream issues; the `"Toggle Warp AI"` keybinding label still ships in source.

The user has authorized addressing all of the above for the v0.0.2 preview.

## 2. Decisions (from brainstorming)

| Axis | Choice |
|---|---|
| Scope | Items 1–8 above. Tiered: A (visible) → B (compiled-in dead strings + constants). |
| Strategy | `#[cfg(feature = "omw_local")]` gating on copy + Cargo feature subtraction for the OpenWarp launch modal trigger. Source-level changes only in `vendor/warp-stripped/`. Reversible. |
| Banner UX | Keep the inline banner format; rewrite copy and drop the Sign Up button (close-only). |
| About page UX | Rebuild under cfg: app description, link to upstream Warp, link to oh-my-warp, embedded scrollable AGPL license text. |
| Help menu UX | Under cfg: replace the two existing items with "Project on GitHub..." and "Report an Issue..." both pointing at `AndrewWayne/oh-my-warp`. |
| OpenWarpLaunchModal | Strip the `open_warp_launch_modal` feature from `omw_default` so the launch flag is never registered. Source of the modal stays compiled in (we do not gate the module) — strings remain in binary as dead, but the modal can never trigger. |
| Constants & dead strings | Cfg-gate `GITHUB_ISSUES_URL` and the `Toggle Warp AI` label. Cfg-gate the small set of user-visible "Warp" strings in unreachable auth/billing flows so that *if* a future code path reaches them, they show the omw flavor rather than upstream branding. We do **not** restructure the modules. |
| Brand rule | All new copy is lowercase `warp` or omits the word entirely, per CLAUDE.md §5. The literal `Warp` (capitalized) is only retained in upstream-attribution comments and `LICENSE` files. |
| Repo URL | `https://github.com/AndrewWayne/oh-my-warp` |

Rejected approaches:
- **Module-level `#[cfg(not(feature = "omw_local"))]` gates** on `auth_view_modal`, `openwarp_launch_modal`, `build_plan_migration_modal` — would force gating every import site, sharply increasing fork delta and upstream-sync cost (specs/fork-strategy.md §2). The string-level cfg gates we propose remove the brand strings from the user's eyes for the same effective outcome at much smaller cost.
- **Edit `MIT`/`AGPL` source-of-truth directly to swap `Warp` for `omw`** — this is forbidden by CLAUDE.md §5 (must preserve upstream attribution). License text on the About page is the verbatim repo `LICENSE` file, which already contains both omw and upstream copyright lines.

## 3. Architecture

All edits are additive cfg-gates inside `vendor/warp-stripped/`. No new files, no new crates, no public API changes.

```
omw_local ON  →  warp-oss binary shows omw-flavored copy and zero signup/AI/upstream-Warp CTAs.
omw_local OFF →  upstream Warp behavior unchanged (preserves fork delta).
```

The default build (cloud-enabled) is untouched. `omw_default` shrinks by one entry: `open_warp_launch_modal` is removed.

## 4. Detailed plan

### 4.1 Tier A — visible surfaces

**A1. Inline "Login for AI" banner.**

File: `vendor/warp-stripped/app/src/terminal/view/inline_banner/anonymous_user_ai_sign_up.rs`

- Add `#[cfg(feature = "omw_local")]` arms for `TITLE` and `CONTENT`:
  - `TITLE` → `"Welcome to omw (oh-my-warp)"`
  - `CONTENT` → `"Project built on the open source warp terminal. AI is disabled in this build."`
- In `render_three_column_inline_banner`, gate the Sign Up button render with `#[cfg(not(feature = "omw_local"))]` so under omw_local only the close button renders.
- Leave the `SignUp` action variant and its handler in the enum (dead under omw_local — already-existing pattern from cloud-strip cascade; warning suppression follows the same `#[cfg_attr(feature = "omw_local", allow(unused))]` pattern used in `terminal/general_settings.rs`).

**A2. Settings → main_page (Account) Sign up button.**

File: `vendor/warp-stripped/app/src/settings_view/main_page.rs`

- `render_anonymous_account_info` (currently lines ~317–410) builds: Sign-up button + Free-tier badge + "Compare plans" link + upgrade routing.
- Cfg-gate the entire body of `render_anonymous_account_info` so that under omw_local it returns a single muted paragraph: `"Standalone build — sign-in is disabled. See the About page for project info."`
- Click handlers (`SignupAnonymousUser`, `Upgrade`) stay defined; gated dead code under omw_local.

**A3. Settings → About page rebuild.**

File: `vendor/warp-stripped/app/src/settings_view/about_page.rs`

Under `#[cfg(feature = "omw_local")]`, replace the body of `AboutPageWidget::render` with a vertical layout:

1. Existing logo (kept — preview icons may carry transitional OSS glyphs per §5.1).
2. Existing version row (kept).
3. App name line: `"omw — oh-my-warp"`.
4. Description paragraph: `"An audit-clean local build of the open source warp terminal. Cloud, AI, and signup features are stripped."`
5. Section header: `"Acknowledgements"` followed by a paragraph: `"Built on the open source warp terminal. Source:"` + a hyperlink to `https://github.com/warpdotdev/warp` (label `"warpdotdev/warp"`).
6. Section header: `"Project home"` followed by a hyperlink to `https://github.com/AndrewWayne/oh-my-warp` (label `"AndrewWayne/oh-my-warp"`).
7. Section header: `"License"` followed by a fixed-height (≈300 px) scrollable container rendering the verbatim repo `LICENSE` file via `include_str!("../../../../../LICENSE")`. Use a monospace font, soft-wrap on, `overflow_y: scroll`.

Drop the `"Copyright 2026 Warp"` line under omw_local (already present in the embedded LICENSE).

Hyperlink rendering follows the pattern in `settings_view/platform_page.rs` (`FormattedTextElement` + `HighlightedHyperlink` + `HyperlinkClick(String)` action that calls `ctx.open_url(url)`). The page already has its own action enum we will extend with one variant.

Scrollable container: use `warpui::elements::ClippedScrollable::vertical`, the same element used by `settings_view/settings_file_footer.rs:212` and `settings_view/keybindings.rs`. Pattern requires a `ClippedScrollStateHandle` field on the widget (already familiar — `settings_file_footer.rs:92` shows the convention).

**A4. Help menu cleanup.**

File: `vendor/warp-stripped/app/src/app_menus.rs` (lines 978–995, function `make_new_help_menu`)

Restructure under cfg:

```rust
fn make_new_help_menu() -> Menu {
    #[cfg(feature = "omw_local")]
    let items = vec![
        link_menu_item("Project on GitHub...", "https://github.com/AndrewWayne/oh-my-warp".into()),
        link_menu_item("Report an Issue...",   "https://github.com/AndrewWayne/oh-my-warp/issues".into()),
    ];

    #[cfg(not(feature = "omw_local"))]
    let mut items = vec![
        link_menu_item("Warp Documentation...", links::USER_DOCS_URL.into()),
        link_menu_item("GitHub Issues...",      links::GITHUB_ISSUES_URL.into()),
    ];

    #[cfg(not(feature = "omw_local"))]
    {
        items.insert(0, feedback_menu_item());
        items.push(link_menu_item("Warp Slack Community...", links::SLACK_URL.into()));
    }

    Menu::new("Help", items)
}
```

The omw_local arm uses inline `&str` literals (no need to add new constants in `util/links.rs`).

**A5. GetStartedView "Welcome to Warp" + tagline.**

File: `vendor/warp-stripped/app/src/pane_group/pane/get_started_view.rs` (lines ~232 and ~242)

- Cfg-gate the title string: `"Welcome to Warp"` → `"Welcome to omw"` under omw_local.
- Cfg-gate the tagline: `"The Agentic Development Environment"` → `"Open-source terminal — local build"` under omw_local.

Two-line surgical edit. No structural changes.

### 4.2 Tier B — compiled-in dead strings + constants

**B1. OpenWarpLaunchModal trigger removal.**

File: `vendor/warp-stripped/app/Cargo.toml` (line 593)

Remove `"open_warp_launch_modal"` from the `omw_default` feature list. The corresponding `FeatureFlag::OpenWarpLaunchModal` registration in `app/src/lib.rs:2893` is already cfg-gated on `feature = "open_warp_launch_modal"`, so this single Cargo edit:

- Stops the modal from ever being triggered in omw_local builds.
- Leaves all source intact (no cfg-gates inside the module). Strings (`"Warp is now open-source"`, `REPO_URL`, `CONTRIBUTING_URL`, the three feature item descriptions) **remain compiled into the binary as dead code**. Acceptable trade-off: gating the whole module would touch dozens of import sites in `workspace/view.rs`, `workspace/action.rs`, `workspace/one_time_modal_model.rs`, etc., for zero user-visible benefit.

We document this as an explicit non-goal in §6.

**B2. `util/links.rs` constants.**

File: `vendor/warp-stripped/app/src/util/links.rs`

```rust
#[cfg(feature = "omw_local")]
pub const GITHUB_ISSUES_URL: &str = "https://github.com/AndrewWayne/oh-my-warp/issues";

#[cfg(not(feature = "omw_local"))]
pub const GITHUB_ISSUES_URL: &str = "https://github.com/warpdotdev/Warp/issues";
```

`feedback_form_url()` (which uses `warpdotdev/Warp/issues/new/choose`) is only called from `feedback_menu_item()`, which is already gated `#[cfg(not(feature = "omw_local"))]` (app_menus.rs:963). No change needed there.

**B3. "Toggle Warp AI" keybinding label.**

File: `vendor/warp-stripped/app/src/workspace/mod.rs` (line 1156)

Cfg-gate the label string:
- omw_local → `"Toggle AI Assistant"` (neutral).
- default → `"Toggle Warp AI"` (unchanged).

The `IS_ANY_AI_ENABLED` predicate keeps the menu item hidden in omw_local, so this is purely about removing the brand string from the binary.

**B4. Other "Warp" strings in dead-but-compiled flows.**

Cfg-gate the *minimum* set of user-visible "Warp" capitalized strings, leaving the rest of these modules untouched:

| File | Lines | Change under cfg(feature = "omw_local") |
|---|---|---|
| `auth/auth_view_body.rs` | 619, 622, 625 | "In order to use Warp's AI features..." → `""` (paragraph hidden — these strings are reached only via auth view variants that omw_local never opens) |
| `auth/auth_view_body.rs` | 651, 654, 1009 | "Welcome to Warp!" / "Sign up for Warp" → `""` |
| `auth/needs_sso_link_view.rs` | 79 | "Click the button below to link your Warp account..." → `""` |
| `auth/auth_override_warning_body.rs` | 31 | `AUTH_OVERRIDE_DESCRIPTION` → `""` |
| `workspace/view/build_plan_migration_modal.rs` | 518 | "Welcome to Warp Build" → `""` |

We **do not** touch `workspace/home.rs` (wasm-only — `cfg(target_family = "wasm")` already excludes it from native macOS builds) or `workspace/view.rs:17015` (also wasm-only). These are already invisible to the omw-warp-oss binary; gating them adds delta with zero user value.

Justification for blanking strings rather than rewriting them: these surfaces are unreachable in omw_local. We cannot easily verify a rewrite renders correctly because we cannot trigger them. Empty strings preserve module structure (no removed code, struct shapes unchanged) while removing the brand text from the compiled binary.

## 5. Brand-rule check (CLAUDE.md §5)

Every new copy string vetted:

| String | Capitalized "Warp"? | OK? |
|---|---|---|
| `"Welcome to omw (oh-my-warp)"` | no | ✓ |
| `"Project built on the open source warp terminal. AI is disabled in this build."` | no (lowercase) | ✓ |
| `"Standalone build — sign-in is disabled. See the About page for project info."` | no | ✓ |
| `"omw — oh-my-warp"` | no | ✓ |
| `"An audit-clean local build of the open source warp terminal..."` | no | ✓ |
| `"Built on the open source warp terminal. Source:"` | no | ✓ |
| `"warpdotdev/warp"` (link label) | no | ✓ (allowed: §5 explicitly permits the codename `warp` in lowercase) |
| `"Welcome to omw"` (Get Started) | no | ✓ |
| `"Open-source terminal — local build"` | no | ✓ |
| `"Toggle AI Assistant"` | no | ✓ |
| `"Project on GitHub..."` / `"Report an Issue..."` (menu labels) | no | ✓ |

License text shown on the About page is the verbatim repo `LICENSE`, which contains the literal `Warp` capitalized — this is the explicitly-allowed `LICENSE` exemption in CLAUDE.md §5.

The hyperlink to upstream uses the URL fragment `warpdotdev/warp` (lowercase — that's the actual GitHub org/repo path).

## 6. Non-goals

- **Removing OpenWarpLaunchModal source code** — see B1. Strings remain in binary as dead. Justification: fork-strategy delta cost.
- **Restructuring auth modules** — see B4. We only blank user-visible strings; struct/event/enum shapes preserved.
- **Touching `workspace/home.rs` and the wasm warp logo** — already wasm-only, invisible on macOS builds.
- **Changing the LICENSE file** — forbidden per CLAUDE.md §5.
- **Renaming the `vendor/warp-stripped/` directory or the `warp-oss` Cargo bin name** — that's a v0.3 task per [TODO.md](../../../TODO.md) and §5.1.
- **Codesigning the resulting `.dmg`** — separate v1.0 task per PRD §13.

## 7. Verification

A1–A5, B1–B4 each have explicit verification:

| Item | Build verification | Runtime verification |
|---|---|---|
| A1 | `cargo check -p warp --no-default-features --features omw_local` succeeds | Launch app → first prompt shows the new banner copy with no Sign Up button |
| A2 | Same | Open Settings → Account: only the muted "Standalone build" notice; no Sign up / Compare plans / Upgrade |
| A3 | Same; `include_str!` resolves at compile time | Open Settings → About: app name, description, two links, scrollable LICENSE text. Click each link — opens correct GitHub URL |
| A4 | Same | Help menu shows "Project on GitHub..." and "Report an Issue..." only — no "Warp Documentation...", no upstream link |
| A5 | Same | New Tab → Get Started: title says "Welcome to omw" |
| B1 | Same | Modal does not appear on first launch even after clearing `~/Library/Application Support/omw.local.warpOss/` |
| B2 | Same | (No direct user surface in omw_local — verified by `grep` after build that the binary contains the new URL but not the old) |
| B3 | Same | (Predicate already hides menu — verified by source diff) |
| B4 | Same | (Unreachable surfaces — verified by source diff and `cargo build --message-format=json` does not emit warnings) |

`vendor/warp-stripped/scripts/audit-no-cloud.sh` (per `specs/fork-strategy.md:69`) should still report zero forbidden hostnames in the resulting binary.

## 8. Risks & mitigations

| Risk | Mitigation |
|---|---|
| Cfg-gate typo breaks default-features build | Verify both feature sets build: `cargo check -p warp` (default) and `cargo check -p warp --no-default-features --features omw_local` |
| `include_str!` path drifts as `vendor/warp-stripped/app/src/settings_view/about_page.rs` is 5 levels deep | Path will be `../../../../../LICENSE`. Validate at compile time — if wrong, build fails immediately |
| Hyperlink action enum name collision in About page | Reuse the `HyperlinkClick(String)` pattern from `platform_page.rs` verbatim |
| LICENSE text rendering glitches in scrollable container | If the existing scrollable container struggles with 674-line text, fall back to a plain non-scrollable paragraph + a "Read full license" hyperlink to the GitHub LICENSE file. Decide at implementation time after one screenshot |
| Future upstream sync of `vendor/warp-stripped` collides with our cfg-gates | All cfg-gates use the same `feature = "omw_local"` flag; conflict markers will surface at the same locations; mitigation is documented in `specs/fork-strategy.md §2` |

## 9. Out of scope (for this design)

- Replacing `vendor/warp-stripped/about.hbs` (the upstream source-attribution doc) — kept as-is per CLAUDE.md §5.
- Changing `Cargo.toml` `[[bin]] warp-oss` rename to `omw-warp-oss` — packaging-time rename only (per §5.1), not source-level.
- The full "v0.3 rebrand" pass — separate roadmap item.

## 10. TODO.md & spec coupling

This design does not touch PRD §3.1 (no scope changes). Per CLAUDE.md §5 the only required cross-link is a `TODO.md` entry under the v0.0.x preview track marking the residual-signup cleanup as completed once the implementation lands. The implementation plan (separate doc) will include the TODO.md update as a step.
