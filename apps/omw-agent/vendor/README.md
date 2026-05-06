# Vendored pi-agent-core

This directory is a checked-in copy of the [pi-agent-core](https://github.com/badlogic/pi-mono/tree/main/packages/agent) kernel from the pi-mono monorepo.

## Pin

- **Source repo**: `https://github.com/badlogic/pi-mono.git`
- **Pinned commit**: `fe1381389de87d2620af5d7e46d00f76f4e65274`
- **Vendored package**: `packages/agent/src/{agent-loop,agent,index,proxy,types}.ts` (5 files, ~2k LOC)
- **License**: MIT (Mario Zechner). Preserved verbatim in `pi-agent-core/LICENSE`.

## What is NOT vendored

- `@mariozechner/pi-ai` — this is consumed as a pinned npm dependency (see `apps/omw-agent/package.json`). The provider layer pulls in heavy SDKs (`@anthropic-ai/sdk`, `openai`, `@google/genai`, `@aws-sdk/client-bedrock-runtime`, `@mistralai/mistralai`, etc.) which are unchanged whether vendored or installed; npm install is simpler.
- `@mariozechner/pi-coding-agent`'s `bash.ts` — deeply coupled to pi-coding-agent's TUI internals (`pi-tui`, theme, keybinding hints, render utils). The omw bash tool in Phase 5 implements pi-agent's tiny `BashOperations`-shaped interface from scratch against the visible warp pane.

## Refresh ritual

`bash apps/omw-agent/scripts/refresh-pi-mono.sh` rsyncs the agent kernel from the `vendor/pi-mono` submodule into `apps/omw-agent/vendor/pi-agent-core/`. Manual; not run in CI.

To bump the pin:

```sh
cd vendor/pi-mono
git fetch origin
git checkout <new-commit>
cd ../..
bash apps/omw-agent/scripts/refresh-pi-mono.sh
# Update the "Pinned commit" line above.
# Bump @mariozechner/pi-ai version in apps/omw-agent/package.json to match.
# Update LICENSE if upstream license header changes.
# Re-run apps/omw-agent: npm install && npm run typecheck && npm test.
```

## Do not edit `pi-agent-core/*.ts` directly

If we need to modify the kernel — for example, to thread approval state through `beforeToolCall` differently than upstream — propose the change upstream first. The vendor directory is for *importing*, not *patching*. Patches against the vendored copy will silently disappear on the next refresh.
