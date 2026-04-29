---
name: refresh-cassette
description: Walk the provider cassette refresh ritual from specs/test-plan.md §4.4. Use when the user explicitly asks to refresh cassettes for a provider, or when CI flags cassette drift. Argument is the provider name (openai, anthropic, openai-compat, ollama). REFUSES to make real API calls unless OMW_CASSETTE_REFRESH=1.
tools: Bash, Read
---

# Refresh Cassette

Guided walkthrough of the cassette refresh procedure for one Tier-1 provider. Real API calls cost real money — this skill is gated behind an explicit env var.

## Hard guards

Before doing anything:

1. If `$ARGUMENTS` is empty, prompt for the provider name (`openai`, `anthropic`, `openai-compat`, `ollama`). Do not pick a default.
2. If `OMW_CASSETTE_REFRESH=1` is NOT set in the environment, refuse:
   - Print: `"Refusing to run real-API recordings without OMW_CASSETTE_REFRESH=1. Set the env var, then re-run. This safeguard prevents accidental billing."`
   - Exit without making any API calls.
3. If the provider crate / scripts directory doesn't exist yet (Phase 0–v0.1 may not have it), print the manual checklist (§4 below) and exit. Do not attempt to run anything.

## Procedure (only after guards pass)

1. Read [`specs/test-plan.md` §4.3](../../../specs/test-plan.md) to enumerate the required cassette set for `$ARGUMENTS`. Currently 8 cassettes per provider: `simple-response`, `streaming-with-thinking`, `tool-call-shell`, `tool-call-with-fs-write`, `multi-turn`, `error-rate-limit`, `error-malformed`, `usage-reconciliation`.
2. Confirm with the user that they want to record all 8 (or a subset). Default: all.
3. Invoke `scripts/refresh-cassettes.sh $ARGUMENTS` if it exists. (This script is built in v0.1 and will not exist during Phase 0.)
4. Diff the regenerated cassettes against the previous version. Surface:
   - Token counts that drifted by more than 10%.
   - Tool-call shape changes.
   - New or removed fields in the response.
5. Print a PR description draft summarizing the drift, suitable for the cassette-refresh PR.

## Manual checklist (when scripts don't exist yet)

If `scripts/refresh-cassettes.sh` is missing, print this checklist for the human to follow:

```
Manual cassette refresh — provider: <name>
1. Set provider API key in keychain (or as env var locally).
2. Run the cassette runner in record mode:
   OMW_CASSETTE_RECORD=1 cargo test -p omw-provider-<name> --test cassettes
3. Inspect the regenerated JSON files under
   crates/omw-provider-<name>/tests/fixtures/cassettes/
4. Verify each cassette covers the 8 required scenarios.
5. Open a PR titled "test(cassettes): refresh <name> Q<n> 20<yy>"
6. Reviewer confirms semantic equivalence per test-plan.md §4.4.
```

## Notes

- This skill never auto-executes the recording. The recording is destructive in the sense that it overwrites prior cassettes and burns provider tokens. Always require explicit human confirmation.
- For OSS contributors: do NOT use the maintainer's keys. Refresh PRs from contributors should use their own keys; reviewer verifies.
