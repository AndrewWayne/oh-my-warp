# omw-local-preview v0.0.2

Second preview of the audit-clean `omw_local` build, with the v0.4-thin-byorc work folded in: pair-and-share-a-pane via the embedded `omw-remote` daemon, plus WebSocket attach to a real laptop pane from a phone or browser via Tailscale.

This is **still a preview**, not a v0.3 ship — the binary rename to `omw`, `omw-server` standalone, and the v0.3 GUI agent panel (per [TODO.md](./TODO.md)) remain pending.

## Highlights since v0.0.1

### Pair → share → attach: a real, working flow

- The Phone button on a pane now bridges that pane through `omw-remote`, exposing it over a signed WebSocket. The phone (or a browser on the same tailnet) attaches and sees the laptop's actual TUI — Claude Code, vim, htop, anything ratatui-based — typed input flows back to the laptop pane.
- Per-pane share dispatch (multiple panes can be shared independently); reactive Phone-button state; an "agent footer" that stays visible while a pane is shared.
- A `pair-redeem` HTTP endpoint and pairing UI on the web controller, gated behind a one-shot pairing token.

### Tmux-style attach (the hard part)

- Server-side `vt100::Parser` per session: the daemon maintains a virtual terminal grid that mirrors the laptop pane's rendered state. On attach, the phone receives a serialized snapshot of that grid as the first WebSocket frame, so it sees the current TUI directly instead of a blank canvas waiting for the next redraw.
- SIGWINCH-on-share trick (`Message::Resize` jitter) forces the laptop's child process to emit a complete repaint, so the parser captures a coherent baseline before the phone attaches.
- Resize-aware phone attach: laptop sends its actual pane size on attach; phone xterm.js matches it. For viewport-narrow clients (iPhone Safari at &lt;80 cols) the phone instead asks the laptop pane to shrink to its size, and Claude Code re-flows for the narrower viewport.

### Network reliability

- WS connect retry-with-timeout (3 attempts × 6s) plus an HTTP pre-warm fetch to `/api/v1/host-info` to wake up cold Tailscale WireGuard paths before the WS upgrade. Helps a known iOS-Safari-over-Tailscale failure mode where the very first packet to a peer can stall for tens of seconds.
- Inactivity-timeout skew on the WS handshake bumped 30s → 300s for mobile clients with drifting clocks.
- On-device debug log overlay in the Terminal page, primarily for iOS where DevTools isn't accessible — surfaces WS lifecycle, signature checks, frame counts, and connect-attempt timing.

### Test infrastructure

- `crates/omw-pty/tests/capture_claude_exit_hint.rs` spawns a real `claude.cmd` via `omw-pty`, drives `/exit` keystroke-by-keystroke, captures every PTY byte to a fixture file (gated on `OMW_CAPTURE_CLAUDE=1`).
- `apps/web-controller/tests/xterm-mid-stream-attach.test.ts` replays that fixture into real `@xterm/xterm` (jsdom) at the captured pane's 149×39 size and at the phone's default 80×24 — the failing-at-80×24 test was the smoking gun that proved the original duplicate-render bug was a phone-vs-laptop size mismatch.
- `crates/omw-server/tests/parser_attach.rs`, `crates/omw-remote/tests/ws_tui_no_duplicate.rs`, `crates/omw-server/tests/vt100_mode_2026.rs` round out the byte-flow and parser-correctness coverage.

## Known issues

- iOS Safari over Tailscale can still take 10-30s to establish the very first WS for a fresh tab — pre-warm + retry mitigates but doesn't eliminate this. Tracked for a Tailscale-side investigation.
- The agent panel is still gated to a "no agent yet" placeholder; v0.3 work.
- This `.dmg` is unsigned; Gatekeeper requires the same `xattr -d com.apple.quarantine` as v0.0.1 (see Install below).

## What this build is (unchanged from v0.0.1)

A Mac terminal application that boots upstream Warp with all cloud surfaces and AI panels removed at compile time, then audited against eight forbidden hostnames in the binary's `.rodata`:

- `app.warp.dev`
- `api.warp.dev`
- `cloud.warp.dev`
- `oz.warp.dev`
- `firebase.googleapis.com`
- `firebaseio.com`
- `identitytoolkit.googleapis.com`
- `securetoken.googleapis.com`

All eight return zero hits in this build (`scripts/audit-no-cloud.sh`). The binary still runs as a working terminal: local PTY, command palette, settings (the local-only tabs), code editor, file tree, completer.

## Install

```bash
hdiutil attach omw-warp-oss-v0.0.2-aarch64-apple-darwin.dmg
cp -R "/Volumes/omw-warp-oss v0.0.2/omw-warp-oss.app" /Applications/
hdiutil detach "/Volumes/omw-warp-oss v0.0.2/"
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

AGPL-3.0. Corresponding source is the [oh-my-warp repo](https://github.com/AndrewWayne/oh-my-warp) at tag `omw-local-preview-v0.0.2`. The umbrella's `LICENSE` is included in the `.dmg`.

## Reporting issues

Open an issue on the [oh-my-warp repo](https://github.com/AndrewWayne/oh-my-warp/issues) with:
- macOS version (`sw_vers`)
- Last 200 lines of `~/Library/Logs/warp-oss.log` if the app fails to launch
- Output of `xattr /Applications/omw-warp-oss.app` if Gatekeeper blocked you
- For phone-attach / share issues: contents of the on-device debug overlay on the Terminal page
