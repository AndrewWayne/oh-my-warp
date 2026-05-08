# Contributing to omw

Welcome. Before writing code, please:

1. Read [PRD.md](./PRD.md) — especially §3.1 (v1.0 Committed Scope) and §3.2 (Non-Goals).
2. Read [CLAUDE.md](./CLAUDE.md) — the four general principles plus §5 project-specific rules.
3. Skim the relevant spec under `specs/` for the area you're touching.

## Brand vs codename

- **omw** is the product brand — used on the binary, GUI wordmark, website, and packaging.
- **oh-my-warp** is the GitHub repo / community codename — used in repo paths and source attribution only.
- Never write `Warp` (capitalized) in product-surface code. The pre-commit hook will warn you.

## Claude Code hooks (project-local)

This repo ships with project-local Claude Code hooks under [`.claude/hooks/`](./.claude/hooks/). They run automatically when you use Claude Code in this project. They are advisory or guard-rail; none send telemetry. The hooks are:

| Event | Script | What it does |
|---|---|---|
| `PreToolUse` (Write/Edit) | `guard-vendor-and-brand.sh` | Warns on `Warp` (capitalized) in product-surface source. |
| `PreToolUse` (Bash) | `guard-bash.sh` | Blocks `rm -rf vendor/` and `rm -rf .git`. |
| `PostToolUse` (Edit/Write) | `spec-touch-reminder.sh` | Reminds you to run `/spec-consistency` after editing planning docs. |
| `Stop` | `todo-reminder.sh` | Reminds you to update TODO.md when phase status changes. |

### Disabling hooks

If a hook misfires or you need to bypass it temporarily:

```sh
CLAUDE_HOOKS_DISABLED=1 claude
```

Every hook script honors this env var and exits cleanly without firing.

For per-user overrides that don't get committed, drop a `.claude/settings.local.json` (already gitignored) — it merges over `.claude/settings.json`, with local taking precedence.

### Reporting false positives

If a hook blocks or warns on something it shouldn't, please file an issue with:
- The exact hook output.
- The tool input (tool name, file path, command).
- What you expected to happen.

We'd rather loosen a regex than have a contributor disable hooks wholesale.

## Slash commands

Project-local slash commands live under [`.claude/skills/`](./.claude/skills/). Invoke them in a Claude Code session:

| Command | Use when |
|---|---|
| `/check-scope` | Verifying a branch stays inside PRD §3.1 Committed Scope. |
| `/spec-consistency` | After editing PRD or any spec; cross-checks PRD ↔ TODO ↔ specs. |
| `/refresh-cassette <provider>` | Refreshing test cassettes for one provider (gated on `OMW_CASSETTE_REFRESH=1`). |
| `/release-checklist <version>` | Walking the pre-release checklist for a tag. |
| `/byorc-protocol-check` | Reviewing a PR that touches `crates/omw-remote/` or `specs/byorc-protocol.md`. |

All commands are read-only. None of them modify files, push to remote, or run real-API calls without explicit guards.

## Pull requests

Before opening a PR:

1. Run `/check-scope` and `/spec-consistency` and read their output.
2. If you changed PRD §3.1, update TODO.md in the same PR (CLAUDE.md §5).
3. If you added an endpoint to `crates/omw-remote/`, add a contract test and a fuzz target (see `specs/test-plan.md` §1.2 and §3.1).
4. PR title format: `<type>(<scope>): <subject>` — e.g. `feat(omw-agent): add OpenAI streaming retry`.

Reviewers will use the same slash commands during review.
