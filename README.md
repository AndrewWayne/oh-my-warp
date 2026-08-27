# oh-my-warp (omw)

> You want a fast GPU-rendered terminal tool.
>
> You want **WezTerm**. Open-source, configurable, scriptable. Ugly and tedious.
>
> You want **[warp](https://github.com/warpdotdev/warp)**. Half-open-source, block-based, modern, beautiful, intelligent. They want to charge you for their best AI integration.
>
> We want to hybridize the two. We will have **warp-oss + Tailscale + pi-agent wrapper**. Configurable, all open-source, run by the community.

---

## What this is

`omw` is a local-first fork of the open-source upstream terminal ([warpdotdev/warp](https://github.com/warpdotdev/warp)) with a thin wrapper of pi-mono. It replaces the upstream cloud half with components you control:

- **BYOK** — bring your own LLM keys (OpenAI, Anthropic, OpenAI-compatible, Ollama). No omw cloud. No upstream cloud.
- **BYORC** — bring your own remote controller. Pair sessions over your Tailscale tailnet, never the public internet.
- **Local agent** — orchestrates LLMs, shell, and file edits with explicit approvals.

`omw` is the product brand. `oh-my-warp` is the repo codename.

## Install

Download the latest release from [Releases](https://github.com/AndrewWayne/oh-my-warp/releases).

On Windows, run the `x86_64-pc-windows-msvc-setup.exe` artifact. It installs
for the current user, adds Start Menu and Apps & Features entries, and does not
require administrator access. Preview installers are unsigned, so Windows may
show a SmartScreen warning. The portable `.zip` remains available for users who
do not want an installed application.

On macOS, download the `.dmg`, then:

```bash
# drag omw-warp-oss.app into /Applications, then:
xattr -d com.apple.quarantine /Applications/omw-warp-oss.app
open /Applications/omw-warp-oss.app
```

The `xattr` step is required because preview builds are unsigned. See
[Windows installation](./docs/windows-installation.md) for installer,
uninstaller, upgrade, and silent-deployment details.

## What works today

- **Audit-clean stripped client.** All upstream cloud / sign-in / Drive / hosted-agent surfaces removed at compile time.
- **BYORC over Tailscale.** With Tailscale running on both devices, launch the host with `OMW_REMOTE_BIND=<host-tailnet-IPv4>:8787`, then click the Phone button on a shareable local pane → the pair URL is auto-copied to your clipboard → open it on your phone (or paste into another laptop's browser) → attach to the live pane. Viewing is read-only by default; remote typing also requires `OMW_REMOTE_ALLOW_DEFAULT_WRITE=1`.
- **Inline agent.** Type `# <prompt>` at the start of any pane to run your prompt through `omw-agent` against your configured provider. Shell commands and file edits prompt for approval before running.
- **Settings → Agent.** Configure providers, default model, and API keys from inside the app. Keys are stored in macOS Keychain or Windows Credential Manager.

## Limitations

- **Unsigned previews.** macOS is arm64-only; Windows is x86_64-only. Windows may show SmartScreen and macOS requires the `xattr` workaround above. Linux packaging is not available yet.
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

Windows release builds use the portable payload as the single source for both
artifacts:

```powershell
pwsh -File scripts/build-windows-zip.ps1 -Version <version>
pwsh -File scripts/build-windows-installer.ps1 -Version <version>
```

See [`specs/fork-strategy.md`](./specs/fork-strategy.md) for the upstream-sync workflow.

## Docs

- [PRD.md](./PRD.md) — product scope, principles, roadmap
- [CONTRIBUTING.md](./CONTRIBUTING.md) — how to contribute
- [Mobile Web Controller Phone QA](./docs/mobile-web-controller-phone-qa.md) — mobile Web Controller QA ladder
- [Mobile Remote-Control QA](./docs/mobile-remote-control-qa.md) — full Simulator and phone QA for real shell, Claude Code, and Codex CLI flows
- [Windows installation](./docs/windows-installation.md) — installer, upgrades, uninstall, and retained user data
- [`specs/`](./specs/) — protocol specs, test plan, fork strategy

## License

AGPL-3.0, inherited from upstream Warp. See [LICENSE](./LICENSE).
