# omw-local-preview v0.0.3

Third preview of the audit-clean `omw_local` build, with the **inline-agent stack** wired through. The agent panel now talks to a real local omw-server running inside the app, the `# `-prefix sigil routes prompts to the agent without leaving the terminal, and bash tools the agent calls execute in your focused pane with output streamed back inline.

This is **still a preview**, not a v0.3 ship — the binary rebrand to `omw` / `omw-server` standalone, multi-session agents, and inline tool-call cards (per [TODO.md](./TODO.md) v0.3 / v1.0) remain pending.

## Highlights since v0.0.2

### Inline agent: type `# foo` in any pane

```text
# explain why the last test failed
```

A `# ` (hash + space) at column 0 of a single-line buffer is intercepted before the shell sees it. The line is sent to the local agent kernel; the streaming response appears **inline in the same pane** you typed in. No need to open the agent panel. The grammar:

- `# <prompt>` ⇒ inline-agent prompt.
- `## …` ⇒ literal shell comment (escape hatch).
- `#123 fix bug` ⇒ shell (no space after `#`).
- `echo foo # bar` ⇒ shell (mid-buffer `#`).
- Multi-line buffers ⇒ shell (heredocs, continuation prompts).

The `inline-agent-command-execution-report.md` report's §4 grammar is what landed; pinned by 7 unit tests in `omw_inline_prompt_test.rs`.

### One binary — no sidecars

`omw-warp-oss.app` now bundles the omw-agent kernel (Node script + dependencies) inside `Contents/Resources/` and lazy-spawns it from inside the Rust process on first agent use. The in-process omw-server binds `127.0.0.1:8788` for the GUI's WebSocket bridge. You launch the app, you get an agent. **Node is required** on the user's `$PATH` (every Mac with Homebrew has it; if not, `brew install node`).

### Bash broker: agent-driven commands run in the focused pane

When the agent calls `bash` as a tool, the command is injected into your **currently-focused pane's PTY** (the pane you were looking at when the agent emitted `bash/exec`). Output streams back to the agent transcript. If you shift focus mid-command, the original pane keeps streaming — focus changes only affect *future* tool calls. OSC 133 prompt-end markers terminate cleanly; a 30-second timeout falls back to a `snapshot:true` exit so the agent never wedges.

### Agent settings page

A new "Agent" tab in Settings exposes the `omw-config` schema:
- Provider list (OpenAI / Anthropic / OpenAI-compatible / Ollama).
- Per-provider model + base URL + API-key-via-keychain entry.
- Approval mode (`read_only` / `ask_before_write` / `trusted`).
- Default-provider selector.

Apply writes pending API keys to the macOS keychain first (`keychain:omw/<provider-id>`), then atomically serialises the typed config to `~/.config/omw/config.toml` via `toml_edit` (preserves your handwritten comments + key order). Discard reverts to the on-disk state.

### Approval card click handlers

Approval cards in the agent panel now have working Approve / Reject buttons. Clicks call into `OmwAgentState::send_approval_decision` directly; the kernel resolves the approval and the agent loop continues.

### Test infrastructure

| Suite | Tests |
|---|---|
| `omw-server` (broker, sessions, audit, registry, …) | 47 passing |
| `omw-agent` (vitest: serve, session, bash adapter, policy, keychain) | 79 passing / 3 skipped |
| Warp L3a integration (panel, command broker, approval, settings, page-logic, prompt parser) | 39 passing / 2 ignored |

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

All eight return zero hits in this build (`scripts/audit-no-cloud.sh`).

## Configuring the agent

1. Open `omw-warp-oss.app`.
2. Open Settings → Agent.
3. Add a provider, paste your API key, hit Apply.
4. Type `# hello` in any terminal pane.

The agent kernel reads `~/.config/omw/config.toml`. API keys go to the macOS keychain under `keychain:omw/<provider-id>`. The configured `[approval]` mode controls whether tool calls (e.g. bash, file edits) require a click-to-approve in the panel.

## Known issues

- **No inline tool-call cards.** The agent's text response streams inline in your pane, but tool-call cards (the agent saying "I'm going to run `ls`") render in the agent panel only — not inline in the terminal block. Open the panel to see the full transcript and approval cards.
- **One agent session per app process.** Re-opening the panel restarts the session. No transcript persistence across launches.
- **No multi-pane sessions.** Every terminal pane shares the singleton agent session; the focused pane is the bash-target.
- **Node-on-PATH requirement.** If `node` isn't on `$PATH` when the app launches, the agent panel reports `Failed: spawn omw-agent kernel: ...`. Install Node and relaunch. (Bundling a Node binary inside the `.app` is deferred to a future preview to keep `.dmg` size sane.)
- **iOS Safari over Tailscale cold-path connect** (carried over from v0.0.2 — pair-and-share-a-pane). Pre-warm + retry mitigates but doesn't eliminate.
- **Unsigned `.dmg`.** Same `xattr -d com.apple.quarantine` as v0.0.1 / v0.0.2.
- **macOS aarch64 only.** No Windows `.zip` for v0.0.3 — the Windows build script does not yet bundle the `omw-agent` kernel or `omw-keychain-helper.exe`, so the inline-agent feature wouldn't work there. Windows parity will land in a later preview.

## Install

```bash
hdiutil attach omw-warp-oss-v0.0.3-aarch64-apple-darwin.dmg
cp -R "/Volumes/omw-warp-oss v0.0.3/omw-warp-oss.app" /Applications/
hdiutil detach "/Volumes/omw-warp-oss v0.0.3/"
xattr -d com.apple.quarantine /Applications/omw-warp-oss.app
open /Applications/omw-warp-oss.app
```

The `xattr` line is required because the build is unsigned. Without it macOS shows "omw-warp-oss can't be opened because Apple cannot check it for malicious software." See [vendor/warp-stripped/OMW_LOCAL_BUILD.md](./vendor/warp-stripped/OMW_LOCAL_BUILD.md) for build prerequisites if you'd rather build from source.

## Bundle identity

| | |
|---|---|
| Bundle ID | `omw.local.warpOss` |
| App data dir | `~/Library/Application Support/omw.local.warpOss/` |
| Agent config | `~/.config/omw/config.toml` |
| Agent kernel | `omw-warp-oss.app/Contents/Resources/bin/omw-agent.mjs` |
| Logs | `~/Library/Logs/warp-oss.log` |
| Loopback agent server | `127.0.0.1:8788` (in-process; not exposed on the network) |

## License

AGPL-3.0. Corresponding source is the [oh-my-warp repo](https://github.com/AndrewWayne/oh-my-warp) at tag `omw-local-preview-v0.0.3`. The umbrella's `LICENSE` is included in the `.dmg`.

## Reporting issues

Open an issue on the [oh-my-warp repo](https://github.com/AndrewWayne/oh-my-warp/issues) with:
- macOS version (`sw_vers`)
- Last 200 lines of `~/Library/Logs/warp-oss.log` if the app fails to launch
- Output of `xattr /Applications/omw-warp-oss.app` if Gatekeeper blocked you
- Output of `node --version` if the agent panel reports a kernel-spawn failure
- `~/.config/omw/config.toml` (with API keys redacted) if the agent panel reports a config-related failure
