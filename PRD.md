# omw — Product Requirements Document

Status: Draft v0.2 — pre-implementation
Brand: **omw** (product, binary, wordmark)
Repo / community codename: **oh-my-warp** (this GitHub org/repo, homage to `oh-my-zsh`)
Owners: Shenhao Miao (project), TBD (engineering leads)
Last updated: 2026-04-29

---

## 0. TL;DR

**omw is a developer-owned, local-first fork of the open-source Warp terminal that replaces Warp's cloud dependencies with components the user controls — their own model keys, their own network, their own remote control surface, their own audit trail.**

Three pillars:

1. **BYOK** — bring your own LLM provider keys (OpenAI, Anthropic, OpenAI-compatible, Ollama in v1; Google/LM-Studio later). No omw cloud, no Warp cloud, no hosted entitlements.
2. **BYORC** — bring your own remote controller. Sessions, terminals, and agent runs are exposed over the user's Tailscale tailnet — never the public internet. Single-host pairing in v1; fleet UX is Beyond v1.
3. **Local-first agent platform** — an `omw-agent` binary that orchestrates LLMs, tools (MCP-compatible), shell, files, and approvals, with full audit and cost telemetry, owned end-to-end by the user.

**v1.0 is intentionally narrow** (see §3.1 Committed Scope). The CLI agent is v0.1; the forked GUI is v0.3; the remote daemon + Web Controller is v0.4; v1.0 is polish + ship. Each version is independently shippable.

The product brand is **omw** with no Warp wordmark or icon. The repo/community uses the codename `oh-my-warp` as an `oh-my-zsh`-style homage to acknowledge the fork lineage; this codename does not appear on the product surface.

---

## 1. Problem & Motivation

Warp is one of the best agentic terminals available, and as of April 2026 the **client is open source** under AGPL. But:

- **Hosted lock-in.** The agent harness, identity, Drive, and orchestration layer are not in the open-source repo. Using Warp's agent at scale requires a paid plan and Warp's cloud.
- **BYOK is gated.** Users with existing OpenAI/Anthropic/Google contracts (often paid by their employer) still pay Warp on top to use those keys.
- **Cloud-bound features.** Drive sync, team workspaces, hosted Oz cloud agents are unavailable to users who can't or won't send code/secrets to a third party (regulated industries, on-prem teams, privacy-focused devs, hobbyists).
- **No remote control surface.** There is no first-party way to attach to a Warp session from a phone or another laptop. Mobile use is essentially impossible.
- **No agent reuse.** The agent's tool stack and approval flow is hidden behind the hosted service — users can't extend it, audit it, or run it offline.

**omw** assumes the open client is a great UI substrate, and replaces the cloud half with things the user owns.

### Why now

- Warp's client went open-source (AGPL) — the substrate is finally available.
- BYOK demand is at an all-time high; every dev has at least one paid LLM key.
- Tailscale is mainstream; tailnet-as-VPN is a default for serious developers.
- MCP and ACP are emerging as standard agent protocols, lowering the cost of building a replacement agent backend.
- Local LLMs (Ollama, LM Studio) are good enough for many coding tasks, making true offline mode viable.

---

## 2. Vision & Product Principles

> **A terminal where the user owns the model, the network, the remote, and the audit trail.**

Seven principles, in priority order. When in conflict, earlier principles win.

1. **Owner-controlled.** No omw-controlled hosted service. No phone-home telemetry from omw. The user's machine is the source of truth.
2. **Local-first, opt-in to cloud.** All omw services run locally. *Provider* (LLM API) and *transit* (Tailscale) calls are explicit, visible, and opt-in — but they exist, by design. omw never *introduces* cloud dependencies the user didn't choose.
3. **Tailnet, not internet.** Remote control surfaces live on the user's tailnet by default. Public exposure is an explicit, advanced action.
4. **Visible cost.** Every token, every API call, every dollar is shown to the user in real time. BYOK without cost UX is a regression.
5. **Auditable.** Every shell command, every agent tool call, every file write is logged locally to an append-only journal that the user can read, search, export.
6. **Approve, don't surprise.** Destructive or remote actions require explicit consent. Defaults err toward read-only and ask-first.
7. **Don't fake official entitlements.** No bypassing Warp's paid features by impersonating their cloud. We replace; we do not pretend.

---

## 3. Goals & Non-Goals

### 3.1 v1.0 Committed Scope (bright line)

What MUST work for v1.0 to ship. If it's not on this list, it's **Beyond v1** (§13.x).

- `omw-agent` CLI: BYOK + tools + MCP + approval policy + audit + cost telemetry — see §6 FR-1..FR-6.
- Forked Warp client running in *local mode*, calling `omw-agent` via `omw-server`. Zero Warp cloud calls (verified via packet capture).
- `omw-remote` daemon serving the **Web Controller** to a single paired host over Tailscale Serve.
- Per-host pairing flow with revocation, signed requests, and replay protection (per `specs/byorc-protocol.md`).
- **Tier-1 providers only**: OpenAI, Anthropic, OpenAI-compatible, Ollama. (OpenAI-compatible covers Azure OpenAI, Bedrock-via-LiteLLM, OpenRouter, vLLM, Together, Fireworks.)
- Real-time cost surface in CLI and GUI.
- Append-only audit log with hash-chain integrity.
- Homebrew install on macOS.

### 3.2 Non-Goals (v1.0)

- **Bypassing Warp paid features.** No proxying Warp's cloud, no fake entitlements, no scraping their hosted agent.
- **omw hosted service.** No omw cloud, no SaaS tier, no centralized backend run by us.
- **Public-internet exposure by default.** No ngrok-style tunnels in the default install path.
- **Reimplementing Warp Drive.** Block sharing/sync is out of scope; we don't have a server.
- **Native mobile/desktop apps.** Web Controller first; native shim is Beyond v1.
- **Drop-in compatibility with hosted Warp APIs.** We replace, we don't mirror.
- **Multi-machine fanout.** Each host pairs separately in v1; auto-discovery and fleet UX are Beyond v1.
- **Plugin marketplace.** A pluggable agent + provider system, yes; a marketplace, no.
- **Tier-2 providers.** Google Gemini and LM Studio are deferred (Beyond v1).

