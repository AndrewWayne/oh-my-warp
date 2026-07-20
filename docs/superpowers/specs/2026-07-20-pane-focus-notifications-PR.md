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

## Design decisions (see design doc for rationale)

- **Default-on** without touching the legacy `mode` (default `Unset`) or the unconfigured-banner path: a dedicated `is_escape_sequence_enabled` (default true) + the flag ungate.
- **P1 = single "generic" notification.** OSC 9 carries no kind, so "completed vs needs-approval" classification is deferred to P2's `CLIAgentEventType` rather than extending `Handler::pluggable_notification` (which would touch 4 impls).
- **Custom sound via `~/Library/Sounds`** (name reference), verified empirically to be resolvable by name; no packaging change.
- **Sound plumbing is additive** (`Option` fields + builders) rather than a breaking `play_sound: bool → SoundSpec` refactor, to avoid churning all 6 platform senders.

## Verification

- `cargo check -p warp --lib` — green after every milestone (includes cc-compiling the objc).
- `cargo test -p warpui_core pane_focus_notification` — 4/4 (UserNotification builders + template rendering).
- `cargo test -p warp_features omw_local_flags` — green (flag contracts).
- ⚠️ **App-crate test suite not executed**: `cargo test -p warp` needs to download cross-platform test-only deps (windows/wgpu) and crates.io is unreliable in this environment. The MS1 settings round-trip tests compile-clean (serde_json is a direct dep) but were not run here.
- ⚠️ **No GUI smoke yet.** Needs a built omw_local app: `printf '\033]9;hello\007'` → banner; switch window/tab → click → focus origin pane; two panes → each click returns to its own pane; set `[notifications] notification_sound_name`/`title_template`/`throttle_window_secs` and observe.

## Remaining (not in this PR)

- **MS5 settings UI**: the settings all work via `settings.toml [notifications]` today; a dedicated Notifications settings page (mirroring `omw_agent_page.rs`, `#[cfg(feature="omw_local")]`) is still to do for discoverability.
- **MS6 P2 native agent-turn detection**: raise "completed / needs-approval" notifications from `cli_agent_sessions` (`agent_management_model.rs`) reusing this P1 delivery/focus/config base, with the `CLIAgentEventType` classification. Cross-check the `terminal_view_id → PaneViewLocator` mapping vs. `FocusTerminalViewInWorkspace` (design §3).
- **MS7 NotificationChannel trait** + Linux abstraction hook (macOS is full-featured; Linux delivers but click routing is a follow-up).

## Open items for maintainers

1. `is_pane_in_foreground` is a window-level approximation; precise tab/pane-level foreground (design §2.8) needs the Workspace active-tab / `focused_pane_id` path confirmed. Only gates the default-off suppression option.
2. OSC-kind decision (single generic vs. sub-parameter kind) — this PR takes "single generic, classify in P2".
3. `event_sounds` per-event map (rich `NotificationSound` enum) was simplified to a single `notification_sound_name` for P1 to avoid `SettingsValue`-on-nested-enum risk; per-event sounds land with P2's kinds.
