# omw Test Plan

Status: Draft v0.1
Last updated: 2026-04-29
Owners: TBD

This spec defines how omw is tested across all phases. It is referenced by [PRD §16](../PRD.md#16-success-metrics) and gates each phase's exit criteria ([PRD §13](../PRD.md#13-phased-roadmap)).

The plan is structured by **trust tier**, not by component — each tier has different cadence, blast radius, and ownership.

---

## 0. Goals & Non-Goals

### Goals

- Every commit lands with a fast, deterministic green signal (Tier A).
- Security-critical surfaces (audit chain, BYORC protocol, redaction, approval policy) are covered by property and fuzz tests, not just example tests.
- Provider integrations are testable without spending money or hitting flaky APIs (cassette replay).
- Fork health is continuously monitored against upstream Warp.
- Pre-release readiness is a checklist, not a vibe.

### Non-Goals (v1)

- Automated UI tests for the forked GUI (we run upstream's tests as-is; see §5).
- Performance benchmarks (no SLOs defined yet).
- Internationalization / localization tests (English only in v1).
- Upgrade / migration tests (no v0 → v1 path until v1 ships).
- Hostile-network testing (DPI, MITM beyond what Tailscale provides).
- Real-LLM CI calls (cost + flakiness; replaced by cassette replay).

---

## 1. Trust Tiers

### Tier A — every commit (must stay <5 min CI wall)

Blocks merge if red. Runs on macOS-latest and Linux-latest.

#### A.1 Unit
- Standard Rust `#[test]` per crate.
- Pure logic only — no network, no filesystem outside `tempdir`.
- Tokenizer cost estimation, hash-chain math, redaction rules, schema validation, config parsing.

#### A.2 Contract
- `omw-server` HTTP/GraphQL: schema-driven test that asserts every documented endpoint accepts documented inputs and emits documented outputs.
- `omw-remote` HTTP/WS: protocol conformance against [`specs/byorc-protocol.md`](./byorc-protocol.md). Each route documented in the spec must have a contract test asserting positive *and* negative cases (auth failure, replay, scope violation).

#### A.3 Provider-mocked integration
- Each provider crate runs against **recorded cassettes** under `tests/fixtures/cassettes/<provider>/`.
- Cassette format: see §4.
- No real LLM calls in CI ever.
- The shared cassette runner asserts: streaming chunk timing, tool-call shape, usage-token capture, retry/backoff behavior.

#### A.4 Frontend unit (Web Controller)
- Vitest for components and routing logic.
- Mocked WS client; no real `omw-remote`.

### Tier B — nightly + pre-release

Doesn't block merge but a 7-day red streak halts release. Runs on macOS-latest unless noted.

#### B.1 End-to-end Journey A
- Full CLI BYOK flow against cassetted providers.
- Walk: `omw setup` → `omw provider add` → `omw ask` → assert streamed output → assert cost row in SQLite → assert audit entry.

#### B.2 End-to-end Journey B (protocol only)
- Synthetic Web Controller client (Rust async) connects to `omw-remote` over loopback.
- Tailscale Serve is **not** invoked; we test the BYORC protocol independently of Tailscale's transport.
- Walk: pair → attach to GUI terminal session → send keystroke → receive output → request agent run → approve → verify audit entry.

#### B.3 Property tests
See §2 catalog. Run with 1000 cases per property nightly.

#### B.4 Fuzz tests *(introduced v0.4)*
See §3 catalog. Run for 5 min per target nightly under `cargo-fuzz`. Linux runner.

#### B.5 Snapshot-rebuild smoke *(per-restrip, not nightly)*

Replaces the v0.1-spec "fork-rebase smoke" now that upstream tracking moved to the tracked-snapshot model (`specs/fork-strategy.md` v0.2). This check runs **only when `vendor/warp-stripped/` is restripped** — the snapshot model has no nightly CI.

- After a restrip PR is opened, CI builds `vendor/warp-stripped/` (`cargo build -p warp --bin warp-oss --features omw_local`) on macOS and Linux.
- CI runs upstream Warp's own `cargo test --workspace` inside `vendor/warp-stripped/` against the new snapshot — smoke signal that omw's strip didn't break upstream invariants.
- CI runs the outer workspace's tests (`cargo test --workspace` from repo root) to catch path-dep wiring breakage (e.g., `omw-server` embedded into the stripped tree).
- A failed restrip PR is iterated on by the maintainer until both checks pass; no `upstream-conflict` issues filed (restrip is not a background process — it's the active PR's responsibility).

### Tier C — pre-release manual

Required for every released version (§7).

- Homebrew clean-install on a fresh macOS VM.
- Real-Tailscale + real-phone Web Controller smoke (v0.4+).
- Forked GUI rendering on macOS (visual eyeball).
- PWA on iOS Safari + Android Chrome (manual; iOS WebKit doesn't CI cheaply).

### Tier D — external (per [PRD §11.5](../PRD.md#115-external-review-tiered))

- **Protocol/design review** before v0.4 implementation begins.
- **Implementation security review** before v1.0 ship.

Both are paid scoped engagements (week-long retainer), not full audits, until budget allows otherwise.

---

## 2. Property Test Catalog

Property tests use `proptest`. Introduced phase-by-phase: cost reproducibility in v0.1; audit + redaction + approval in v0.2; pairing in v0.4.

### 2.1 Audit chain integrity (`omw-audit`)

- **chain-validates-after-N-appends** — for any sequence of N appends with arbitrary content, `verify(chain) == true`.
- **single-byte-tamper-detected** — for any single-byte mutation in any line of any chain, `verify(chain) == false` and the corrupted line is identified.
- **reorder-detected** — any non-trivial reordering of lines fails verification.
- **truncation-detected** — truncation at any point fails verification.
- **append-only** — there is no API path that writes anywhere except the next line.

### 2.2 Cost reproducibility (`omw-agent`)

- **price-snapshot-deterministic** — for any historical transcript with a `pricing_version_id`, `recompute(cost)` returns byte-exact match against `stored.cost`.
- **pricing-version-selection** — for any transcript at time T, the version chosen is `max(version with effective_at ≤ T)`.
- **estimate-vs-reported-bounded** — for any cassette response with both estimate and reported usage, `|estimate − reported| / reported < 0.10` (10% tolerance is generous for v1; tightens as tokenizers improve).

### 2.3 Redaction (`omw-audit`)

- **secret-never-leaks** — for any input string containing a redaction-rule-matched substring (`sk-...`, `anthropic-...`, `KEY=value` in `.env` shape, custom user patterns), the audit emit does NOT contain the secret.
- **redaction-idempotent** — `redact(redact(x)) == redact(x)`.
- **non-secret-preserved** — for any input string with no matched substrings, `redact(x) == x`.

### 2.4 Approval policy (`omw-policy`)

- **read-only-rejects-writes** — in `read_only` mode, every write/exec/network tool call returns `Decision::Reject`.
- **ask-defers-on-write** — in `ask_before_write` mode, every write/exec/network tool call returns `Decision::AskUser`.
- **trusted-approves-all** — in `trusted` mode, every tool call returns `Decision::Approve`.
- **allowlist-bypass** — a tool call matching the allowlist returns `Decision::Approve` regardless of mode.
- **denylist-overrides-trust** — a tool call matching the denylist returns `Decision::Reject` even in `trusted` mode.

### 2.5 Pairing tokens (`omw-remote`, v0.4)

- **single-use** — redeeming a pairing token twice rejects the second redemption.
- **expiry-respected** — a token used after `expires_at` is rejected.
- **token-stored-hashed** — a database read returning the `pairings` row never reveals the raw token.

---

## 3. Fuzz Test Catalog

Targets registered with `cargo-fuzz`. Introduced at v0.4 alongside the protocol code. Each target runs 5 min nightly; CI fails on any new crash.

### 3.1 BYORC signed-request validator (`omw-remote`)

- **Target:** the function taking raw HTTP body + headers + signature and returning `Result<AuthedRequest, Error>`.
- **Properties asserted on every input:**
  - never panics
  - rejects malformed signatures with a structured error
  - rejects expired or replayed nonces
  - capability scope is enforced (a token scoped to read-only PTY cannot satisfy an agent endpoint)

### 3.2 Pairing token consumer (`omw-remote`)

- **Target:** `POST /api/v1/pair/redeem` body parser + redeemer.
- **Properties:** never panics; never logs the raw token; rejects all malformed inputs.

### 3.3 MCP message parser (`omw-agent`)

- **Target:** the JSON-RPC envelope parser used to receive MCP server messages.
- **Properties:** never panics on arbitrary JSON; cleanly rejects non-conforming envelopes.

### 3.4 Audit JSONL parser (`omw-audit`)

- **Target:** the per-line parser used by `omw audit search`.
- **Properties:** any random line either parses cleanly or returns a typed error; never panics; never reads outside its line bounds.

---

## 4. Provider Cassette Strategy

User decision: **100% mocked in CI**, refreshed on a quarterly cadence.

### 4.1 Cassette format

Each cassette is a JSON file capturing one HTTP exchange:

```json
{
  "request": {
    "method": "POST",
    "url_pattern": "https://api.openai.com/v1/chat/completions",
    "match_headers": ["authorization-prefix", "content-type"],
    "match_body_jsonpath": [
      "$.model",
      "$.messages[*].role",
      "$.tools[*].function.name"
    ]
  },
  "response": {
    "status": 200,
    "headers": { "content-type": "text/event-stream" },
    "stream_chunks": [
      { "delay_ms": 50, "data": "data: {...}\n\n" },
      { "delay_ms": 80, "data": "data: {...}\n\n" },
      { "delay_ms": 30, "data": "data: [DONE]\n\n" }
    ],
    "trailing_usage": { "prompt_tokens": 412, "completion_tokens": 87 }
  },
  "metadata": {
    "recorded_at": "2026-04-15",
    "real_provider": "openai",
    "real_model": "gpt-4o-2024-08-06",
    "purpose": "covers tool-calling with shell"
  }
}
```

### 4.2 Cassette runner

A small library crate `omw-test-cassette` provides:

- `Cassette::load(path)` — parse a cassette JSON.
- `MockServer::serve(&[Cassette])` — start an in-process HTTP server that matches incoming requests against cassettes.
- `MockServer::url()` — return base URL for the test to point its provider client at.
- Streaming support: chunks emitted with the recorded delays.
- Recording mode: when `OMW_CASSETTE_RECORD=1`, proxies real requests and writes new cassettes.

### 4.3 Required cassette coverage per provider

For each Tier-1 provider (OpenAI, Anthropic, OpenAI-compatible, Ollama):

- `simple-response` — single user prompt, single assistant reply.
- `streaming-with-thinking` — streamed response with chain-of-thought / thinking blocks (where supported).
- `tool-call-shell` — assistant requests a shell tool call.
- `tool-call-with-fs-write` — assistant requests a file-write tool call (exercises approval).
- `multi-turn` — conversation with multiple turns and tool calls.
- `error-rate-limit` — provider returns 429.
- `error-malformed` — provider returns malformed SSE / truncated stream.
- `usage-reconciliation` — response with usage tokens that the test verifies match the estimate.

### 4.4 Cassette refresh

- **Quarterly PR** refreshes cassettes against real APIs.
- **Refresh script:** `scripts/refresh-cassettes.sh <provider>` — runs the recording mode against the maintainer's keys, regenerates JSON, opens a PR.
- **Reviewer's job:** confirm semantic equivalence (token counts didn't drift wildly; tool-call shapes match).

### 4.5 Manual `omw provider test` ritual

At each release, a maintainer runs `omw provider test <provider>` against their personal keys for each Tier-1 provider. Human-in-the-loop check that real APIs still match cassettes. Logged in the release checklist (§7).

---

## 5. Forked GUI Test Strategy

User decision: **adopt upstream Warp's existing test suite as-is; no new GUI-specific tests in v1.**

### 5.1 What we run

- Upstream Warp's `cargo test --workspace` runs against our patched fork in CI.
- Upstream's `./script/presubmit` runs in pre-release.

### 5.2 What we don't run

- No new WarpUI snapshot tests for our patches.
- No Playwright/Selenium against the desktop app.
- No visual regression.

### 5.3 Failure handling

- Upstream test red on a restrip PR → triage in the PR itself (no separate `upstream-conflict` issue, since restrip is not a background process):
  - If their test is testing a behavior we accidentally broke → fix the strip in this restrip PR.
  - If their test is testing a behavior we *intentionally* changed → skip with `#[ignore = "omw-strip: <reason>"]` inside the new snapshot tree, with a commit-message trailer `Series-tag: omw/local-mode` per `specs/fork-strategy.md` §4.

### 5.4 Visual rendering

Tier C manual eyeball on macOS at each pre-release. No automation in v1.

---

## 6. CI Matrix

| Stage | Trigger | OS | Cadence | Wall budget |
|-------|---------|-----|---------|-------------|
| Tier A unit + contract | every push | macOS, Linux | per-commit | 5 min |
| Tier A provider cassettes | every push | macOS | per-commit | 3 min |
| Tier A frontend (Vitest) | every push | Linux | per-commit | 2 min |
| Tier B E2E A+B | nightly | macOS | 1×/day | 15 min |
| Tier B property | nightly | macOS | 1×/day | 10 min |
| Tier B fuzz (per target) | nightly | Linux | 1×/day | 5 min × N |
| Tier C snapshot-rebuild smoke | restrip PR | macOS, Linux | per-restrip | 30 min |
| Tier C manual | pre-release | manual | per-release | varies |
| Tier D external | gate | external | once per phase gate | vendor |

Linux Tier A runs unit-only — no integration tests yet (Linux app packaging is Beyond v1). It exists to catch macOS-specific code that would block Linux later.

---

## 7. Pre-Release Checklist

Each release commit must check:

- [ ] All Tier A green on the release commit.
- [ ] Tier B green for the past 7 nights (no regressions).
- [ ] Manual `omw provider test` against each Tier-1 provider with maintainer's real keys.
- [ ] Manual Homebrew clean-install on a fresh macOS VM (v1.0+).
- [ ] Real-Tailscale + real-phone Web Controller smoke (v0.4+).
- [ ] Snapshot-rebuild smoke green on the most recent restrip (or no restrip since the last release).
- [ ] No open sev-1 tracking issues.
- [ ] CHANGELOG.md updated.
- [ ] External review sign-off (v0.4: protocol; v1.0: implementation).

---

## 8. Per-Phase Test Commitments

Aligned with [PRD §13](../PRD.md#13-phased-roadmap). Each phase exits only when its commitments are met.

| Phase | Test commitments |
|-------|------------------|
| Phase 0 | This spec written, reviewed, merged. CI scaffold (Tier A skeleton on a hello-world crate). |
| v0.1 | Unit + contract for `omw-config`, `omw-keychain`, `omw-agent`, providers. Cassette runner library. Initial cassettes for all Tier-1 providers (`simple-response`, `streaming-with-thinking`, `tool-call-shell`, `usage-reconciliation`). Cost-reproducibility property test. |
| v0.2 | Audit chain property tests. Redaction property tests. Approval policy property tests. MCP message fuzzer. Audit JSONL fuzzer. |
| v0.3 | `omw-server` contract tests. Upstream Warp test suite green on patched fork. |
| v0.4-thin | `omw-remote` contract tests against `specs/byorc-protocol.md` (signed requests, replay defense, capability scopes). BYORC validator fuzzer. Pairing token property tests. `omw-pty` unit + integration. Web Controller (Vitest) for pairing + terminal pages. E2E pairing-then-shell smoke (pseudo-Journey B). `omw-server` internal session registry contract tests (now needed earlier — adopted into omw-remote during wiring 1). |
| v0.4-thin-polish | External-source SessionRegistry test (Gap 1). PtyController tap safety test inside `vendor/warp-stripped/`. Tailscale CLI fixture-based detection test (Gap 4). Multi-origin handshake tests (Gap 4). Reactive-status property test for OmwRemoteState (Gap 3). Modal smoke (Gap 2 — Warp UI test, Tier C manual if no harness). |
| v0.4-cleanup | WarpSessionBashOperations integration test. Web Controller agent + approvals + diff Vitest. Full E2E Journey B (pairing → terminal → agent → approval → audit). External protocol review sign-off (or merged review reconciliation if v0.4-thin proceeded in parallel). |
| v1.0 | Full pre-release checklist green. External implementation review sign-off. |

---

## 9. Test Ownership

Each crate owns its own tests. Cross-cutting test infrastructure lives in dedicated crates.

| Crate / app | Owns |
|-------------|------|
| `omw-config` | Unit |
| `omw-keychain` | Unit |
| `omw-agent` (pi-agent) | Unit, cost reproducibility property, provider cassette tests, WarpSessionBashOperations integration |
| `omw-policy` | Unit, approval policy property |
| `omw-audit` | Unit, audit chain property, redaction property, JSONL fuzz |
| `omw-acp` | Unit |
| `omw-server` | Unit, contract |
| `omw-remote` | Unit, contract, BYORC validator fuzz, pairing property (v0.4) |
| `omw-pty` | Unit |
| `omw-cli` | Unit, E2E Journey A |
| `omw-test-cassette` | Cassette runner library + recording mode |
| `apps/web-controller` | Vitest unit, E2E Journey B (protocol-only) |
| Forked `omw` GUI | Inherits upstream's tests; no additions |

---

## 10. Failure Response

| Signal | Severity | Response |
|--------|----------|----------|
| Tier A red on a PR | blocking | Fix before merge. |
| Tier B nightly red | high | File with `nightly-broken` label. Fix within 48h. Block release if ≥7 days red. |
| Snapshot-rebuild red on restrip PR | high | Address in the restrip PR before merging. No separate tracking issue; the PR is itself the work. If a single upstream change is genuinely incompatible, skip with rationale per `specs/fork-strategy.md` §3.2 step 6. |
| Cassette mismatch with real API | medium | Refresh cassette PR. Reviewer confirms semantic equivalence. |
| Fuzz target finds new crash | high | Reproducer added to corpus; root-cause and fix; re-fuzz to confirm. |
| External reviewer finds sev-1 | blocking | Block release. Fix + re-review. |

---

## 11. Open Questions

- Cassette refresh cadence — quarterly enough, or trigger on user reports of API drift?
- Real-LLM smoke at each release — maintainer pays out of pocket, or seek sponsorship?
- BrowserStack / Sauce Labs for iOS Safari — rent at v1.0 release, or stay manual indefinitely?
- Visual regression for the forked GUI in Beyond v1 — `cargo-test-screenshot`-style tool, or skip indefinitely?
- Performance budget — when do we start tracking it? (Likely Beyond v1 once we have real users.)

---

*End of test plan v0.1.*