### 3.3 Anti-goals (will refuse)

- Anything that masquerades as Warp's official infrastructure.
- Anything that lets a remote attacker reach a developer machine without explicit pairing.
- Anything that stores API keys in plaintext config files.

---

## 4. Target Users

### Primary persona — "Indie / staff-level developer with strong opinions"

- Has paid OpenAI + Anthropic accounts, wants one terminal to use both.
- Runs Tailscale on every device they own.
- Frustrated that remote development tooling barely exists in 2026.
- Privacy- and cost-conscious; reads pricing docs.
- Comfortable with `cargo`, `brew`, dotfiles.

### Secondary persona — "On-prem / regulated team lead"

- Cannot send code to third-party hosted agents.
- Has internal LLM endpoints (Bedrock, Vertex, self-hosted vLLM) that are OpenAI-compatible.
- Needs an audit trail their security team can review.
- Will pay for support, not for hosted compute.

### Tertiary persona — "Homelab / oncall engineer"

- Wants to attach to their home box from a phone during a commute or a 3am page.
- Already runs Tailscale; doesn't want to expose anything publicly.
- Wants approval workflow when an agent suggests destructive ops.

### Out of scope for v1

- Casual terminal users with no LLM workflow.
- Enterprises requiring SSO/SAML/centralized policy.
- Teams that need shared sessions across multiple humans.

---

## 5. Product Pillars

### 5.1 BYOK — Bring Your Own Key

**Problem.** Users with their own LLM keys are forced into Warp's billing layer. There's no way to point Warp at Ollama or a self-hosted endpoint.

**Solution.** A pluggable provider system in `omw-agent`. Keys live in the OS keychain; provider config in `~/.config/omw/config.toml`.

**Tier-1 providers (v1.0)**

- OpenAI
- Anthropic
- OpenAI-compatible (covers Azure OpenAI, Bedrock-via-LiteLLM, OpenRouter, vLLM, Together, Fireworks)
- Ollama (local)

**Tier-2 providers (Beyond v1)**

- Google (Gemini) — needs separate SDK; defer.
- LM Studio — already covered by OpenAI-compatible; first-class UX is polish.

**v1.0 features**

- Per-provider config, model lists, capability hints (vision, tool use, streaming).
- Real-time cost display: tokens in/out, dollar cost per response, running per-session and per-day totals.
- Provider health: latency p50/p95, recent error rate, per-provider.

**Beyond v1**

- Per-task routing rules ("Anthropic for code review, Ollama for chat") — config schema only in v1; runtime routing in v1.x.
- Privacy Mode toggle (hard-block of cloud providers).

**Key storage.** OS keychain only. No plaintext fallback. Config files reference keychain entries by name (`keychain:omw/openai`).

**Tradeoff.** We will NOT support BYOK against Warp's hosted cloud agent. We replace that agent with `omw-agent`.

### 5.2 BYORC — Bring Your Own Remote Controller

**Problem.** No first-party way to control a Warp session from another device. Cloud-broker'd remote desktop solutions are slow, public, or both.

**Solution.** A local daemon (`omw-remote`) exposes HTTP + WebSocket APIs on `127.0.0.1` and serves the Web Controller bundle. Tailscale Serve forwards a single port to the user's tailnet. Any device on the tailnet — laptop, phone, tablet — connects via the tailnet hostname using the official Web Controller (or any client speaking the protocol). Never the public internet.

**v1.0 features**

- Terminal sessions anchored to the **omw GUI process** (`~/oh-my-warp/warp`). The GUI holds the PTY and shell executor — the same per-session executor model the warp codebase uses. `omw-remote` is a protocol bridge: it subscribes to session events from `omw-server` and pipes them to the remote client over authenticated WebSocket. Sessions survive Web Controller disconnects and `omw-remote` restarts; they require the GUI to be running (headless survival via tmux is Beyond v1 — see §13.x).
- Remote UI delivered in two ways: the **Web Controller** (PWA at `https://hostname.tailnet.ts.net`, served by `omw-remote`) for any browser-capable device, and the **omw GUI itself** as the local container. Both surfaces speak the same `omw-remote` WebSocket protocol.
- Agent sessions exposed over WebSocket with streaming output and approval prompts.
- **Single-host pairing.** Each host pairs independently; the Web Controller's URL (`https://hostname.tailnet.ts.net`) identifies the host. A device pairs separately with each host it wants to control.
- Pairing: QR-based, one-time token, per-device Ed25519 keypair, signed requests, replay window — see `specs/byorc-protocol.md`.
- Revocation: any device can be revoked from the host machine in one command.
- Audit: every keystroke batch, command, agent tool call, file write, approval is logged.
- Local kill switch: `omw remote stop --all`.

**Defaults**

- Listens on `127.0.0.1:8787` only. Tailscale Serve must be explicitly started.
- New paired devices default to `read_only`.

**Beyond v1**

- Multi-machine fleet UX (single device sees all paired hosts in one picker).
- Auto-discovery via Tailscale tags or mDNS.
- `omw-tsnet-gateway` (Go) for embedded Tailscale.
- Optional Cloudflare Tunnel path for non-Tailscale users.

**Tradeoff.** We require Tailscale to be installed on host and client for v1. We will NOT bundle Tailscale or embed `tsnet` in v1.

### 5.3 Local-first agent platform

**Problem.** The hosted Warp agent is a black box — users can't extend it, swap models, or audit what it's doing.

**Solution.** **pi-agent** (`vendor/pi-mono`, TypeScript) adopted as the omw agent kernel. omw's primary contribution to the agent layer is a **`WarpSessionBashOperations` adapter** that replaces pi-agent's default isolated subprocess executor (`createLocalBashOperations`) with one that writes commands into the active Warp terminal session's PTY via `omw-server`'s internal session API — so the agent's shell commands execute inside the user's open terminal pane, not a hidden subprocess.

