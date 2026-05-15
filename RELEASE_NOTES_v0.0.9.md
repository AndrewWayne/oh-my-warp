# omw-local-preview v0.0.9

**Real autoupdate fix.** v0.0.8 claimed to restore auto-poll under `omw_local` but only fixed an inner gate — the outer `FeatureFlag::Autoupdate` was still off, and even with both gates open the actual fetch, dispatch, and swap paths had additional blockers. This release closes the full chain.

Two independent `codex exec` reviews found three extra blockers across two passes; all addressed below. One more (the action-dispatch gate) was caught only by runtime click testing.

## What was broken in v0.0.6 / v0.0.7 / v0.0.8

Five independent gates kept the autoupdate subsystem dead on omw builds:

1. **`FeatureFlag::Autoupdate` was never enabled.** `app/src/lib.rs` only enables it when `cfg!(feature = "autoupdate")`, `is_release_bundle()`, or `additional_features()` contain it. The omw_local cargo feature didn't include `autoupdate`, the build script doesn't pass `release_bundle`, and `bin/oss.rs` didn't wire it into `additional_features`. Result: the poll loop never initialized and `workspace:check_for_updates` never registered in the command palette.

2. **`fetch_version` errored out before reaching the `Channel::Oss` branch.** `fetch_channel_versions` (which hits the upstream cloud server, then falls back to `${releases_base_url}/channel_versions.json`) ran unconditionally at the top of `fetch_version`. Under omw the cloud server is stripped and the GCP fallback URL resolves against the GitHub API base (404). The function aborted; the omw GitHub-Releases path was never executed.

3. **`executable_path` returned the wrong layout for the swap.** macOS-side `apply_update` joins `staged_bundle.path` with `executable_path(channel)` to verify the new binary exists before the atomic rename. `executable_path` only returns `Contents/MacOS/<name>` when `is_release_bundle()` is true; omw doesn't pass `release_bundle`, so the path resolved to a flat name. The "new executable does not exist" check would have failed every swap once the upstream gates were opened.

4. **`is_official_cloud_workspace_action` swallowed `CheckForUpdate`.** When `official_cloud_services_enabled()` is false (always true under omw), `Workspace::handle_action` early-returned for actions in a "cloud-only" matches! list. That list included `CheckForUpdate`, `ApplyUpdate`, `DownloadNewVersion`, and `AutoupdateFailureLink` — but those are autoupdate actions, not cloud actions. The manual-trigger button (and tab-bar overflow menu) would dispatch but get silently dropped before reaching `manual_check_for_update`. Caught only by runtime click testing — the static review missed it.

5. **Stripped About-page autoupdate UI.** The omw build replaced upstream's About panel with a re-themed version that dropped the autoupdate CTA entirely. No status text, no "Check for updates" button. The action handler at `WorkspaceAction::CheckForUpdate` existed but had no UI hook.

v0.0.8's PR #63 fix to `can_autoupdate()` was necessary but cleared only the inner gate at `autoupdate/mod.rs::register`. Standalone, it changed nothing observable.

## The fix

### Feature flag wiring
- New `OMW_LOCAL_FLAGS = &[FeatureFlag::Autoupdate]` in `crates/warp_features/src/lib.rs`.
- `bin/oss.rs` calls `state.with_additional_features(OMW_LOCAL_FLAGS)` under `#[cfg(feature = "omw_local")]`. Scope is the `warp-oss` binary only; non-omw builds are byte-identical.

### Channel::Oss routing
- `fetch_version` now short-circuits for `Channel::Oss` under omw_local before invoking the upstream channel-versions endpoint. The omw GitHub-Releases path (`autoupdate::oss::omw_fetch_latest_release`) runs directly.

### Bundle path resolution
- `executable_path` treats `cfg!(feature = "omw_local") && Channel::Oss` as a bundle in addition to `is_release_bundle()`. The omw `.app` layout matches what `apply_update` expects.

### Action dispatch
- `is_official_cloud_workspace_action` no longer matches autoupdate-related actions. `CheckForUpdate`, `ApplyUpdate`, `DownloadNewVersion`, and `AutoupdateFailureLink` propagate through `Workspace::handle_action` to their actual handlers under omw.

### UI surface
- Restored a "Check for updates" button to the omw About page under a new **Updates** section. Dispatches `WorkspaceAction::CheckForUpdate`.
- Added status text below the button reflecting `autoupdate::get_update_state(app)`: "Up to date", "Checking for update…", "Downloading update…", "Update ready: vX.Y.Z (relaunch to apply)", "Applying update vX.Y.Z…", or unable-to-update / unable-to-launch states.

### Brand
- `UPDATE_READY_TEXT`, the tab-bar "Update" menu items, and the manual-update menu items in `workspace/view.rs` are now cfg-gated to use `omw-warp-oss` instead of the upstream `Warp` wordmark when built with `omw_local`. Per CLAUDE.md §5, the literal `Warp` wordmark is forbidden in product-surface code for omw builds.

## Regression tests

Added three:
- `crates/warp_features/src/features_test.rs::omw_local_flags_enables_autoupdate` — guards `OMW_LOCAL_FLAGS` constant.
- `crates/warp_core/src/channel/state_tests.rs::omw_local_channel_state_enables_autoupdate` — exercises the actual `with_additional_features(OMW_LOCAL_FLAGS)` wiring.
- `crates/warp_core/src/execution_mode.rs::tests::omw_local_app_can_autoupdate` + `sdk_mode_cannot_autoupdate` — guards the v0.0.8 inner-gate fix from regression.

## After installing v0.0.9

Within ~10 minutes of first launch, `~/Library/Logs/warp-oss.log` should contain:

```
[INFO] omw autoupdate: fetching latest release from https://api.github.com/repos/AndrewWayne/oh-my-warp/releases/latest
```

The Settings → About panel now shows a **Check for updates** button under the **Updates** section. The command palette (Cmd+P → "Check for updates") also lists the action.

Upgrade path from v0.0.6 / v0.0.7 / v0.0.8: these older builds can't self-update (the outer gate kept the manual-check path's UI surface from registering, even though the action handler was wired). Install v0.0.9 manually one last time — from there future releases auto-poll and self-update.

## Install (first time only)

Open the DMG, drag `omw-warp-oss.app` to `/Applications`, then strip the quarantine attribute (the binary is unsigned):

```sh
xattr -d com.apple.quarantine /Applications/omw-warp-oss.app
```

If macOS still blocks with "Apple could not verify ...", go to **System Settings → Privacy & Security → "Open Anyway"** once.

## Architecture

- `aarch64-apple-darwin` only.
- Unsigned.
- App bundle ID: `omw.local.warpOss`. Logs: `~/Library/Logs/warp-oss.log`.
