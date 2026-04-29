# omw — TODO

Tracker for the phases defined in [PRD.md](./PRD.md). Mark `[x]` when done.

Brand: **omw** (product) · Repo codename: **oh-my-warp**.

---

## Phase 0 — Decisions & specs (no code)

Phase 0 is *only* about getting decisions and specs written down. No application code starts until Phase 0 closes.

- [x] Brand decision (omw + oh-my-warp codename)
- [x] Initiate legal review (AGPL/MIT boundary, trademark posture, Homebrew distribution)
- [x] Write `specs/threat-model.md` — actors, surfaces, invariants
- [x] Write `specs/byorc-protocol.md` — auth, signing, replay, capability scopes (used in v0.4)
- [x] Write `specs/fork-strategy.md` — branching, patch series, nightly upstream-tracking CI
- [x] Write `specs/test-plan.md` — trust tiers, property/fuzz catalog, cassette strategy
- [x] Component ownership map (already in PRD §8.3 — confirm with engineering leads)
- [x] Repo skeleton: Cargo workspace with empty `omw-*` crates
- [x] Add `LICENSE-AGPL` file referencing combined-distribution terms
- [x] CI: build, fmt, clippy, test
- [x] CI: nightly `upstream-rebase.yml` workflow rebasing `omw/main` onto `warpdotdev/master` (scaffold; activates when `oh-my-warp/warp-fork` is created in v0.3)

**Exit criteria:** all listed specs merged; CI green; license boundaries documented; legal review at least initiated.

---

## v0.1 — CLI agent (Tier-1 BYOK)

- [x] `omw-config` crate (TOML loading, schema validation, watcher)
- [x] `omw-keychain` crate (macOS Keychain first; Linux/Windows Beyond v1)
- [x] Wire pi-agent (`vendor/pi-mono`) as `apps/omw-agent` TypeScript package
- [x] Plumb `omw-keychain` into pi-agent's `getApiKey` hook for Tier-1 providers (OpenAI, Anthropic, OpenAI-compatible, Ollama)
- [x] Adapt pi-agent SQLite session storage path to `~/.local/share/omw/` (resolved via `omw-cli/src/db.rs::data_dir`; pi-agent kernel itself deferred to v0.2)
- [x] `omw-cli`: `omw provider {add,list,remove}` (test deferred; needs HTTP cassette infra)
- [x] `omw-cli`: `omw ask "<prompt>"` (one-shot, streams to stdout)
- [x] `omw-cli`: `omw agent --cwd .` (interactive REPL — line-buffered stdin)
- [x] `provider_pricing` snapshots wired into `usage_records` for cost reconciliation
- [x] `usage_records` (reported sources only — estimate variant deferred)
- [x] Cost reporting per response, per session, per day
- [x] `omw costs --since <date>`

**Exit criteria:** `omw ask` works against all four Tier-1 providers with streaming + reconciled cost.

---

## v0.2 — Tools, MCP, approval, audit

- [ ] MCP client in `omw-agent` (stdio + HTTP transports)
- [ ] Built-in tools: `shell`, `fs.read`, `fs.write`, `grep`, `git`, `editor`
- [ ] `omw-policy` library: three approval modes (`read_only`, `ask_before_write`, `trusted`) + per-command allowlist
- [ ] `approvals` table + per-call audit emission
- [ ] `omw-audit` library: append-only JSONL, per-day rotation, hash chain (`audit_chain` table)
- [ ] `redaction_rules` defaults (API keys, .env values)
- [ ] `omw audit {tail,search,export}`
- [ ] Routing rules block in config (parsed in v0.2; runtime evaluation Beyond v1)

**Exit criteria:** multi-step agent task with shell + file edits, every destructive op prompts, full audit trail, hash chain verifiable end-to-end.

---

## v0.3 — Forked client + local mode

- [ ] Fork Warp into `oh-my-warp/warp-fork`; first rebase against `warpdotdev/master`
- [ ] Add `omw_local` Cargo feature
- [ ] Branding patch series: rename binary to `omw`, swap icon, change palette (no "Warp" wordmark anywhere)
- [ ] `omw-server` (axum): identity, providers, agent sessions, settings endpoints
- [ ] `omw-server`: single audit-writer endpoint (`POST /api/v1/audit/append`)
- [ ] `omw-server`: minimum GraphQL surface needed to boot the client (instrument the fork to discover required queries)
- [ ] Wire fork's agent panel to `omw-server` → `omw-agent`
- [ ] Provider settings page in the GUI
- [ ] Cost surface in the GUI (per-message + session totals)

**Exit criteria:** `omw` GUI opens, agent panel works against BYOK keys, zero outbound calls to Warp cloud (verified via packet capture).