- **Providers** — pi-agent's `packages/ai` layer covers all Tier-1 providers (OpenAI, Anthropic, OpenAI-compatible, Ollama) and 20+ others out of the box. Keys are resolved from `omw-keychain` via the `getApiKey` hook in `AgentLoopConfig`.
- **Built-in tools** — pi-agent ships bash, read, write, edit, grep, find, ls. The bash tool is the critical extension point: `createBashTool(cwd, { operations: warpSessionBashOps })` replaces the spawn path with PTY writes into the active session.
- **MCP client** — pi-agent's `getTools` extension hook adds MCP-backed tools at agent startup. Warp's own `rmcp`-based MCP client (`vendor/warp`) informs the implementation.
- **Approval policy** — pi-agent's `beforeToolCall` / `afterToolCall` hooks implement the `read_only` / `ask_before_write` / `trusted` modes; `omw-policy` becomes a thin configuration layer over these hooks rather than a standalone engine.
- **Persistent transcripts** — pi-agent's SQLite session storage, path-adapted to `~/.local/share/omw/`.
- **ACP server mode** — an ACP wrapper is added around pi-agent's `agentLoop` so other editors can use it as their backend (ACP is not yet in pi-agent upstream; omw adds it).

**v1.0 features**

- Streaming responses with tool-call surface.
- Approvals: any tool call that writes/executes/networks must be approved unless on the allow list.
- Cost reporting: every response carries token + dollar metadata; reconciled against API-reported usage when available.
- Transcript search across sessions.

**Beyond v1**

- Workspace indexing for code search context.
- Routing rules executed at runtime.

**Tradeoff.** v1 runs pi-agent as a Node.js process; `omw-server` spawns and communicates with it over stdio or loopback HTTP. The forked Warp UI calls into `omw-server`, which delegates to pi-agent. Adopting pi-agent removes ~5,000 lines of agent loop, provider, and tool code from omw's scope; the cost is a TypeScript runtime dependency alongside the Rust binaries. The `WarpSessionBashOperations` adapter is omw's sole required fork of pi-agent internals.

---

## 6. Functional Requirements (FR)

### FR-1 Installation & Onboarding

- FR-1.1 One-line install: `brew install omw` (Linux/Windows packaging is Beyond v1).
- FR-1.2 Post-install wizard (CLI): provider setup, optional remote enable, optional pairing.
- FR-1.3 First-run check: detects existing keys in keychain (e.g. from `gh`, `op`, env), offers to import.
- FR-1.4 Sane defaults: GUI opens to a local profile with a default provider configured to Ollama if detected, else prompts.

### FR-2 Provider Management

- FR-2.1 `omw provider add <name>` — interactive prompt for keys, stored in keychain.
- FR-2.2 `omw provider list` — show configured, default, health.
- FR-2.3 `omw provider test <name>` — round-trip to verify.
- FR-2.4 UI parity: provider settings page in the forked client.
- FR-2.5 Default-model selection per provider.

### FR-3 Agent Sessions

- FR-3.1 `omw ask "<prompt>"` — one-shot, streams to stdout.
- FR-3.2 `omw agent --cwd <path>` — interactive REPL.
- FR-3.3 `omw agent --provider <id> --model <id>` — override per session.
- FR-3.4 In-UI agent panel in the forked client uses the same backend (`omw-agent` via `omw-server`).
- FR-3.5 Tool calls visible in transcript with input, output, and approval state.
- FR-3.6 Approvals are blocking; the agent waits until the user accepts/rejects/skips.

### FR-4 Remote Control

- FR-4.1 `omw remote start [--listen 127.0.0.1:8787]` — boots HTTP + WS daemon.
- FR-4.2 `omw remote status` — shows listening port, connected devices, active sessions.
- FR-4.3 `omw remote stop [--all]` — shuts down daemon and revokes ephemeral pairings.
- FR-4.4 Sessions survive Web Controller disconnects and `omw-remote` restarts (GUI is the anchor). Sessions do not survive GUI process exit in v1; the tmux headless path is Beyond v1.
- FR-4.5 The daemon reads/writes audit logs at `~/.local/share/omw/audit/`.

### FR-5 Pairing

- FR-5.1 `omw pair qr` — prints a QR + URL with a one-time token (TTL 10 min).
- FR-5.2 `omw pair list` — show paired devices, last-seen, permissions.
- FR-5.3 `omw pair revoke <id>` — immediate revocation; active sessions are dropped within 1s.
- FR-5.4 New device default permissions: `read_only`. User must explicitly upgrade.
- FR-5.5 Pairing requires the host to be online; no out-of-band pairing.

### FR-6 Audit

- FR-6.1 `omw audit tail` — live tail of the audit log.
- FR-6.2 `omw audit search <query>` — full-text + structured search.
- FR-6.3 `omw audit export --since <date> [--format jsonl|md]` — exportable.
- FR-6.4 UI: "Activity" view in the forked client with filters by device, session, action type.
- FR-6.5 Append-only on disk with hash-chain integrity (per-day file with rolling SHA chain).

### FR-7 Web Controller (single host, v1.0)

The official BYORC client. A web app served by `omw-remote` over the tailnet, also installable as a PWA. Runs on any modern browser — phone, tablet, laptop, second monitor. Mobile is one form factor, not a separate product.

- FR-7.1 Pairing: scan or paste the QR/URL from the host's `omw pair qr` output (camera scan on mobile, paste-token fallback on desktop).
- FR-7.2 Terminal view (xterm.js): attach, send, resize, scrollback, copy.
- FR-7.3 Agent view: send prompt, watch streaming response, see tool calls, approve/reject.
- FR-7.4 Approvals tray: queue of pending approvals across all sessions on this host.
- FR-7.5 Diff view: inline diffs with accept/reject.
- FR-7.6 Settings: providers (read-only in v1), permissions, device info.
- FR-7.7 Read-only mode toggle (per session).
- FR-7.8 Responsive layout: phone-first dense layout, expands to multi-pane on wider screens.
- FR-7.9 PWA install + service worker so a paired device can re-attach to a session without round-tripping a fresh page load.

**Single-host scope.** v1 has no fleet/picker view across multiple hosts. Each host's Web Controller is a distinct PWA install at a distinct URL. Multi-host UX is Beyond v1.

**Known v1 limitation.** iOS PWA push notifications remain restricted in 2026; the oncall "wake me up at 3am" flow (Journey B) requires the user to already have the page open or use a separate pager (PagerDuty etc.). A native shim closing this gap is Beyond v1.

