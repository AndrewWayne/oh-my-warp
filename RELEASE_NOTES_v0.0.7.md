# omw-local-preview v0.0.7

**Validation release.** No code changes from v0.0.6. Cut to verify two things end-to-end:

1. The CI release workflow timeout bump (90 → 180 min, PR #61) actually fixes the cold-cache mac build that timed out on both v0.0.4 and v0.0.6.
2. The v0.0.6 autoupdate path: installed v0.0.6 clients should detect v0.0.7, download, SHA-256-verify, swap, and relaunch into v0.0.7 within ~10 minutes of this release going live.

If both work, v0.0.7 is the proof that omw_local autoupdate ships. v0.0.8+ can resume feature work.

## What this build is (unchanged from v0.0.6)

A Mac terminal application that boots the stripped upstream client with all cloud surfaces and AI panels removed at compile time, then audited against eight forbidden hostnames in the binary's `.rodata`:

- `app.warp.dev`
- `auth.warp.dev`
- `dataplane.warp.dev`
- `dragonfruit.warp.dev`
- `events.warpdotdev.com`
- `gql.warp.dev`
- `releases.warp.dev`
- `signup.warp.dev`

## Install (first time only)

Open the DMG, drag `omw-warp-oss.app` to `/Applications`, then strip the quarantine attribute (the binary is unsigned):

```sh
xattr -d com.apple.quarantine /Applications/omw-warp-oss.app
```

If macOS still blocks the launch with "Apple could not verify ..." (Gatekeeper has its own cached decision separate from the quarantine xattr), go to **System Settings → Privacy & Security → "Open Anyway"** once. Subsequent launches will work.

For users already on v0.0.6: nothing to do. The autoupdater will pick this release up automatically within ~10 minutes and prompt for restart.

## Architecture

- `aarch64-apple-darwin` only.
- Unsigned.
- App bundle ID: `omw.local.warpOss`. Logs: `~/Library/Logs/warp-oss.log`.
