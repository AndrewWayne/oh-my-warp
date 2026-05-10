# omw-local-preview v0.0.4

Fourth preview of the audit-clean `omw_local` build. Adds a **global `AGENTS.md` system prompt** to the inline agent — point the agent at your own writing-style preferences, project conventions, or shell-skill reminders, and they apply to every `# <prompt>` you type.

This is **still a preview**, not a v0.3 ship — the binary rebrand to `omw` / `omw-server` standalone, multi-session agents, and inline tool-call cards (per [TODO.md](./TODO.md) v0.3 / v1.0) remain pending.

## Highlights since v0.0.3

### Global AGENTS.md, loaded on every session

Every inline-agent session now starts with a system prompt sourced from a single canonical file:

```
~/Library/Application Support/omw.local.warpOss/AGENTS.md
```

On first launch the app materializes a baseline AGENTS.md at that path. The baseline tells the model it's sharing your shell, asks it to plan-and-verify before mutating state, and reminds it to ask when ambiguous. You can edit the file directly and the next `# <prompt>` picks up your changes.

### Settings → Agent → "AGENTS.md source path"

If you'd rather keep your own AGENTS.md elsewhere (dotfiles repo, iCloud, wherever), paste the path into the new field in Settings → Agent and click Apply. The contents are copied to the canonical path immediately on Apply, and re-synced automatically on every subsequent agent session — so external edits to your source flow through without a re-import.

If the source path is ever missing or unreadable, the agent silently falls back to whatever's at the canonical path; a broken AGENTS.md never blocks session creation. A 64 KB cap rejects oversized files (treats them as absent) so a misconfiguration can't blow up the prompt budget.

### Test infrastructure

| Suite | Tests |
|---|---|
| `omw-config` (schema, writer, watcher, AGENTS.md helpers) | 62 passing |
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
4. (Optional) Paste a path to your own AGENTS.md and Apply again.
5. Type `# hello` in any terminal pane.

The agent kernel reads `~/.config/omw/config.toml` and `~/Library/Application Support/omw.local.warpOss/AGENTS.md`. API keys go to the macOS keychain under `keychain:omw/<provider-id>`. The configured `[approval]` mode controls whether tool calls (e.g. bash, file edits) require a click-to-approve in the panel.

## Known issues (carried over from v0.0.3)

- **No inline tool-call cards.** The agent's text response streams inline in your pane, but tool-call cards render in the agent panel only — not inline in the terminal block.
- **One agent session per app process.** Re-opening the panel restarts the session. No transcript persistence across launches.
- **No multi-pane sessions.** Every terminal pane shares the singleton agent session; the focused pane is the bash-target.
- **iOS Safari over Tailscale cold-path connect** (carried from v0.0.2). Pre-warm + retry mitigates but doesn't eliminate.
- **Unsigned `.dmg`.** Same `xattr -d com.apple.quarantine` as prior previews.
- **macOS aarch64 only.** No Windows `.zip` for v0.0.4.

## Install

```bash
hdiutil attach omw-warp-oss-v0.0.4-aarch64-apple-darwin.dmg
cp -R "/Volumes/omw-warp-oss v0.0.4/omw-warp-oss.app" /Applications/
hdiutil detach "/Volumes/omw-warp-oss v0.0.4/"
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
| **Agent system prompt** | `~/Library/Application Support/omw.local.warpOss/AGENTS.md` |
| Agent kernel | `omw-warp-oss.app/Contents/Resources/bin/omw-agent.mjs` |
| Logs | `~/Library/Logs/warp-oss.log` |
| Loopback agent server | `127.0.0.1:8788` (in-process; not exposed on the network) |

## License

AGPL-3.0. Corresponding source is the [oh-my-warp repo](https://github.com/AndrewWayne/oh-my-warp) at tag `omw-local-preview-v0.0.4`. The umbrella's `LICENSE` is included in the `.dmg`.

## Reporting issues

Open an issue on the [oh-my-warp repo](https://github.com/AndrewWayne/oh-my-warp/issues) with:
- macOS version (`sw_vers`)
- Last 200 lines of `~/Library/Logs/warp-oss.log` if the app fails to launch
- Output of `xattr /Applications/omw-warp-oss.app` if Gatekeeper blocked you
- Output of `node --version` if the agent panel reports a kernel-spawn failure
- `~/.config/omw/config.toml` (with API keys redacted) if the agent panel reports a config-related failure
- The first ~40 lines of `~/Library/Application Support/omw.local.warpOss/AGENTS.md` if you suspect the system prompt is misbehaving