### FR-8 Costs & Telemetry

- FR-8.1 Every response shows tokens in/out + dollar estimate.
- FR-8.2 Per-session and per-day rollups in UI.
- FR-8.3 `omw costs --since <date>` CLI rollup.
- FR-8.4 Reconciliation: estimated tokens (tokenizer-side) vs reported tokens (API-side); both stored, deltas surfaced.
- FR-8.5 No outbound telemetry to any omw-controlled service. Period.
- FR-8.6 Optional opt-in local-only metrics for personal dashboards.

---

## 7. User Journeys

### Journey A — "First-run BYOK" *(v1.0 critical path)*

> *Sarah just installed omw. She has paid OpenAI and Anthropic accounts and a homelab running Ollama.*

1. `brew install omw`
2. `omw setup` — CLI wizard.
3. Wizard asks: which providers? She picks OpenAI, Anthropic, Ollama. Pastes keys (stored in Keychain). Wizard pings each, all green.
4. Wizard asks default model. She picks Claude Sonnet 4.6.
5. Wizard offers to launch the GUI: `omw`. The forked client opens; the agent panel works immediately. No login screen, no payment.
6. She asks the agent to summarize a panic in her clipboard. Streams in 1.2s. Cost shown: $0.003.

**Success criteria:** zero browser-based auth, zero payment prompts, working agent in <2 minutes.

### Journey B — "BYORC: 3am page from a hotel" *(v1.0 single-host)*

> *Mark gets paged by PagerDuty on his phone at 3am. His prod tail is on his home Mac.*

1. Phone has the home-mac Web Controller bookmarked. Tailscale on.
2. He opens the Web Controller for `home-mac`. Already paired.
3. He taps the `incident-23-watch` tmux session. Terminal attaches in 200ms.
4. Logs are scrolling. He switches to agent view: "what changed in the last hour?"
5. Agent runs `git log --since='1h ago'` — pending approval. He taps approve. Output streams.
6. Agent suggests a rollback: `git revert abc123 && deploy`. Approval required because it's a write. He reads the diff, approves the revert, holds the deploy.
7. Audit log captures the whole sequence with his phone's device id.

**Success criteria:** from page to first command in <60 seconds, no unauthorized actions, full trail.

> **v1 caveat.** "Got paged on his phone" assumes a separate pager (PagerDuty etc.) waking him. Reliable in-app push is Beyond v1.

### Journey C — "Privacy mode for an offline flight" *(Beyond v1)*

> *Priya is on a flight. Privacy Mode toggle hard-blocks cloud providers; default model auto-switches to Ollama.*

Listed for vision; the toggle ships in v1.x once routing rules are runtime-evaluated.

### Journey D — "Multi-machine fleet" *(Beyond v1)*

> *Devon has a laptop, a desktop, and a build server, all on his tailnet.*

Listed for vision. v1 supports the use case via separate Web Controller installs per host; the unified picker is Beyond v1.

---

## 8. Technical Architecture

### 8.1 High-level diagram

```
┌──────────────────────────────────────────────────────────────────┐
│                         omw client (forked)                      │
│  Rust + WarpUI                                                   │
│  Local mode: BackendMode::OmwLocal                               │
└──────────────────────────────┬───────────────────────────────────┘
                               │ HTTP + GraphQL + WS (loopback)
                               ▼
┌──────────────────────────────────────────────────────────────────┐
│                          omw-server                              │
│  Rust (axum). Runs on 127.0.0.1.                                 │
│  Local identity, settings, single audit-log writer.              │
│  Stateless proxy for agent + remote operations.                  │
└──────┬─────────────────────────────────────┬─────────────────────┘
       │                                     │
       ▼                                     ▼
┌──────────────────┐                ┌────────────────────────────┐
│   omw-agent      │                │        omw-remote          │
│   TypeScript     │                │   Rust                     │
│   pi-agent kernel│                │   GUI-anchored PTY bridge  │
│   +WarpSession   │                │   PTY sessions             │
│   BashOps adapter│                │   Pairing, signed reqs     │
│   ACP wrapper    │                │   Web Controller bundle    │
└──────┬───────────┘                └──────────────┬─────────────┘
       │                                           │
       │ provider HTTPS                            │ Tailscale Serve
       ▼                                           ▼
┌──────────────────┐                ┌────────────────────────────┐
│  LLM providers   │                │      User's tailnet        │
│  Ollama / cloud  │                │   Web Controller (any dev) │
└──────────────────┘                └────────────────────────────┘
```

### 8.2 Components

| Component | Lang | Role | v1 |
|-----------|------|------|----|
| `omw` (fork) | Rust | The terminal UI, agent panel, settings | ✓ |
| `omw-server` | Rust (axum) | Local backend shim for the fork; sole audit writer | ✓ |
| `omw-agent` | TypeScript (pi-agent) | pi-agent kernel (`vendor/pi-mono`) + `WarpSessionBashOperations` adapter; providers, tools, approval hooks, SQLite transcripts, ACP server wrapper | ✓ |
| `omw-remote` | Rust | GUI-anchored PTY bridge + remote API + pairing | ✓ |
| `omw-cli` | Rust | `omw` umbrella CLI wrapping subcommands | ✓ |
| `omw-config` | Rust | Config loading, schema, validation | ✓ |
| `omw-policy` | Rust | Approval/permission engine (library, no state) | ✓ |
| `omw-keychain` | Rust | OS keychain wrapper (macOS first; Linux/Windows Beyond v1) | ✓ |
| `omw-pty` | Rust | PTY/portable-pty wrapper (used by remote) | ✓ |
| `omw-acp` | Rust | ACP protocol bindings | ✓ |
| `omw-audit` | Rust | Audit schema, hash chain, redaction (library) | ✓ |
| `omw-web-controller` | TS+React | Web controller (PWA-installable) | ✓ |
| `omw-tsnet-gateway` | Go | Optional `tsnet`-based gateway | Beyond v1 |
| `omw-native-shim` | Tauri or Flutter (TBD) | Push-notification + background wrapper | Beyond v1 |

### 8.3 Component Ownership Map

