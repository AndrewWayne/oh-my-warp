# omw Threat Model

Status: Draft v0.1
Last updated: 2026-04-29
Owners: TBD

This spec codifies the actors, attack surfaces, and invariants that omw is designed against. It is the source of truth referenced by [PRD §11](../PRD.md#11-security--privacy), [`specs/byorc-protocol.md`](./byorc-protocol.md), and [`specs/test-plan.md`](./test-plan.md) (security-tier property and fuzz targets).

It is **not** a pen-test report or a security audit. Both are external deliverables (PRD §11.5, Tier D in test-plan). This document defines what those reviews should be measured against.

---

## 0. Goals & Non-Goals

### Goals

- Enumerate the actors who interact with omw, including hostile ones.
- Map each surface omw exposes (loopback, tailnet, disk, OS APIs, provider HTTPS) to the actors who can reach it.
- State the invariants that hold across all v1.0 builds, regardless of feature-flag state.
- For each invariant, name the mechanism that enforces it and the test target that proves it.
- Make assumptions explicit so that every "we don't defend against X" decision is auditable.

### Non-Goals (v1)

- Anti-malware on the host (PRD §11.1: host machine is trusted).
- Multi-user / multi-tenant isolation on a single host.
- Defending against a compromised LLM provider exfiltrating prompt content (Privacy Mode is Beyond v1; v1 docs the risk).
- Defending against the user themselves (no DRM, no anti-fork measures).
- Side-channel attacks (timing, power, cache).
- Hardening Tailscale itself (we trust their transport).

---

## 1. Trust Assumptions

These are the axioms the rest of the model rests on. Every one is a candidate to be revisited in Beyond v1.

| Assumption | Why we make it | What changes if it breaks |
|---|---|---|
| The host machine is trusted | omw is a developer tool; the user already runs editors, package managers, shells with full host access. Defending against host-level malware is a different product. | We'd need sandboxed execution, signed binaries, hardware attestation. None in v1. |
| The OS keychain is trusted | We rely on the keychain's process-isolation and at-rest encryption. | We'd need our own at-rest encryption with a user-supplied passphrase. |
| Tailscale's transport is auth'd and confidential | Tailscale provides WireGuard-grade transport. We add app-layer auth on top, not in place of. | Public-internet exposure path becomes mandatory; threat model expands by an order of magnitude. |
| The user can read and act on UI prompts | Approval-mode defaults assume the user *sees* the prompt and decides. A blind-approve user is out of scope. | We'd need typed/no-default-button confirmation UX, hardware-button approvals. |
| LLM providers process but do not retain the user's prompts beyond their stated retention window | We log when the user sends data; we don't second-guess provider terms. | Privacy Mode (Beyond v1) hard-blocks providers; runtime routing rules can downgrade per-task. |
| The local SQLite + audit JSONL files are readable only by the user's OS account | We rely on filesystem permissions, not application-level ACLs. | We'd need encrypted-at-rest storage with a separate key. |

---

## 2. Actors

Each actor is a who or what that interacts with omw, listed with their **capability** (what they can do) and their **assumed intent** (benign, opportunistic, or hostile).

### 2.1 Benign actors

- **The user** (host owner).
  - Capability: full local control. Configures providers, runs agents, approves actions.
  - Intent: benign. Out of scope to defend against.

- **A device the user has paired** (their phone, second laptop).
  - Capability: bounded by capability scopes assigned at pair time. Default `read_only`.
  - Intent: benign, but **may be lost or stolen** — see §2.3.

- **The forked omw GUI process**.
  - Capability: in-process holder of PTYs, shell processes, the executor registry. Talks to `omw-server` over loopback.
  - Intent: benign. Same trust as the user.

- **`omw-server`, `omw-agent`, `omw-remote`** (sibling processes).
  - Capability: in-process owners of audit, agent sessions, pairing per the [component ownership map](../PRD.md#83-component-ownership-map).
  - Intent: benign. Subject to the invariants in §4.

### 2.2 Opportunistic actors

- **An LLM provider's logging pipeline.**
  - Capability: sees prompts, completions, tool-call arguments the user sent it. May retain per its terms.
  - Intent: opportunistic — not actively malicious, but its retention can become a leak source.
  - Mitigation: visible cost UX makes provider use explicit (PRD §5.1); Privacy Mode (Beyond v1) for hard-block.

- **An MCP server the user installed.**
  - Capability: sees tool-call inputs routed to it; can return arbitrary outputs. Runs as a subprocess (stdio) or hits an HTTP endpoint.
  - Intent: opportunistic. The user picks which servers to load; bundled servers go through code review.
  - Mitigation: approval mode applies to any tool — including MCP-provided tools — that writes/executes/networks. Audit captures every call. Beyond v1: per-tool sandboxing.

- **An AGPL fork distributor** redistributing our binary.
  - Capability: republishes a possibly modified omw build. AGPL allows this.
  - Intent: opportunistic. May or may not be honest.
  - Mitigation: no embedded secrets, no per-instance keys baked into binaries, no hosted entitlement system to impersonate. The fork distribution is an upstream concern, not an omw threat surface (PRD §11.1).

### 2.3 Hostile actors

These are the actors we actively defend against in v1.

- **A lost or stolen paired device.**
  - Capability: holds the device's Ed25519 private key + capability tokens. Can sign requests as that device until revoked.
  - Mitigation: per-device pairing; `omw pair revoke <id>` propagates within 1 second (invariant I-7); audit logs the device id on every action so blast-radius is computable.
  - Reference: [`specs/byorc-protocol.md`](./byorc-protocol.md) §6 (revocation), §3 (pairing).

- **A malicious app on the same tailnet.**
  - Capability: can reach `https://hostname.tailnet.ts.net` (Tailscale Serve), can attempt HTTP and WS handshakes against `omw-remote`. Cannot read traffic between the user's paired devices and `omw-remote`; Tailscale handles transport crypto.
  - Mitigation: app-layer signature on every request and WS handshake (invariant I-3); per-frame token auth on WS (invariant I-9); origin pinning (invariant I-10); replay window + nonce (invariant I-4).
  - Reference: [`specs/byorc-protocol.md`](./byorc-protocol.md) §4 (request signing), §7 (WS framing).

- **A malicious app on the same host that did not get keychain access.**
  - Capability: can connect to `127.0.0.1:8787` (the loopback listener). Can attempt the same handshake as a tailnet app.
  - Mitigation: same as the tailnet case — loopback is **not** a trust zone. Pairing is required regardless of source.
  - Note: this is why `omw-remote`'s loopback listener uses the same auth as the tailnet listener. There is no "trusted localhost" path.

- **A network attacker between the user's device and Tailscale's coordination server.**
  - Capability: can drop packets, block connectivity. Cannot decrypt WireGuard tunnels.
  - Mitigation: out of scope — Tailscale's transport assumption (§1).

- **A malicious app on a paired device** (e.g. a malicious browser extension on the user's phone).
  - Capability: can read what the Web Controller renders, can drive its UI, can read service-worker cached state.
  - Mitigation: paired device is treated as an extension of the user (§2.1). If the device is compromised at the OS level, that is out of scope.
  - Note: this is the same trust model as a malicious browser extension on the user's primary machine. Documented, not defended against.

- **A malicious or buggy MCP server the user installed.**
  - Capability: returns arbitrary tool outputs, including outputs designed to convince the model to do unsafe things ("prompt injection via tool result").
  - Mitigation: approval mode covers tools that *write/execute/network*. The agent reading a malicious tool result is not a write — but any action the model takes *in response* is a tool call subject to approval. v1 relies on the approval gate, not on model-level filtering.
  - Open question: should we surface MCP tool-result content to the user verbatim (Beyond v1)?

---

## 3. Attack Surfaces

Each surface is a place where one of the actors above can interact with omw. We list the surface, who can reach it, and what they can attempt.

### 3.1 `omw-server` loopback HTTP / GraphQL / WS (`127.0.0.1:<random>`)

- **Reachable from:** the omw GUI process; `omw-agent`; `omw-remote`; any other process on the host.
- **Surface:** §9.1 endpoints (identity, providers, agent sessions, settings, audit-append, GraphQL `/graphql/v2`).
- **Threats:**
  - Same-host malicious process attempting to read/write provider config, start agent sessions, or forge audit entries.
  - Cross-process race conditions on the audit writer.
- **Mitigations:**
  - Bind to `127.0.0.1` only; never `0.0.0.0`.
  - Per-process loopback token: `omw-server` writes a one-time token to a 0600 file on a path readable only by the user; sibling processes read and present it on every request.
  - Single-writer SQLite for audit (invariant I-8) plus the hash chain (I-12) — a forged append is detectable on next verify.
- **Beyond v1:** UNIX-domain sockets with peer-credential checks (Linux/macOS).

### 3.2 `omw-remote` tailnet HTTP / WS (`tailnet`-exposed via Tailscale Serve)

- **Reachable from:** any device on the user's tailnet, including malicious-app and lost-device cases (§2.3).
- **Surface:** §9.2 endpoints (sessions, agent tasks, pair, audit) and WS streams (`/ws/v1/pty/:id`, `/ws/v1/agent/:id`, `/ws/v1/events`).
- **Threats:** the largest in v1. Auth bypass, replay, capability-scope escalation, WS frame injection, origin spoof, pairing-token brute force.
- **Mitigations:** the entire `byorc-protocol.md` exists for this surface. Summary:
  - Per-device Ed25519 keypairs (invariant I-2).
  - Signed requests + signed WS handshakes + per-frame signed messages (I-3, I-9).
  - Capability tokens scope per-route access (I-11).
  - Nonce + 30s window (I-4).
  - Origin pinning (I-10).
  - Hashed single-use pairing tokens (I-5).
  - Loopback listener uses the same auth (no localhost-trust shortcut).
- **Test targets:** byorc-protocol contract tests (test-plan §A.2), BYORC validator fuzzer (test-plan §3.1), pairing property tests (test-plan §2.5).

### 3.3 Audit log on disk

- **Reachable from:** any process running as the user. Read-only for non-`omw-server` processes by convention (we don't enforce filesystem ACLs beyond default).
- **Threats:**
  - Tamper: another process or hostile actor edits a JSONL line to remove evidence.
  - Truncate: deletion of recent lines or whole files.
  - Reorder: lines moved between days.
  - Plaintext secret leakage: a tool argument containing an API key gets logged.
- **Mitigations:**
  - Append-only via the `omw-server` writer (I-8).
  - SHA-rolling hash chain across lines + across daily files (I-12).
  - Redaction rules applied before write (I-13); see `redaction_rules` table.
  - Property tests cover tamper / reorder / truncate detection (test-plan §2.1).
  - JSONL line parser fuzzed (test-plan §3.4).

### 3.4 OS keychain

- **Reachable from:** `omw-keychain` library, in-process only. The OS gates access by application identity (macOS) or session (Linux Secret Service / Windows DPAPI — Beyond v1).
- **Threats:**
  - macOS: another app prompting the user for keychain unlock and getting the secret.
  - Plaintext exposure if a developer regresses and writes a key to a config file or env var.
- **Mitigations:**
  - Invariant I-1: no plaintext keys on disk. Code review + a CI grep for known key prefixes (`sk-`, `anthropic-`, etc.) in any committed file.
  - Config files reference keychain entries by name only (`keychain:omw/openai`).
  - macOS-only in v1; Linux/Windows have their own threat models, deferred to Beyond v1.

### 3.5 Provider HTTPS surface

- **Reachable from:** `omw-agent` only.
- **Threats:**
  - Provider sees the user's prompts; retention policy is the provider's. Out of scope to defend.
  - Provider returns malformed or malicious responses (e.g. attempt to confuse parsers).
  - SSRF if a provider URL field is user-controllable and misused.
- **Mitigations:**
  - Strict provider-URL allowlist or scheme check in `omw-config` (no `file://`, no internal IPs by default).
  - Provider response parsers tolerant of malformed input — fuzzed via the MCP message parser pattern (test-plan §3.3 covers MCP; provider responses get a v0.1 follow-up).
  - Cassette tests cover error/malformed cases (test-plan §4.3).

### 3.6 MCP server transports

- **Reachable from:** `omw-agent`.
- **Threats:** see §2.2 (opportunistic) and §2.3 (malicious server). Stdio JSON-RPC and HTTP MCP are both in v0.2 scope.
- **Mitigations:**
  - JSON-RPC envelope parser fuzzed (test-plan §3.3) — never panics on arbitrary JSON.
  - MCP-tool calls go through the same approval pipeline as built-in tools.
  - Beyond v1: per-MCP-server resource limits (CPU, memory, time) and FS-scope sandboxing.

### 3.7 ACP server interface

- **Reachable from:** any local editor speaking ACP (Zed, etc.).
- **Threats:** a malicious editor invoking the agent on the user's behalf.
- **Mitigations:**
  - The user runs the ACP server themselves (`omw acp-agent`); it's not exposed automatically.
  - Same approval-mode policy as any other agent session.
  - Beyond v1: per-editor capability tokens.

### 3.8 The omw GUI process

- **Reachable from:** the local desktop session.
- **Threats:** another GUI app injecting events (covered by OS-level UI isolation), a screen-capture app reading rendered content (out of scope per host-trust assumption).
- **Mitigations:** rely on OS-level UI isolation. Document the assumption.

---

## 4. Invariants

The invariants are the load-bearing properties. Every one has an enforcement mechanism and a test target. They MUST hold for any build that ships.

| ID | Invariant | Enforcement | Test target |
|----|-----------|-------------|-------------|
| I-1 | No plaintext keys on disk. | `omw-keychain` is the only path; config files reference `keychain:` URIs. CI grep for known key prefixes. | Unit test in `omw-config` that rejects literal `sk-`/`anthropic-` values. |
| I-2 | Every paired device has a unique Ed25519 keypair generated on-device at pair time. | `omw-remote` pairing handshake. Server stores public key only. | byorc-protocol contract test. |
| I-3 | No unauthenticated remote request — every HTTP request and WS handshake is signed. | Signed-request middleware in `omw-remote`. | Contract test (negative case: 401); BYORC validator fuzzer. |
| I-4 | Replay defense: nonce + 30-second window. Replays rejected and logged to `request_log`. | Nonce store with TTL = window. | Contract test (replay → 403); pairing property test. |
| I-5 | Pairing tokens are single-use, hashed at rest, and expire (TTL 10 min). | Token-redeem handler hashes input, looks up by hash, rejects on second use. | Pairing property test (`single-use`, `expiry-respected`, `token-stored-hashed`). |
| I-6 | New paired device defaults to `read_only`. Upgrade requires explicit user action on the host. | `omw-policy` default; `omw pair` does not accept a higher mode at create time. | Unit test in `omw-policy`. |
| I-7 | Revocation propagates within 1 second. Active WS connections drop; subsequent requests rejected. | `omw-remote` revocation list checked on each frame; broadcast tear-down on revoke. | E2E Journey B test (test-plan §B.2) measures latency. |
| I-8 | Audit writes go through `omw-server` only. SQLite serializes; JSONL appended atomically. | Single-writer architecture (PRD §8.3). | Contract test on `POST /api/v1/audit/append`. |
| I-9 | WS frames are individually authenticated. A handshake-only signature is not sufficient. | Per-frame token + sequence number; rejected frames close the connection. | Contract test; fuzz target on frame parser (Beyond v0.4 extension to test-plan §3.1). |
| I-10 | Origin pinning at WS handshake. | Origin allowlist; reject otherwise. | Contract test (negative). |
| I-11 | Capability tokens scope per-route access. A token authorized for read-only PTY cannot satisfy an agent endpoint. | Per-route capability check in middleware. | BYORC validator fuzz target (test-plan §3.1) explicitly asserts this. |
| I-12 | Audit hash chain. Tamper / reorder / truncation detected by `omw audit verify`. | SHA over each line including the previous SHA; per-day file pointer chain. | Audit-chain property tests (test-plan §2.1). |
| I-13 | Default redaction rules strip API keys, `.env` values, and known-secret patterns before audit write. | `omw-audit` redaction layer applied to every entry. | Redaction property tests (test-plan §2.3). |
| I-14 | No outbound telemetry to any omw-controlled endpoint. Period. | No omw-controlled endpoints exist. CI grep for any URL referencing telemetry-like hostnames. | Code review + grep test. |
| I-15 | Public-internet exposure is opt-in. `omw-remote` listens on loopback by default; tailnet exposure requires explicit `tailscale serve` invocation by the user. | Default config; `omw remote start` does not invoke `tailscale serve`. | Contract test on default-listen address. |
| I-16 | No silent destructive actions. Default approval mode is `ask_before_write`. | `omw-policy` default. Trusted mode requires explicit per-device upgrade. | Approval-policy property tests (test-plan §2.4). |

---

## 5. Mapping Surfaces × Invariants

| Surface | Load-bearing invariants |
|---|---|
| `omw-server` loopback (§3.1) | I-8, I-12, I-14 |
| `omw-remote` tailnet (§3.2) | I-2, I-3, I-4, I-5, I-6, I-7, I-9, I-10, I-11, I-15 |
| Audit log (§3.3) | I-8, I-12, I-13 |
| Keychain (§3.4) | I-1 |
| Provider HTTPS (§3.5) | I-1 (no key on disk), I-14 (no outbound telemetry) |
| MCP transports (§3.6) | I-16 (approval gate) |
| ACP server (§3.7) | I-16 |
| GUI process (§3.8) | I-1, I-14 |

---

## 6. Out of Scope (v1)

Documented explicitly so reviewers don't assume coverage we don't have.

- **Multi-user / multi-tenant on a single host.** One host = one user.
- **Federated identity** (SSO/SAML).
- **Hardware-key approvals** (YubiKey).
- **Encrypted-at-rest audit log.** We rely on disk-level encryption.
- **Per-tool MCP sandboxing.** v1 trusts what the user installs.
- **Defending against the user.** No DRM, no anti-fork measures.
- **Public-internet exposure paths.** Documented as a non-goal in PRD §3.2; if revisited Beyond v1, this threat model expands significantly.
- **Side channels** (timing, cache, power).
- **Supply-chain compromise of crates we depend on.** Out of scope; we rely on `cargo audit` in CI as a backstop, not a defense.

---

## 7. Open Questions

- Loopback authentication — the per-process token file approach (§3.1) is a v0.1 design. Should `omw-server` use UNIX-domain sockets with peer-credential checks instead? Decide during v0.1.
- Should `omw-config` include a SSRF allowlist for OpenAI-compatible base URLs by default, or accept any URL the user types? Decide during v0.1.
- MCP tool-result surfacing to the user (vs only model-internal): Beyond v1 RFC.
- Linux/Windows keychain models — the §3.4 threat model is macOS-specific. Beyond v1.
- Approval-prompt UX hardening (no default-button, typed confirmation for destructive ops): v1.0 polish vs Beyond v1.

---

*End of threat model v0.1.*