---

## v0.4 — BYORC + Web Controller (single host)

**Gate:** `specs/byorc-protocol.md` reviewed externally and merged before any code below starts.

- [ ] `omw-pty`: PTY abstraction over `portable-pty`
- [ ] `omw-server`: internal session registry API (`GET /internal/v1/sessions`, `WS /internal/v1/sessions/:id/pty`, `POST /internal/v1/sessions/:id/input`)
- [ ] `omw-remote`: GUI-anchored PTY bridge — subscribe to omw-server session events, pipe to Web Controller WS
- [ ] `WarpSessionBashOperations` adapter in `apps/omw-agent` — route pi-agent bash tool calls to Warp terminal session PTY via omw-server internal API
- [ ] HTTP API per spec: sessions, agent tasks, pairing, audit
- [ ] WebSocket streams per spec: `/ws/v1/pty/:id`, `/ws/v1/agent/:id`, `/ws/v1/events`
- [ ] Pairing flow per spec: QR, one-time hashed token, Ed25519 keypair, signed requests, replay window
- [ ] `request_log` table + nonce dedup
- [ ] Capability tokens with per-route scoping
- [ ] WS frame-level auth (not just handshake)
- [ ] Origin pinning + CORS posture
- [ ] `omw pair {qr,list,revoke}`
- [ ] `omw remote {start,status,stop}`
- [ ] Web Controller scaffold (`apps/web-controller/`, Vite + React + TS + Tailwind)
- [ ] Web Controller: pairing flow (camera QR scan + paste-token fallback)
- [ ] Web Controller: terminal view with `xterm.js`
- [ ] Web Controller: agent view with streaming + tool calls
- [ ] Web Controller: approvals tray
- [ ] Web Controller: diff view
- [ ] Web Controller: settings (read-only providers, device info, permissions)
- [ ] Web Controller: responsive layout (dense on phone, multi-pane on desktop)
- [ ] Web Controller: PWA install + service worker for fast re-attach
- [ ] Audit "Activity" view in the forked client
- [ ] **External protocol/design review sign-off in repo**

**Exit criteria:** pair a single host (phone or laptop) via QR, attach to a GUI terminal session over Tailscale Serve, ask agent something (bash tool executes in the Warp terminal pane), approve a write, see the audit entry. Protocol review sign-off in repo.

---

## v1.0 — Polish & ship

- [ ] First-run wizard (`omw setup`)
- [ ] Homebrew formula
- [ ] Docs site (mdBook or Astro)
- [ ] Screencast: Journey A (first-run BYOK)
- [ ] Screencast: Journey B (single-host BYORC oncall)
- [ ] **External implementation security review**
- [ ] Resolve all sev-1 findings
- [ ] Cut v1.0 tag

**Exit criteria:** Journey A and Journey B (single-host) demonstrable on a fresh Mac via Homebrew install.

---

## Beyond v1 — vision, not committed

Listed for direction; not in v1.0 scope. Each becomes its own RFC + planned phase post-v1.

- [ ] Multi-machine fleet UX (auto-discovery, fleet picker, unified audit)
- [ ] Headless BYORC (tmux-backed sessions that survive GUI process exit, for server/no-display hosts)
- [ ] Tier-2 providers: Google Gemini, LM Studio
- [ ] Privacy Mode runtime (hard-block of cloud providers)
- [ ] Per-task routing rules executed at runtime
- [ ] Native shim (`apps/native-shim/`) — Tauri or Flutter, push notifications + background; pick based on v1 user feedback
- [ ] `omw-tsnet-gateway` in Go (embed Tailscale; remove external dependency)
- [ ] Plugin/themes system for the fork
- [ ] Public-internet alternative path (Cloudflare Tunnel + auth) — only if user demand justifies
- [ ] Workspace/profile-scoped settings
- [ ] Linux/Windows packaging
- [ ] Hardware-key-backed approvals (YubiKey)
- [ ] Route B migration (clean fork of cloud paths) if any trigger hits

---

## Open questions to resolve before/during the relevant phase

See PRD §15. Highlights:

- [ ] (v0.1) Default model when no providers configured
- [ ] (v0.2) Bundled vs BYO MCP servers
- [ ] (v0.3) Scope of GraphQL local-server surface
- [ ] (v0.4) Single device identity per host vs reused across hosts (current default: per-host)
- [ ] (Beyond v1) Cloudflare Tunnel as documented fallback — yes or hard non-goal?
- [ ] (Beyond v1) Plugin system stake-in-the-ground for v1.x
- [ ] (Phase 0) Umbrella repo license — keep MIT or relicense AGPL