Single source of truth for each concern. Cross-component access goes through the listed owner's API; nobody else writes to the underlying storage.

| Concern | Sole owner | Notes |
|---------|------------|-------|
| Agent session lifecycle (create, run, end) | `omw-agent` (pi-agent `AgentSession`) | Sessions, transcripts, tool-call rows — pi-agent's SQLite storage, path-adapted to omw data dir. |
| Provider API calls (HTTP, streaming, tokenization) | `omw-agent` (pi-agent `packages/ai`) | One client per provider in pi-agent's provider layer; retry/backoff isolated per provider. |
| MCP server lifecycle | `omw-agent` (pi-agent `getTools` hook) | Spawn MCP servers, JSON-RPC, tool registry — via pi-agent extension hook + warp's `rmcp` client. |
| Approval policy *logic* | `omw-agent` (pi-agent `beforeToolCall` hook) | `omw-policy` provides the mode/allowlist config; pi-agent's hook executes the decision. |
| Approval *decisions* (recorded) | `omw-agent` | `afterToolCall` hook writes to `approvals` table and emits audit entry via `omw-server` API. |
| Cost estimation | `omw-agent` (pi-agent usage events) | pi-agent emits token usage per turn; omw records estimate row in `usage_records`. |
| Cost reconciliation | `omw-agent` | API-reported usage joined to estimate; delta logged. |
| PTY sessions | `omw-remote` (subscriber) + `omw GUI` (anchor) | GUI holds PTY; omw-remote subscribes via omw-server internal session API and bridges to Web Controller. |
| Device pairing + signature validation | `omw-remote` | Ed25519 keys, nonce store, replay window. Spec in `specs/byorc-protocol.md`. |
| Web Controller bundle serving | `omw-remote` | Bundle ships with the binary; no external CDN. |
| **Audit log writes** | `omw-server` (single SQLite writer) | All other processes append via local API. SQLite serializes. |
| Audit log reads | Anyone | Read-only access to JSONL + SQLite index. |
| Keychain access | `omw-keychain` (library) | Process-local; no IPC. |
| Configuration parsing | `omw-config` (library) | Validates and watches. |

The forked client (`omw` GUI) is a *consumer*: it talks to `omw-server` and renders state. It owns no persistent state of its own.

### 8.4 Implementation route

**Route A first, Route B trigger:**

- **Route A** (v0.1 → v1.0): build `omw-server` as a local-mode shim; the forked client talks to it via the existing `with_local_server` Warp feature flag. Replace cloud paths only as needed.
- **Route B trigger:** when *any* of the following hit, fork the cloud paths cleanly:
  - We accumulate a 3rd compat bug from upstream schema changes.
  - We hit 5,000 weekly active installs.
  - Warp upstream removes/breaks the local-server feature.

This keeps us shipping fast without committing to indefinite shimming.

### 8.5 Fork strategy & upstream tracking

- Fork lives in a sibling repo (`oh-my-warp/warp-fork`) under AGPL.
- Branches:
  - `upstream/master` — mirror of `warpdotdev/master` (read-only).
  - `omw/main` — our integration branch.
  - `omw/local-mode` — patch series for local backend.
  - `omw/branding` — patch series for omw branding.
- CI: nightly job rebases `omw/main` onto `upstream/master`; opens a tracking issue on conflict.
- Public *patch series* (`git format-patch`) so the diff against upstream is auditable and reviewable.
- Full strategy in `specs/fork-strategy.md` (Phase 0 deliverable).

### 8.6 Repo layout

```
oh-my-warp/                  # repo / codename
  README.md
  PRD.md
  TODO.md
  LICENSE                    # MIT — covers original omw-* crates
  LICENSE-AGPL               # AGPL notice for combined distribution
  CLAUDE.md
  crates/
    omw-cli/
    omw-server/
    omw-remote/
    omw-config/
    omw-policy/              # approval mode + allowlist config; wired into pi-agent beforeToolCall hook
    omw-keychain/
    omw-pty/
    omw-acp/
    omw-audit/
  apps/
    web-controller/          # PWA-installable web app — official BYORC client
    omw-agent/               # pi-agent fork: WarpSessionBashOperations adapter + omw ACP wrapper (TypeScript)
  vendor/
    warp/                    # upstream Warp fork (submodule → oh-my-warp/warp-fork)
    pi-mono/                 # pi-agent monorepo (submodule → badlogic/pi-mono)
  specs/
    byok.md
    byorc-protocol.md        # auth, signing, replay, capability scopes (Phase 0)
    fork-strategy.md         # branching, patch series, upstream tracking (Phase 0)
    threat-model.md          # actors, surfaces, invariants (Phase 0)
    test-plan.md             # trust-tiered test strategy (Phase 0)
    audit.md
  packaging/
    homebrew/
  .github/
    workflows/
      upstream-rebase.yml    # nightly fork tracking
```

The Warp fork itself is a sibling repo (`oh-my-warp/warp-fork`), referenced via a submodule under `vendor/warp-fork/`.

---

## 9. APIs & Protocols

### 9.1 `omw-server` (loopback only)

Surface the *minimum* of Warp's local-server contract to boot the client + native APIs for everything else.

- `GET /api/v1/identity` — returns local profile (no auth).
- `GET /api/v1/providers` — configured providers.
- `POST /api/v1/agent/sessions` — start an agent session.
- `WS  /ws/v1/agent/:session_id` — streaming agent events.
- `GET /api/v1/settings` / `PUT /api/v1/settings`.
- `POST /api/v1/audit/append` — single writer endpoint for other in-process components.
- `GraphQL /graphql/v2` — minimal compatibility surface for Warp's queries (introspect from the fork, implement only what the local-mode client uses, error on the rest).

### 9.2 `omw-remote` (tailnet-exposed via Tailscale Serve)

The HTTP/WS surface listed below is a **rough sketch**. The authoritative protocol — including request signing, capability scopes, replay window, WS frame auth, origin pinning, and pairing handshake — lives in **`specs/byorc-protocol.md`** and must be reviewed before v0.4 implementation begins (§13).

