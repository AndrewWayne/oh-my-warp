# omw-local-preview v0.0.5

Fifth preview of the audit-clean `omw_local` build. This release focuses on a **better remote-control UI/UX** for phones and browsers: the Web Controller terminal now behaves more like a usable pocket terminal instead of a raw xterm viewport.

This is **still a preview**, not a v0.3 ship -- the binary rebrand to `omw` / `omw-server` standalone, multi-session agents, and inline tool-call cards (per [TODO.md](./TODO.md) v0.3 / v1.0) remain pending.

## Highlights since v0.0.4

### Phone-first terminal controls

The remote terminal now ships a mobile shortcut strip with the keys you need most when controlling a host pane from a phone:

- Shift-Tab, Esc, Tab, Ctrl-C, and arrow keys are one tap away.
- The More drawer adds Ctrl-D, Ctrl-L, `/`, `|`, and `?`.
- A hide-keyboard button lets you get back to full terminal visibility without leaving the page.
- Touch handling preserves xterm focus, so shortcut taps send bytes instead of stealing the hidden terminal input.

### Keyboard-aware remote terminal layout

The Terminal page now listens to `visualViewport` resize/scroll events, docks the shortcut strip above the mobile keyboard, and refits xterm after keyboard and drawer transitions settle. On narrow phone screens, the client asks the host pane to resize to a stable phone-sized grid; on wider browser clients, it keeps the host pane's size to avoid disturbing the laptop session.

### Better pair-to-terminal flow

When pairing lands on a host with exactly one active shared pane, the Web Controller auto-opens that terminal once per mount. The Sessions page still remains reachable from the terminal toolbar, and it now makes the "Start a new shell" / "Stop sharing" behavior clearer.

### Native mobile QA ladder

This preview adds automated and manual QA coverage for the remote-control path:

- Mobile web automation for pairing, sessions, terminal input, shortcut bytes, viewport shrink, and TUI scroll behavior.
- Native iOS Safari automation through Appium/XCUITest for keyboard-visible and shortcut-drawer scenarios.
- A real-Claude QA harness that starts a real `omw-remote` session and records on-device evidence for terminal ergonomics.

See [docs/mobile-web-controller-phone-qa.md](./docs/mobile-web-controller-phone-qa.md) for the current ladder.

## What this build is (unchanged from v0.0.1)

A Mac terminal application that boots the stripped upstream client with all cloud surfaces and AI panels removed at compile time, then audited against eight forbidden hostnames in the binary's `.rodata`:

- `app.warp.dev`
- `api.warp.dev`
- `cloud.warp.dev`
- `oz.warp.dev`
- `firebase.googleapis.com`
- `firebaseio.com`
- `identitytoolkit.googleapis.com`
- `securetoken.googleapis.com`

All eight return zero hits in this build (`scripts/audit-no-cloud.sh`).

## Remote control quick start

1. Open `omw-warp-oss.app`.
2. Click the Phone button on a pane.
3. Open the copied pair URL on your phone or another browser.
4. Pair the host; if there is one active shared pane, the terminal opens automatically.
5. Tap the terminal to show the keyboard, then use the shortcut strip for Esc, arrows, Ctrl-C, Tab, and More.

## Configuring the agent

1. Open `omw-warp-oss.app`.
2. Open Settings -> Agent.
3. Add a provider, paste your API key, hit Apply.
4. (Optional) Paste a path to your own AGENTS.md and Apply again.
5. Type `# hello` in any terminal pane.

The agent kernel reads `~/.config/omw/config.toml` and `~/Library/Application Support/omw.local.warpOss/AGENTS.md`. API keys go to the macOS keychain under `keychain:omw/<provider-id>`. The configured `[approval]` mode controls whether tool calls (e.g. bash, file edits) require a click-to-approve in the panel.

## Known issues

- **Reverse-direction resize during an active phone session.** Initial phone attach can shrink the host pane for readability, but later laptop window resizes still do not propagate back to the phone.
- **iOS Safari over Tailscale cold-path connect.** Pre-warm + retry mitigates first-handshake stalls, but a cold peer path can still take noticeably longer than a warm reconnect.
- **No inline tool-call cards.** The agent's text response streams inline in your pane, but tool-call cards render in the agent panel only.
- **One agent session per app process.** Re-opening the panel restarts the session. No transcript persistence across launches.
- **No multi-pane agent sessions.** Every terminal pane shares the singleton agent session; the focused pane is the bash target.
- **Unsigned `.dmg`.** Same `xattr -d com.apple.quarantine` workaround as prior previews.
- **macOS aarch64 only.** No Windows `.zip` for v0.0.5.

## Install

```bash
hdiutil attach omw-warp-oss-v0.0.5-aarch64-apple-darwin.dmg
cp -R "/Volumes/omw-warp-oss v0.0.5/omw-warp-oss.app" /Applications/
hdiutil detach "/Volumes/omw-warp-oss v0.0.5/"
xattr -d com.apple.quarantine /Applications/omw-warp-oss.app
open /Applications/omw-warp-oss.app
```

The `xattr` line is required because the build is unsigned. Without it macOS shows an unsigned-app warning. See [vendor/warp-stripped/OMW_LOCAL_BUILD.md](./vendor/warp-stripped/OMW_LOCAL_BUILD.md) for build prerequisites if you'd rather build from source.

## Bundle identity

| | |
|---|---|
| Bundle ID | `omw.local.warpOss` |
| App data dir | `~/Library/Application Support/omw.local.warpOss/` |
| Agent config | `~/.config/omw/config.toml` |
| Agent system prompt | `~/Library/Application Support/omw.local.warpOss/AGENTS.md` |
| Agent kernel | `omw-warp-oss.app/Contents/Resources/bin/omw-agent.mjs` |
| Logs | `~/Library/Logs/warp-oss.log` |
| Loopback agent server | `127.0.0.1:8788` (in-process; not exposed on the network) |
| Remote-control server | `127.0.0.1:8787` plus tailnet access when enabled |

## License

AGPL-3.0. Corresponding source is the [oh-my-warp repo](https://github.com/AndrewWayne/oh-my-warp) at tag `omw-local-preview-v0.0.5`. The umbrella's `LICENSE` is included in the `.dmg`.

## Reporting issues

Open an issue on the [oh-my-warp repo](https://github.com/AndrewWayne/oh-my-warp/issues) with:

- macOS version (`sw_vers`)
- iOS / browser version for remote-control issues
- Last 200 lines of `~/Library/Logs/warp-oss.log` if the app fails to launch
- Output of `xattr /Applications/omw-warp-oss.app` if Gatekeeper blocked you
- Output of `node --version` if the agent panel reports a kernel-spawn failure
- `~/.config/omw/config.toml` (with API keys redacted) if the agent panel reports a config-related failure
