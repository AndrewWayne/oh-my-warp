# omw-local-preview v0.0.6

Sixth preview of the audit-clean `omw_local` build. This release ships the first **working autoupdate path** for the preview track: installed v0.0.5 (and later) clients detect new preview releases from this GitHub repository, verify the DMG against a published SHA-256 sidecar, strip the macOS quarantine attribute, and restart into the new build.

This is **still a preview**, not a v0.3 ship — the binary rebrand to `omw` / `omw-server`, multi-session agents, and inline tool-call cards (per [TODO.md](./TODO.md) v0.3 / v1.0) remain pending.

## Highlights since v0.0.5

### Autoupdate end-to-end

A v0.0.5 install no longer needs you to manually re-download the DMG when a new preview ships:

- The poll loop hits `https://api.github.com/repos/AndrewWayne/oh-my-warp/releases/latest` every ~10 minutes, parses the tag (must match `omw-local-preview-v<x>.<y>.<z>`), and validates that both the arm64 darwin DMG and its `.sha256` sidecar are present as release assets.
- The downloader follows the `browser_download_url` GitHub returns (signed CDN JWT — constructing the URL doesn't work because the signature is short-lived).
- The downloaded DMG is hashed in 64 KB streamed chunks and compared against the sidecar; a mismatch aborts the apply flow with `log::error!` and no banner.
- After the rename swap, `xattr -dr com.apple.quarantine` runs against the staged bundle so Gatekeeper doesn't block first launch (the build is still unsigned).
- A new omw-specific workspace banner reads "A new version is available — Restart to update."

### Release pipeline changes

To make autoupdate work, this release also changes how preview releases get built and published:

- `scripts/build-mac-dmg.sh` exports `GIT_RELEASE_TAG=omw-local-preview-v${VERSION}` **before** the `cargo build` step so `option_env!("GIT_RELEASE_TAG")` captures it at Rust compile time. Without this, the binary embeds an empty version and the autoupdater early-exits on every poll.
- The same script emits a canonical `omw-warp-oss-v<version>-aarch64-apple-darwin.dmg.sha256` companion file via `shasum -a 256`.
- `.github/workflows/release.yml` uploads the new `.sha256` alongside the DMG, and **drops `--prerelease`** — GitHub's `/releases/latest` endpoint filters out prereleases, so the previous behavior was incompatible with the autoupdate fetch.

### Trust posture

The preview track is **unsigned**. The SHA-256 verification protects against a tampered binary on the download path (CDN compromise, MITM with a forged TLS cert), but it does not protect against a malicious release published by someone with write access to this GitHub repository — that is a v1.0 problem (cosign manifest + Apple notarize). If your threat model includes that, do not use the preview track.

## Known follow-ups (v0.0.7 backlog)

- Cap response body sizes for the GitHub API and `.sha256` fetches (defense-in-depth against OOM).
- Validate the `browser_download_url` scheme/host before download (defense-in-depth against a spoofed API response).
- Honor `X-RateLimit-Reset` / `Retry-After` so 403 / 429 from GitHub trigger backoff instead of polling at the same cadence.
- Add the fixture-based test suite blocked today by an upstream test-target compile defect.
- Add a flock around `apply_update` so two simultaneously running app instances can't race the bundle swap.
- Rename Linux / Windows updater paths from `"warp-oss"` to `"omw-warp-oss"` (currently inert because the preview track is darwin-only).

## What this build is (unchanged from v0.0.1)

A Mac terminal application that boots the stripped upstream client with all cloud surfaces and AI panels removed at compile time, then audited against eight forbidden hostnames in the binary's `.rodata`:

- `app.warp.dev`
- `auth.warp.dev`
- `dataplane.warp.dev`
- `dragonfruit.warp.dev`
- `events.warpdotdev.com`
- `gql.warp.dev`
- `releases.warp.dev`
- `signup.warp.dev`

## Install

Open the DMG, drag `omw-warp-oss.app` to `/Applications`, then strip the quarantine attribute (the binary is unsigned):

```sh
xattr -d com.apple.quarantine /Applications/omw-warp-oss.app
```

For the autoupdate path going forward: install this v0.0.6 manually once, then any subsequent preview tag pushed to this repository will be picked up automatically within ~10 minutes of release publication.

## Architecture

- `aarch64-apple-darwin` only. x86_64 / Intel and universal builds are deferred.
- Unsigned. `Codesign + notarize` is a v1.0 task.
- App bundle ID: `omw.local.warpOss`. App data dir: `~/Library/Application Support/omw.local.warpOss/`. Logs: `~/Library/Logs/warp-oss.log`.