```
GET    /api/v1/status
GET    /api/v1/devices
GET    /api/v1/sessions
POST   /api/v1/sessions
POST   /api/v1/sessions/:id/input
POST   /api/v1/sessions/:id/resize
POST   /api/v1/sessions/:id/kill

GET    /api/v1/agent/tasks
POST   /api/v1/agent/tasks
POST   /api/v1/agent/tasks/:id/approve
POST   /api/v1/agent/tasks/:id/reject

POST   /api/v1/pair/redeem
GET    /api/v1/audit
```

WebSockets:

```
/ws/v1/pty/:session_id
/ws/v1/agent/:task_id
/ws/v1/events
```

### 9.3 Agent protocols

- **MCP (Model Context Protocol)** — `omw-agent` is an MCP *client*. Users add MCP servers in config; the agent loads them at startup and exposes their tools to the LLM. v1 must support stdio and HTTP MCP transports.
- **ACP (Agent Client Protocol)** — `omw-agent` runs as an ACP *server* (`omw acp-agent`) so other editors can use it.

This split — MCP for tools, ACP for editor integration — is intentional and matches the 2026 ecosystem.

---

## 10. Data Model

All persistent state is in `~/.local/share/omw/` (Linux/macOS XDG path; Windows equivalent), backed by SQLite + flat files.

```
~/.local/share/omw/
  db.sqlite               # all structured state
  audit/
    2026-04-29.jsonl      # append-only daily files, hash-chained
    ...
  transcripts/
    <session_id>.json     # full agent transcripts
```

Tables (SQLite):

- `providers(id, kind, config_ref, created_at)`
  — `config_ref` references a keychain entry; no plaintext secrets.

- `provider_pricing(id, provider_id, model, version, effective_at, in_cents_per_mtok, out_cents_per_mtok)`
  — Snapshots of provider prices. Old transcripts cost reproducibly even after pricing changes.

- `agent_sessions(id, started_at, ended_at, provider_id, model, pricing_version_id)`

- `messages(id, session_id, role, content, created_at)`

- `tool_calls(id, message_id, tool, args_json, result_json, started_at, ended_at)`

- `usage_records(id, message_id, source, tokens_in, tokens_out, cost_cents, recorded_at)`
  — `source` ∈ {`estimate`, `reported`}. Both rows exist for the same message; reconciliation = join + delta. Lets the cost UI show "estimated $0.04, actual $0.038."

- `approvals(id, tool_call_id, decision, decided_by_device_id, decided_at, signature, reason)`
  — Separate from `tool_calls` so an approval is a first-class auditable event with its own provenance.

- `devices(id, name, public_key, paired_at, last_seen, permissions_json, revoked_at)`

