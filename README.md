# oh-my-warp (omw)

> You want a fast GPU-rendered terminal tool.
>
> You want **WezTerm**. Open-source, configurable, scriptable. Ugly and tedious.
>
> You want **Warp**. Half-open-source, block-based, modern, beautiful, intelligent. They want to charge you for their best AI integration.
>
> We want to hybridize the two. We will have **warp-oss + Tailscale + PI-agent wrapper**. Configurable, all open-source, run by the community.

---

## What this is

`omw` is a local-first fork of the open-source Warp terminal with a thin wrapper of Pi-mono. It replaces Warp's cloud half with components you control:

- **BYOK** — bring your own LLM keys (OpenAI, Anthropic, OpenAI-compatible, Ollama). No omw cloud. No Warp cloud.
- **BYORC** — bring your own remote controller. Pair sessions over your Tailscale tailnet, never the public internet.
- **Local agent** — `omw-agent` orchestrates LLMs, MCP tools, shell, files, and approvals with full audit and cost telemetry.

`omw` is the product brand. `oh-my-warp` is the repo codename.

## Status

Pre-v1.0. The project is in active development — see [PRD.md](./PRD.md) for committed scope and [TODO.md](./TODO.md) for phase progress.

Current preview track: **`omw-local-preview-v0.0.1`** — an audit-clean local build of the Warp client with cloud calls stripped. macOS arm64 only, unsigned.

| Phase | Deliverable | Status |
|---|---|---|
| v0.1 | `omw-agent` CLI (BYOK + tools + audit) | planned |
| v0.2 | OpenAI-compatible + Ollama providers | planned |
| v0.3 | Forked GUI in local mode (`omw-warp-oss`) | preview shipping |
| v0.4 | `omw-remote` daemon + Web Controller over Tailscale | planned |
| v1.0 | Polish, sign, notarize, Homebrew | planned |

## Install (preview)

Download the latest `.dmg` from [Releases](https://github.com/AndrewWayne/oh-my-warp/releases), then:

```bash
# drag omw-warp-oss.app into /Applications, then:
xattr -d com.apple.quarantine /Applications/omw-warp-oss.app
open /Applications/omw-warp-oss.app
```

The `xattr` step is required because preview builds are unsigned. Codesign + notarize is a v1.0 task.

## Build from source

```bash
bash scripts/build-mac-dmg.sh <version>
```

The script does not modify `vendor/warp-stripped/` — packaging-time renames only. See [`specs/fork-strategy.md`](./specs/fork-strategy.md) for the upstream-sync workflow.

## Docs

- [PRD.md](./PRD.md) — product scope, principles, phased roadmap
- [TODO.md](./TODO.md) — phase-by-phase progress
- [CONTRIBUTING.md](./CONTRIBUTING.md) — how to contribute
- [CLAUDE.md](./CLAUDE.md) — engineering guardrails
- [`specs/`](./specs/) — protocol specs, test plan, fork strategy

## License

AGPL-3.0, inherited from upstream Warp. See [LICENSE](./LICENSE).
