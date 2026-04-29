---
name: byorc-protocol-check
description: Verify changes touching crates/omw-remote/ or specs/byorc-protocol.md against the BYORC protocol invariants. Use when reviewing a PR that modifies the remote daemon, the pairing flow, the signed-request validator, or the protocol spec itself. Read-only review aid.
tools: Bash, Read, Grep
---

# BYORC Protocol Check

Cross-checks a diff against the BYORC protocol invariants. Read-only — does not modify code or spec.

## Triggers (when this skill should run)

This skill is most valuable on PRs that touch:

- `crates/omw-remote/**`
- `specs/byorc-protocol.md`
- `apps/web-controller/src/auth/**` or any client-side signing code
- `crates/omw-policy/src/capabilities*.rs` (capability scoping)

## Invariants to verify

From [PRD §11.2](../../../PRD.md#112-invariants-must-hold-for-all-builds) and [`specs/byorc-protocol.md`](../../../specs/byorc-protocol.md):

1. **No unauthenticated remote requests.** Every HTTP request and WS handshake validated through the signed-request validator.
2. **Replay protection.** Per-request nonce + 30s window. Validator rejects expired or duplicate nonces.
3. **Capability scoping.** Tokens authorized for one route cannot be replayed against another. A read-only PTY token cannot satisfy an agent endpoint.
4. **Per-frame WS auth.** WebSocket frames carry an auth token, not just the handshake.
5. **Origin pinning.** Handshake validates Origin header against pairing record.
6. **Pairing tokens single-use, hashed at rest.** DB read of `pairings` row never reveals the raw token.
7. **Revocation propagates within 1s.** Active connections drop on revoke.
8. **No silent destructive actions.** Default approval mode `ask_before_write`; `trusted` requires explicit per-device upgrade.

## Procedure

1. Determine the diff range. Default `main...HEAD`. Accept `$ARGUMENTS` as override.
2. Run `git diff <range> -- crates/omw-remote/ specs/byorc-protocol.md apps/web-controller/src/auth/` to get the change set.
3. For each invariant above, search the diff for evidence of:
   - **Compliance** (e.g. signature validation added on a new route).
   - **Violation** (e.g. a route added without going through the validator).
   - **Unclear** (e.g. signature mentioned but enforcement not visible in diff).
4. Cross-reference: if `specs/byorc-protocol.md` was edited, verify implementation in `crates/omw-remote/` reflects the new spec. If only implementation changed, verify the spec wasn't silently invalidated.
5. Look for red flags:
   - New HTTP/WS routes without explicit `require_signed_request!` (or equivalent) macro/middleware.
   - Test code that disables validation without a `#[cfg(test)]` gate.
   - Hardcoded keys, nonces, or timestamps in non-test files.
   - Logging of the raw signature or token value.
6. Output a per-invariant report. Do NOT edit anything.

## Output format

```
BYORC protocol check — diff: <range>

Invariants:
  [✓|✗|?] No unauthenticated remote requests
  [✓|✗|?] Replay protection (nonce + 30s window)
  [✓|✗|?] Capability scoping
  [✓|✗|?] Per-frame WS auth
  [✓|✗|?] Origin pinning
  [✓|✗|?] Pairing tokens single-use + hashed
  [✓|✗|?] Revocation propagates within 1s
  [✓|✗|?] Default approval = ask_before_write

Red flags found:
  - <path:line> — <description>

Verdict: COMPLIANT | NEEDS_REVIEW (N concerns)
```

## Notes

- During Phase 0 / before v0.4, `crates/omw-remote/` and `specs/byorc-protocol.md` may not exist. This skill should print: `"BYORC implementation not started yet — nothing to check."` and exit cleanly.
- This skill is advisory. A green verdict does NOT replace external protocol/design review (per [PRD §11.5](../../../PRD.md#115-external-review-tiered)).
