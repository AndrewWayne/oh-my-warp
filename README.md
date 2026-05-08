# oh-my-warp (omw)

> You want a fast GPU-rendered terminal tool.
>
> You want **WezTerm**. Open-source, configurable, scriptable. Ugly and tedious.
>
> You want **Warp**. Half-open-source, block-based, modern, beautiful, intelligent. They want to charge you for their best AI integration.
>
> We want to hybridize the two. We will have **warp-oss + Tailscale + pi-agent wrapper**. Configurable, all open-source, run by the community.

---

## What this is

`omw` is a local-first fork of the open-source Warp terminal with a thin wrapper of pi-mono. It replaces Warp's cloud half with components you control:

- **BYOK** — bring your own LLM keys (OpenAI, Anthropic, OpenAI-compatible, Ollama). No omw cloud. No Warp cloud.
- **BYORC** — bring your own remote controller. Pair sessions over your Tailscale tailnet, never the public internet.
- **Local agent** — orchestrates LLMs, shell, and file edits with explicit approvals.

`omw` is the product brand. `oh-my-warp` is the repo codename.

## Install

Download the latest `.dmg` from [Releases](https://github.com/AndrewWayne/oh-my-warp/releases), then:

```bash
# drag omw-warp-oss.app into /Applications, then:
xattr -d com.apple.quarantine /Applications/omw-warp-oss.app
open /Applications/omw-warp-oss.app
```

The `xattr` step is required because preview builds are unsigned.

## What works today

- **Audit-clean stripped client.** All Warp cloud / sign-in / Drive / hosted-agent surfaces removed at compile time.
- **BYORC over Tailscale.** Click the Phone button on any pane → the pair URL is auto-copied to your clipboard → open it on your phone (or paste into another laptop's browser) → attach to the live pane. Phone keystrokes echo on the laptop in real time.
- **Inline agent.** Type `# <prompt>` at the start of any pane to run your prompt through `omw-agent` against your configured provider. Shell commands and file edits prompt for approval before running.
- **Settings → Agent.** Configure providers, default model, and API keys (stored in the macOS Keychain) from inside the app.

## Limitations

- **macOS arm64 only, unsigned.** No Windows or Linux build yet. The `xattr` step above is the unsigned-binary workaround.
- **First-key-save on the bundled `.app`** may silently fail to write to the macOS Keychain on some machines (an ad-hoc-signed bundle ACL issue — Apply now surfaces this as an error rather than swallowing it). If it happens, save the key once from a terminal:
  ```bash
  security add-generic-password -s "omw/<provider-id>" -a "<provider-id>" -w "<your-key>" -A
  ```
  Real fix arrives with codesign + notarize.
- **One agent session per app process.** Multi-pane simultaneous agent sessions aren't supported yet.
- **Agent panel renders streaming text + Approve/Reject buttons only** — no per-call `args` / `result` cards yet.
- **Cost surface only in the CLI** (`omw costs`), not in the GUI.
- **Reverse-direction resize during an active phone session.** Resizing the laptop window while a phone is attached doesn't propagate the new size to the phone's xterm.
- **iOS Safari cold-path connect.** First handshake to a peer can stall 10–30s when the Tailscale path / iOS connection pool is cold; the client retries automatically.

## Build from source

```bash
bash scripts/build-mac-dmg.sh <version>
```

See [`specs/fork-strategy.md`](./specs/fork-strategy.md) for the upstream-sync workflow.

## Docs

- [PRD.md](./PRD.md) — product scope, principles, roadmap
- [CONTRIBUTING.md](./CONTRIBUTING.md) — how to contribute
- [`specs/`](./specs/) — protocol specs, test plan, fork strategy

## License

AGPL-3.0, inherited from upstream Warp. See [LICENSE](./LICENSE).
