# omw-local-preview v0.0.1

First publicly distributed `.dmg` of the audit-clean `omw_local` build of upstream `warpdotdev/warp`. **Preview track only.** This is not a v0.3 ship: the `omw` rebrand of the binary, `omw-server`, and the GUI agent panel (all listed in [TODO.md](./TODO.md) v0.3) are still pending.

## What this is

A Mac terminal application that boots the upstream Warp client with all cloud surfaces and AI panels removed at compile time, then audited against eight forbidden hostnames in the binary's `.rodata`:

- `app.warp.dev`
- `api.warp.dev`
- `cloud.warp.dev`
- `oz.warp.dev`
- `firebase.googleapis.com`
- `firebaseio.com`
- `identitytoolkit.googleapis.com`
- `securetoken.googleapis.com`

All eight return zero hits in this build (`scripts/audit-no-cloud.sh`). The binary still runs as a working terminal: local PTY, command palette, settings (the local-only tabs), code editor, file tree, completer.

## What's missing

- **Agent panel** — gated to "no agent yet" placeholder; full re-wire to `omw-server` → `omw-agent` is v0.3 work.
- **`omw` rename of the Cargo bin target** — the bundle ships as `omw-warp-oss`, but the underlying binary inside `Contents/MacOS/` is still produced from the upstream `[[bin]] warp-oss` target. Logs land in `~/Library/Logs/warp-oss.log`.
- **Code signing / notarization** — this build is unsigned. Gatekeeper will block it on first launch unless you run the quarantine command below.
- **x86_64** — `aarch64-apple-darwin` only. Intel Macs not supported in this preview.
- **Auto-update** — none. Each preview is a fresh download.

## Install

```bash
hdiutil attach omw-warp-oss-v0.0.1-aarch64-apple-darwin.dmg
cp -R "/Volumes/omw-warp-oss v0.0.1/omw-warp-oss.app" /Applications/
hdiutil detach "/Volumes/omw-warp-oss v0.0.1/"
xattr -d com.apple.quarantine /Applications/omw-warp-oss.app
open /Applications/omw-warp-oss.app
```

The `xattr` line is required because the build is unsigned. Without it macOS shows "omw-warp-oss can't be opened because Apple cannot check it for malicious software." See [vendor/warp-stripped/OMW_LOCAL_BUILD.md](./vendor/warp-stripped/OMW_LOCAL_BUILD.md) for build prerequisites if you'd rather build from source.

## Bundle identity

| | |
|---|---|
| Bundle ID | `omw.local.warpOss` |
| App data dir | `~/Library/Application Support/omw.local.warpOss/` |
| Logs | `~/Library/Logs/warp-oss.log` (path inherited from the embedded plist; not currently overridden) |

The bundle ID intentionally differs from upstream Warp's `dev.warp.WarpOss` so the two can coexist if you also have an upstream OSS build installed.

## License

AGPL-3.0. Corresponding source is the [oh-my-warp repo](https://github.com/AndrewWayne/oh-my-warp) at tag `omw-local-preview-v0.0.1`. The umbrella's `LICENSE` is included in the `.dmg`.

## Reporting issues

Open an issue on the [oh-my-warp repo](https://github.com/AndrewWayne/oh-my-warp/issues) with:
- macOS version (`sw_vers`)
- Last 200 lines of `~/Library/Logs/warp-oss.log` if the app fails to launch
- Output of `xattr /Applications/omw-warp-oss.app` if Gatekeeper blocked you
