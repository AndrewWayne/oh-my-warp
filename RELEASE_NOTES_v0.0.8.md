# omw-local-preview v0.0.8

**Bug-fix release.** Restores automatic update polling under `omw_local`, which v0.0.6 and v0.0.7 shipped wired but inert.

## What broke in v0.0.6 / v0.0.7

`AppExecutionMode::can_autoupdate()` required `ChannelState::official_cloud_services_enabled()` to be true. That flag is intentionally **false** under `omw_local` — it's what strips the cloud surfaces — so the gate at `AutoupdateState::register` (`app/src/autoupdate/mod.rs:135`) never started the poll loop. Confirmed live on a v0.0.6 install: 5 hours of uptime, the omw GitHub-releases code path linked in, and zero `omw autoupdate` log lines.

Menu-driven "Check for Updates" still worked on v0.0.6 / v0.0.7 because `RequestType::ManualCheck` bypasses the same gate, but the automatic 10-minute poll never fired.

## The fix (PR #63)

`vendor/warp-stripped/crates/warp_core/src/execution_mode.rs`:

```rust
pub fn can_autoupdate(&self) -> bool {
    self.is_app()
        && (cfg!(feature = "omw_local") || ChannelState::official_cloud_services_enabled())
}
```

Bypass scoped to `cfg!(feature = "omw_local")`. Non-omw builds are byte-identical.

## After installing v0.0.8

Within ~10 minutes of first launch, `~/Library/Logs/warp-oss.log` should contain a line like:

```
[INFO] omw autoupdate: fetching latest release from https://api.github.com/repos/AndrewWayne/oh-my-warp/releases/latest
```

That confirms the auto-poll is firing. If v0.0.8 is the latest release on GitHub, the same line is followed by a no-op result; the next release after v0.0.8 will then exercise the full fetch → SHA-256 verify → swap → relaunch path on the existing install.

## Install (first time only)

Open the DMG, drag `omw-warp-oss.app` to `/Applications`, then strip the quarantine attribute (the binary is unsigned):

```sh
xattr -d com.apple.quarantine /Applications/omw-warp-oss.app
```

If macOS still blocks with "Apple could not verify ...", go to **System Settings → Privacy & Security → "Open Anyway"** once.

For users already on v0.0.7 with auto-poll enabled — there aren't any, that's the whole point of this release. For users on v0.0.6 / v0.0.7: trigger "Check for Updates" from the menu once to pull v0.0.8; future autoupdates will work without intervention.

## Architecture

- `aarch64-apple-darwin` only.
- Unsigned.
- App bundle ID: `omw.local.warpOss`. Logs: `~/Library/Logs/warp-oss.log`.
