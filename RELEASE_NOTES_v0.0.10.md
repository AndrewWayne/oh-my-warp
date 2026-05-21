# omw-local-preview v0.0.10

**End-to-end swap path validation.** No code changes from v0.0.9.

v0.0.9 fixed the autoupdate subsystem (poll loop init, action dispatch, Channel::Oss routing, bundle-relative `executable_path`, About-page UX) but we couldn't pre-tag verify the download → SHA-256 → atomic swap → relaunch path locally because v0.0.9 was the latest release (nothing newer to swap into). This release exists for that exercise: v0.0.9 installs auto-poll, find v0.0.10 as the latest non-prerelease, download the DMG, verify the SHA-256 sidecar, atomically swap `/Applications/omw-warp-oss.app`, and relaunch into v0.0.10.

If the swap reaches the UpdateReady stage, the tab-bar shows an "Update omw-warp-oss" button (the brand-rebranded `UPDATE_READY_TEXT` from v0.0.9) and the About panel's "Updates" section reflects the stage. Clicking the button (or the equivalent menu entry, or `workspace:apply_update` from Cmd+P) performs the swap + relaunch.

## What's in this release

Nothing new. Identical to v0.0.9 except for the version tag baked into the binary (`option_env!("GIT_RELEASE_TAG")`) and these release notes.

## Install (first time only)

Open the DMG, drag `omw-warp-oss.app` to `/Applications`, then strip the quarantine attribute (the binary is unsigned):

```sh
xattr -dr com.apple.quarantine /Applications/omw-warp-oss.app
```

If macOS still blocks with "Apple could not verify ...", go to **System Settings → Privacy & Security → "Open Anyway"** once.

## Upgrade from v0.0.9

If v0.0.9 is installed and running, the auto-poll (every 10 min) detects v0.0.10 automatically. You'll see a banner / button "Update omw-warp-oss" — click it to apply. Alternatively, **Settings → About → Check for updates** triggers it manually.

## Architecture

- `aarch64-apple-darwin` only.
- Unsigned.
- App bundle ID: `omw.local.warpOss`. Logs: `~/Library/Logs/warp-oss.log`.