- `pairings(id, token_hash, expires_at, used_at, used_by_device_id)`
  — `token_hash` not the raw token (so a DB read can't replay an unused token).

- `pty_sessions(id, name, gui_session_id, created_at, last_active)`

- `request_log(id, route, actor_device_id, nonce, ts, signature, body_hash, accepted, reason)`
  — Replay-defense audit. Used to detect stuck nonces and rejected requests.

- `audit_chain(file, line_no, prev_sha, this_sha, ts, kind, actor_device_id, target_ref)`
  — Pointer + hash chain across audit JSONL files. Tamper-evident.

- `redaction_rules(id, pattern, scope, action, created_at)`
  — What gets stripped before audit logging (API keys, .env values, user-defined patterns). Default ruleset ships in v1.

Configuration in `~/.config/omw/config.toml`. Secrets exclusively in OS keychain.

---

## 11. Security & Privacy

### 11.1 Threat model

We assume:

- The host machine is trusted (we are not building anti-malware).
- The tailnet is auth'd at the network layer (Tailscale handles transport crypto), but a malicious app *on* the same tailnet may still attempt to connect to `omw-remote`. App-layer auth is required.
- Paired devices may be lost/stolen — revocation must be fast and complete.
- LLM providers may log requests — Privacy Mode (Beyond v1) handles policy choice.
- AGPL fork distributors may copy/clone the binary — no embedded secrets, no per-instance keys baked in.

Full actor/surface mapping in `specs/threat-model.md` (Phase 0 deliverable).

### 11.2 Invariants (must hold for all builds)

- **No plaintext keys on disk.** OS keychain only.
- **No public-internet exposure by default.** `omw-remote` listens on loopback; tailnet exposure is opt-in via `tailscale serve`.
- **No unauthenticated remote requests.** Every HTTP request and WS handshake is signed; tailnet trust alone is insufficient.
- **Replay protection.** Nonce + 30s window; replays rejected and logged.
- **No silent destructive actions.** Default approval mode is `ask_before_write`. Trusted mode requires explicit per-device upgrade.
- **No telemetry to omw.** Period.
- **Hash-chained audit.** Audit JSONL chain rolls SHA over every line.
- **Revocation propagates within 1s.** Active WS connections drop; new requests rejected.
- **Per-device permission scoping.** A token authorized for read-only PTY cannot call agent endpoints.

### 11.3 BYORC protocol (deferred to spec)

The full request-signing scheme, capability tokens, replay window, WS frame authentication, origin/CORS posture, and pairing handshake live in **`specs/byorc-protocol.md`** (Phase 0 deliverable; **required before v0.4 implementation begins**).

PRD-level summary: per-device Ed25519 keypairs; every HTTP request and WS handshake is signed; per-frame token auth on WS; capability tokens scope per-route access; nonce + 30s replay window; origin pinning at handshake; pairing tokens are single-use and stored hashed.

### 11.4 Out of scope for v1

- Multi-user / multi-tenant on a single machine.
- Federated identity (SSO/SAML).
- Hardware-key-backed approvals (YubiKey).
- Encrypted-at-rest audit log (we rely on disk-level encryption).

### 11.5 External review (tiered)

- **Protocol/design review** before v0.4 implementation lands. Catches design flaws before code is in users' hands.
- **Implementation review** before v1.0 ship. Catches code-level bugs.

For an indie project, "review" means a paid-but-scoped engagement (e.g., one-week retainer with a security-aware contractor) rather than a full pentest. Budget honestly; downgrade language to "review" until funded for an "audit."

---

## 12. Legal & Licensing

> Not legal advice. Treat this section as an engineering plan; have a lawyer review before public distribution.

### 12.1 Brand vs codename

- **Product brand:** **omw** (lowercase). Used on the binary, GUI wordmark, website, packaging, social. No reference to "Warp" anywhere on the product surface.
- **Repo / community codename:** **oh-my-warp**. Used as the GitHub org/repo name only, as an `oh-my-zsh`-style homage acknowledging the fork lineage. Does not appear on the product surface.

This split keeps the homage in the open-source/community context (where it's customary and defensible) while keeping the trademarked term out of the brand.

### 12.2 Licensing

- **Warp upstream is AGPL-3.0.** Any fork we distribute carries AGPL obligations: source must be available; users running our distribution can request source.
- **This repo's `LICENSE` is MIT.** Covers original `omw-*` crates only. The actual fork lives in a sibling repo (`oh-my-warp/warp-fork`) under AGPL with full upstream attribution.
- **Combined distribution.** When we ship a binary that includes both AGPL Warp code and MIT `omw-*` code, the *combined work* is effectively AGPL. The user-facing license is AGPL. The MIT'd crates can still be used independently.

### 12.3 Trademarks

- "Warp" is a trademark.
- We do **not** use Warp's logo, name, or icon in the omw product brand.
- Source/docs may reference Warp factually ("forked from the open-source Warp client").
- omw uses a distinct icon, color palette, and wordmark.

### 12.4 Distribution

- Homebrew tap under our org; no submission to Warp's official channels.
- No paid-tier circumvention: we replace cloud features rather than impersonating them. No proxy of Warp's hosted endpoints. No fake entitlement tokens.

---

## 13. Phased Roadmap

Each phase has explicit **exit criteria**. We don't move on until they're met. Calendar weeks are intentionally omitted until scope is sized post-Phase-0.

### Phase 0 — Decisions & specs (no code)

Phase 0 closes when these are written down, reviewed, and committed:

- **Brand decision.** ✓ omw (product) + oh-my-warp (codename).
- **Legal review.** AGPL/MIT boundary, trademark posture, distribution channels. (External dependency; non-blocking for code work in parallel.)
- **Threat model + invariants.** Codified in §11 + `specs/threat-model.md`.
- **Component ownership map.** Codified in §8.3.
- **Fork-rebase strategy.** `specs/fork-strategy.md` — branching, patch series, nightly upstream-tracking CI.
- **Test plan.** `specs/test-plan.md` — trust tiers, property/fuzz catalog, cassette strategy, per-phase commitments.
- **Repo skeleton.** Cargo workspace, CI scaffold (Tier A skeleton), license boundaries. No application code yet.

**Exit:** all listed specs merged; CI green; license boundaries documented; legal review at least initiated.

### v0.1 — CLI agent (Tier-1 BYOK)

Wire pi-agent (`vendor/pi-mono`) as `apps/omw-agent`. Configure its provider layer against Tier-1 providers (OpenAI, Anthropic, OpenAI-compatible, Ollama) with keys from `omw-keychain`. `omw ask` and `omw agent` via `omw-cli`. SQLite transcripts path-adapted to omw data dir. Cost reporting per response, per session, per day. `omw-config` and `omw-keychain` crates wired in.

**Exit:** I can `omw ask "summarize this"` against any Tier-1 provider, see streaming output, see cost. Standalone — no UI work.

### v0.2 — Tools, MCP, approval, audit

MCP client (stdio + HTTP). Built-in tools (shell, fs, grep, git, editor). `omw-policy` library with three approval modes + per-command allowlist. Append-only audit JSONL with hash chain. `omw audit {tail,search,export}`. `omw-audit` library finalized. `usage_records` reconciliation wired through.

**Exit:** multi-step agent task with shell + file edits, every destructive op prompts, full audit trail, hash chain verifiable.

### v0.3 — Forked client + local mode

Fork Warp into `oh-my-warp/warp-fork`; first rebase onto upstream. `omw_local` Cargo feature. Branding patches (binary rename to `omw`, icon swap, palette). `omw-server` minimal surface to boot the client. Wire fork's agent panel to `omw-server` → `omw-agent`. Provider settings UI. Cost surface in UI.

**Exit:** `omw` GUI opens, agent panel works against BYOK keys, zero outbound calls to Warp cloud (verified via packet capture).

### v0.4 — BYORC + Web Controller (single host)

**Gate:** `specs/byorc-protocol.md` written, reviewed externally, and merged before any code work in this phase.

`omw-pty` over `portable-pty`. `omw-remote` GUI-anchored session bridge. `WarpSessionBashOperations` adapter in `apps/omw-agent` — routes pi-agent bash tool calls to the Warp terminal session PTY via `omw-server` internal session API instead of isolated subprocess spawn. HTTP + WS API per the spec. Pairing flow (QR, Ed25519 keypair, signed requests, replay window). Web Controller (`apps/web-controller/`): pairing, terminal, agent, approvals, diff. Audit "Activity" view in the forked client. Single-host scope only.

**Exit:** I pair a single host, attach to a GUI terminal session over Tailscale Serve, ask agent something, approve a write, see the audit entry. Protocol review sign-off in repo.

### v1.0 — Polish & ship

First-run wizard (`omw setup`). Homebrew formula. Docs site. Screencasts of Journey A and Journey B. **External implementation security review.** Resolve all sev-1 findings.

**Exit:** v1.0 tag; Homebrew install on a fresh Mac; Journey A and Journey B (single-host) demoable end-to-end.

### 13.x — Beyond v1 (vision, not committed)

Listed for direction; not in v1.0 scope.

- Multi-machine fleet UX (auto-discovery, fleet picker, unified audit).
- Headless BYORC (tmux-backed sessions that survive GUI process exit, for server/no-display hosts).
- Tier-2 providers: Google Gemini, LM Studio.
- Privacy Mode runtime (hard-block of cloud providers).
- Per-task routing rules executed at runtime.
- Native shim for push notifications + background (Tauri or Flutter).
- `omw-tsnet-gateway` (Go) — embedded Tailscale, no external `tailscaled` install.
- Plugin/themes system.
- Optional non-Tailscale path (Cloudflare Tunnel + auth) — only if user demand justifies.
- Workspace/profile-scoped settings.
- Linux/Windows packaging.
- Hardware-key-backed approvals.
- Route B migration (clean fork of cloud paths) if any trigger hits.

---

## 14. Risks & Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Warp upstream breaks `with_local_server` | Medium | High | Nightly rebase CI + Route B trigger. Keep our patch series small. |
| Warp adds anti-fork measures (signed binaries) | Low | High | Stay current with upstream; legal review of any DRM clauses. Worst case: continue from a pinned good commit. |
| AGPL compliance failure in distribution | Medium | High | Separate fork repo with clear AGPL labeling; lawyer review pre-launch. |
| Trademark complaint from Warp | Low | High | Distinct brand (omw); codename `oh-my-warp` confined to repo only; public statement that we are an unofficial fork. |
| Tailscale dependency creates onboarding friction | High | Medium | Excellent docs; Beyond-v1 `tsnet` gateway eliminates external Tailscale install. |
| BYOK costs surprise users | High | Medium | Real-time cost UX is a v1 must. Default to lowest-cost-suitable models. Reconciliation makes overruns visible immediately. |
| iOS PWA limits (push, background) degrade oncall journey | High | Medium | Beyond-v1 native shim closes the gap. Document the limit prominently in v1; recommend pairing with PagerDuty/etc. for true oncall. |
| Fork rebase becomes unmaintainable | Medium | High | Keep patches small + targeted. Force ourselves to upstream improvements where possible. |
| Sole maintainer bus factor | High (early) | High | Public roadmap, contributor docs, RFC process from week 1. |
| MCP/ACP standards churn | Medium | Low | Both are versioned; pin major versions; isolation behind a trait. |
| External security review unaffordable for indie team | Medium | High | Tiered: peer/community review for v0.4 design; paid scoped review for v1.0 ship. Downgrade language from "audit" to "review" until funded. |
| BYORC protocol design flaw discovered post-ship | Low | Critical | Protocol is reviewed before v0.4 code. Versioned protocol field allows clean breaking changes. |

---

## 15. Open Questions

Closed:
- ~~Brand name~~ → **omw** (product) + **oh-my-warp** (codename).
- ~~Multi-machine in v1~~ → no, Beyond v1.
- ~~Tier-1 provider list~~ → OpenAI, Anthropic, OpenAI-compatible, Ollama.

Open (in priority order):

1. **Default model in the wizard** when no providers are configured. Ollama-first if detected? Force-prompt for cloud key? Decide before v0.1.
2. **GraphQL surface scope.** Implement the entire Warp local-server schema, or only the queries the local-mode client actually uses? Need to instrument the fork to find out. Decide during v0.3.
3. **MCP server distribution.** Bundle a curated set (filesystem, git, GitHub) or always BYO? Probably bundle the no-secret ones. Decide during v0.2.
4. **Cost-estimation accuracy.** Trust tokenizer-side counts only, or only post-response usage? Both, with reconciliation — schema supports it; question is what the UI shows by default.
5. **Pairing across multiple machines.** Single device identity reused per host (UX win, slightly larger blast radius if a device is compromised) vs per-host pairing (simpler, smaller blast radius). v1 = per-host. Reconsider for fleet UX in Beyond v1.
6. **Workspace/profile boundaries.** Per-project provider settings? Per-project agent permissions? Beyond v1.
7. **Public-internet exposure path.** Document Cloudflare Tunnel / ngrok as a Beyond-v1 fallback for users without Tailscale, or hard non-goal? Lean Beyond-v1 with strong warnings.
8. **Plugin/theme system.** "oh-my-warp" framing implies pluggability. v1 has no commitment; Beyond v1 with a committed RFC.
9. **License decision for the umbrella repo.** MIT for original code is fine; revisit if combined-distribution language is unclear post-launch. Lean toward keeping MIT + clear submodule split.

---

## 16. Success Metrics

We won't ship outbound telemetry, so all metrics are inferred or self-reported.

### v1.0 launch criteria

Test gates and per-phase commitments live in [`specs/test-plan.md`](./specs/test-plan.md).

- Journey A and Journey B (single-host) demonstrably work on a fresh Mac in a public screencast.
- External implementation security review completed; all sev-1 issues resolved.
- Homebrew install <60 seconds on a fresh machine.
- All Tier-1 providers (OpenAI, Anthropic, OpenAI-compatible, Ollama) pass round-trip tests.
- Audit hash chain validates end-to-end for a sample run.
- Pre-release checklist (`specs/test-plan.md` §7) signed off.

### Post-launch (90 days)

- Reach 1,000 GitHub stars (vanity but useful).
- 100 Discord/community members.
- Homebrew install count: target 500+ (visible via Homebrew analytics).
- 3 community-contributed MCP server integrations.
- One independent blog post / YouTube review per week on average.
- Zero confirmed RCE/key-leak vulnerabilities.

### Post-launch (1 year)

- Native shim (push notifications + background) shipped.
- Tier-2 providers (Google, LM Studio) shipped.
- Multi-machine fleet UX shipped.
- 10k weekly active installs (estimated).
- One enterprise pilot (paid support, no hosted service).

---

## 17. Glossary

- **ACP** — Agent Client Protocol. Editor↔agent stdio/WebSocket protocol.
- **AGPL** — GNU Affero GPL v3. Warp's upstream license.
- **BYOK** — Bring Your Own Key. User supplies LLM provider API keys.
- **BYORC** — Bring Your Own Remote Controller. User's tailnet, user's daemon.
- **MCP** — Model Context Protocol. Agent↔tools protocol from Anthropic.
- **omw** — The product brand and CLI binary. Lowercase.
- **oh-my-warp** — The GitHub repo / community codename. Homage to `oh-my-zsh`. Not used on the product surface.
- **PWA** — Progressive Web App.
- **Tailnet** — A Tailscale-defined private network across a user's devices.
- **tsnet** — Embedded Tailscale node-as-library; runs without `tailscaled`.
- **Tier-1 providers** — v1.0-committed: OpenAI, Anthropic, OpenAI-compatible, Ollama.
- **Tier-2 providers** — Beyond v1: Google Gemini, LM Studio.
- **Web Controller** — The official BYORC client. PWA-installable web app served by `omw-remote`.

---

*End of PRD v0.2.*
