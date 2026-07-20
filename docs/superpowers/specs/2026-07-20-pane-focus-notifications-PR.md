# PR: Pane-focus notifications (P1 core) — unlock OSC 9/777 desktop notifications with click-to-focus-origin-pane

Branch: `feat/pane-focus-notifications` (off `main`, isolated worktree)
Related docs (same tree): `…-requirements.md`, `…-design.md`, `…-plan.md`

## What & why

omw is positioned as an AI terminal where people run multiple agents (codex/claude/…) across panes. Today there is no reliable "an agent finished / is blocked, come back to *this* pane" signal: omw ships the whole OSC 9/777 → desktop-notification → click-to-focus-origin-pane pipeline that Warp built, but **disabled behind a double gate**, and its one behavioural knob only fired the desktop notification when you had already left the window.

This PR delivers the **P1 core**: any program that writes an OSC 9/777 notification escape raises a native desktop notification whose click focuses the exact tab+pane that emitted it. It is a *"unlock + policy"* change, not a rewrite — the delivery, click routing, and `focus_pane` path are reused unchanged.

## Commits (each compiles; see Verification)

- **MS0 ungate** (`18eb465`): add `PluggableNotifications` to `OMW_LOCAL_FLAGS` and remove it from `OMW_LOCAL_DISABLED_FLAGS`. Adding it via `additional_features()` also bypasses the `#[cfg(feature = "pluggable_notifications")]` gate. Paired regression tests (warp_features + app crate) lock the "must change both" contract.
- **MS1 always-notify** (`c259ba0`): `NotificationsSettings` gains `is_escape_sequence_enabled` (default on), `always_notify` (default on), `suppress_when_pane_foreground` (default off). The `ModelEvent::PluggableNotification` handler now defaults to *always* raising the desktop notification (event-driven), with an opt-in foreground-suppression path. New `is_pane_in_foreground` (window-level approximation for now — see Open items).
- **MS2 identifier + sound** (`75b8e17`): `UserNotification` gains optional `sound_name` + `identifier` (additive; the other 5 platform senders are untouched and keep default behavior). macOS objc/FFI (`notifications.m`/`.h` + `delegate.rs`) use `UNNotificationSound soundNamed:` and a **per-pane request identifier** (fixes cross-pane clobbering, design R4). Settings gain `FocusBehavior`, `focus_on_click`, `notification_sound_name`.
- **MS3 templates** (`468d47f`): `render_notification_template` (pure, in warpui_core, unit-tested) + `title_template`/`body_template` settings applied in the handler.
- **MS4 throttle/DND** (`f2ea521`): `respect_system_dnd` + `throttle_window_secs` settings; process-global `notification_throttled()` rate-limits identical same-pane bursts before delivery.
- **MS6 P2 agent-turn** (`7148f51`): the native agent-turn desktop path already exists and is *live* in omw (the per-view `StatusChanged` handler in `terminal/view.rs`, unlike the `agent_management_model` path which is gated on the omw-disabled `HOANotifications`). It already classifies Success→`AgentTaskCompleted` / Blocked→`NeedsAttention` and reuses the P1 delivery/click-focus. This applies the same MS1 gate treatment (default always-notify + optional foreground suppression), respecting the existing `mode` gate + per-event toggles. **This satisfies acceptance criterion ③ (completed vs needs-you) without ungating HOA.**

## Design decisions (see design doc for rationale)

- **Default-on** without touching the legacy `mode` (default `Unset`) or the unconfigured-banner path: a dedicated `is_escape_sequence_enabled` (default true) + the flag ungate.
- **P1 = single "generic" notification.** OSC 9 carries no kind, so "completed vs needs-approval" classification is deferred to P2's `CLIAgentEventType` rather than extending `Handler::pluggable_notification` (which would touch 4 impls).
- **Custom sound via `~/Library/Sounds`** (name reference), verified empirically to be resolvable by name; no packaging change.
- **Sound plumbing is additive** (`Option` fields + builders) rather than a breaking `play_sound: bool → SoundSpec` refactor, to avoid churning all 6 platform senders.

## Verification

- `cargo check -p warp --lib` — green after every milestone (includes cc-compiling the objc).
- `cargo test -p warpui_core pane_focus_notification` — 4/4 (UserNotification builders + template rendering).
- `cargo test -p warp_features omw_local_flags` — green (flag contracts).
- `cargo test -p warp --lib pane_focus_notifications_settings` (default build) — 3/3 (defaults, backward-compat, serde round-trip).
- ✅ **P1 real-machine smoke PASSED**: built `warp-oss` (omw_local), swapped into a copy of the installed `.app`; `printf '\033]9;…\007'` pops a native notification and clicking it focuses the origin pane. Confirmed on-device.
- **Auth fix found via smoke**: default `mode=Unset` never requested macOS notification authorization (it was gated on `mode==Enabled`), so default-on escape notifications were emitted but silently dropped. Fixed: `request_notification_permissions_if_needed` also fires for `is_escape_sequence_enabled`, and is invoked before delivery (idempotent).
- **Sound**: the code sets `UNNotificationSound defaultSound` whenever `play_notification_sound` (default on); if a notification appears silently, it's the per-app "Play sound for notifications" toggle in macOS System Settings (or Focus/DND), not a code path — a custom sound can be set via `[notifications] notification_sound_name`.
- ⚠️ **Test-target caveat**: `cargo test -p warp` under `--features omw_local` does not compile (157 pre-existing `SettingsSection` stripped-variant errors, unrelated); settings tests were therefore run under the default build.
- ⏳ **P2 smoke pending**: run codex/claude in the app with notifications enabled (`mode==Enabled`) → observe completed/needs-you notifications. (P2 desktop path reuses P1 delivery; the classification is already live.)

## Remaining (not in this PR)

- **MS5 settings UI**: every setting works via `settings.toml [notifications]` today (criterion ④ is functionally met); a dedicated Notifications settings *page* (mirroring `omw_agent_page.rs`, `#[cfg(feature="omw_local")]`) is deferred as discoverability polish.
- **MS7 NotificationChannel trait**: intentionally *not* added this phase — with no consumer it would be dead code, and phone/Tailscale forwarding is explicitly out of scope. The extension points are the additive `UserNotification` (`sound_name`/`identifier` + builders) and the settings enums (`FocusBehavior`, …). The existing winit delivery layer is already the platform abstraction; Linux click-routing is a follow-up.
- **Per-event custom sounds**: P1 uses a single `notification_sound_name`; a per-event `event_sounds` map lands with richer P2 kinds (deferred to avoid `SettingsValue`-on-nested-enum risk).

## Verification note (test target)

The app crate's **test target does not compile under `--features omw_local`** — 157 pre-existing `no variant … for enum SettingsSection` errors from stripped cloud/HOA sections (BillingAndUsage/Account/WarpAgent/Knowledge/…), unrelated to this PR. So app-crate TDD can't run in the omw_local configuration; pure-logic tests live in `warpui_core` (pass), and `cargo check -p warp --lib` is green throughout. GUI behavior needs real-machine smoke.

## Open items for maintainers

1. `is_pane_in_foreground` is a window-level approximation; precise tab/pane-level foreground (design §2.8) needs the Workspace active-tab / `focused_pane_id` path confirmed. Only gates the default-off suppression option.
2. OSC-kind decision (single generic vs. sub-parameter kind) — this PR takes "single generic, classify in P2".
3. `event_sounds` per-event map (rich `NotificationSound` enum) was simplified to a single `notification_sound_name` for P1 to avoid `SettingsValue`-on-nested-enum risk; per-event sounds land with P2's kinds.
