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
- [x] Write `specs/fork-strategy.md` — tracked-snapshot model, restrip procedure, omw-edits provenance (rewritten v0.2 on 2026-05-01 to replace the original branching/patch-series design)
- [x] Write `specs/test-plan.md` — trust tiers, property/fuzz catalog, cassette strategy
- [x] Component ownership map (already in PRD §8.3 — confirm with engineering leads)
- [x] Repo skeleton: Cargo workspace with empty `omw-*` crates
- [x] Add `LICENSE-AGPL` file referencing combined-distribution terms
- [x] CI: build, fmt, clippy, test
- [x] ~~CI: nightly `upstream-rebase.yml` workflow rebasing `omw/main` onto `warpdotdev/master`~~ — retired 2026-05-01 with the move to the tracked-snapshot fork model (`specs/fork-strategy.md` v0.2). Workflow file deleted.

**Exit criteria:** all listed specs merged; CI green; license boundaries documented; legal review at least initiated.

---

## v0.1 — CLI agent (Tier-1 BYOK)

- [x] `omw-config` crate (TOML loading, schema validation, watcher)
- [x] `omw-keychain` crate (macOS Keychain first; Linux/Windows Beyond v1)
- [x] Stand up `apps/omw-agent` TypeScript package (v0.1: direct fetch streaming for the four Tier-1 providers; pi-agent kernel adoption from `vendor/pi-mono` deferred to v0.2)
- [x] Plumb `omw-keychain` into the agent's `getApiKey` hook for Tier-1 providers (OpenAI, Anthropic, OpenAI-compatible, Ollama) — `makeGetApiKey` factory is the v0.2 pi-agent contract surface, used today by tests
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

## v0.3 — Stripped client + local mode

The bulk of the v0.3 fork work landed early via the manual strip on 2026-04-29 — `vendor/warp-stripped/` is a tracked snapshot of upstream Warp with cloud, account, billing, Drive, Oz, and hosted-workflow surfaces removed and an `omw_local` Cargo feature wired in. Remaining work is narrower than originally scoped.

- [x] ~~Fork Warp into `oh-my-warp/warp-fork`~~ — superseded by tracked-snapshot model. `vendor/warp-stripped/` is the canonical Warp host (per `specs/fork-strategy.md` v0.2).
- [x] Add `omw_local` Cargo feature (already wired; binary builds as `warp-oss` with `--features omw_local`).
- [ ] Branding final pass: rename binary `warp-oss` → `omw`, swap icon, color palette, full wordmark removal sweep across remaining product surfaces (per CLAUDE.md §5).
- [ ] `omw-server` (axum) — embedded into `vendor/warp-stripped/app/` via path dep: identity, providers, agent sessions, settings endpoints.
- [ ] `omw-server`: single audit-writer endpoint (`POST /api/v1/audit/append`).
- [ ] `omw-server`: internal session registry API (`GET /internal/v1/sessions`, `WS /internal/v1/sessions/:id/pty`, `POST /internal/v1/sessions/:id/input`) — required by v0.4-cleanup; foundation may land early via v0.4-thin Phase C.
- [ ] `omw-server`: minimum GraphQL surface needed to boot the stripped client (instrument the binary to discover required queries).
- [ ] Wire stripped client's agent panel to `omw-server` → `omw-agent`.
- [ ] Provider settings page in the GUI.
- [ ] Cost surface in the GUI (per-message + session totals).

**Exit criteria:** `omw` GUI opens (rebranded), agent panel works against BYOK keys, zero outbound calls to Warp cloud (verified via packet capture).

---

## v0.4-thin — BYORC transport + Web Controller scaffold

**Gate stance.** `specs/byorc-protocol.md` is in draft and not yet externally reviewed. v0.4-thin proceeds *in parallel* with the review process, accepting reviewer-driven rework risk on conventional primitives. See PRD §13 v0.4-thin for rationale. Implementation plan: `docs/superpowers/plans/2026-05-01-v0.4-thin-byorc.md`.

Transport, pairing, and Web Controller surfaces only — agent integration, approvals, audit attribution, and the Warp-pane PTY adapter all defer to v0.4-cleanup.

- [x] `omw-pty`: PTY abstraction over `portable-pty` (21 tests; commit bc83a2b).
- [x] `omw-remote`: pairing flow per `specs/byorc-protocol.md` — QR, hashed one-time token, Ed25519 keypair, signed requests (Phase D, commit b25b72c).
- [x] `omw-remote`: nonce dedup with 30-second replay window; `request_log` table (Phase D).
- [x] `omw-remote`: capability tokens with per-route scoping (Phase D).
- [x] `omw-remote`: HTTP API surface — host-info, pair-redeem, sessions CRUD (wiring 1, commit 20e1c57).
- [x] `omw-remote`: WS framing — `/ws/v1/pty/:id` with frame-level auth, origin pinning (Phase E, commit 547c3f5; `?ct=` accepted in wiring 2, commit 6018c7e).
- [x] `omw-remote`: shell PTY direct-spawn via `omw-pty` (Phase E + wiring 1 registry).
- [x] `omw-cli`: `omw pair {qr,list,revoke}` (Phase F, commit b0c19e5).
- [x] `omw-cli`: `omw remote {start,status,stop}` (Phase F).
- [x] Web Controller scaffold (Phase G, commit e026fc4).
- [x] Web Controller: signed-request fetch wrapper using WebCrypto Ed25519 (Phase G).
- [x] Web Controller: pairing flow (Phase H, commit 0f54a50).
- [x] Web Controller: terminal view with `xterm.js`, signed WS, resize (Phase I, commit 11ce4f2).
- [x] Embed Web Controller dist into `omw-remote` via `include_dir!` (wiring 3, commit 6c44cb5).
- [x] End-to-end Rust integration test of full BYORC flow (wiring 4, commit ec9c92f).
- [x] warp-stripped Remote Control button + omw-remote launcher (wiring 5, commit 5ce0992).

**Exit criteria:** pair a host (laptop or phone) via QR over Tailscale Serve, the Web Controller opens a terminal of the host's shell, run shell commands and see output. **Components landed; four functional gaps prevent the *seamless* demo — see v0.4-thin-polish below.**

---

## v0.4-thin-polish — Close the four functional gaps

Bridges v0.4-thin's "shipped components" to "real overnight-demo-able" experience. Detailed gap analysis with fix steps + acceptance criteria: [`docs/v0.4-thin-functional-gaps.md`](./docs/v0.4-thin-functional-gaps.md). Total scope ~6-8 hours = one Claude-Code overnight session.

- [ ] **Gap 1**: `omw-server::SessionRegistry` external-source variant + `PtyController` I/O tap in warp-stripped + per-pane "Share this pane" UI. (~2-3 hours.)
- [ ] **Gap 2**: pair modal in warp-stripped with QR rendering, Copy button, paired-device count, Tailscale status line. (~1-2 hours, depends on Gap 3 + Gap 4.)
- [ ] **Gap 3**: `tokio::sync::watch` channel on `OmwRemoteState`; reactive button label/icon. (~30-60 min.)
- [ ] **Gap 4**: `omw/tailscale.rs` module — detect status, auto serve_https/unserve, multi-origin pinning. (~1.5-2 hours.)

**Exit criteria:** click Phone button in warp-oss → modal opens with QR + tailnet pair URL → phone scans QR → phone sees the *active Warp terminal pane* in real time → typing on phone echoes on laptop. No manual `tailscale serve` command, no stderr fishing, no fresh-shell confusion.

**Recommended order:** Gap 1 → Gap 4 → Gap 3 → Gap 2 (Gap 2 depends on the others' wiring).

---

## v0.4-cleanup — Agent integration + audit + approvals (post-v0.3)

Sequenced after v0.2 (policy + audit) and v0.3 (stripped GUI + omw-server) land. The Warp-pane PTY attachment migrated to v0.4-thin-polish Gap 1 (it doesn't depend on pi-agent). What remains here is the agent half of the v0.4 vision.

- [ ] `WarpSessionBashOperations` adapter in `apps/omw-agent` — route pi-agent bash tool calls to Warp terminal session PTY via `omw-server` internal API.
- [ ] HTTP API: agent tasks (`GET /api/v1/agent/tasks`, `POST /api/v1/agent/tasks`, approve/reject).
- [ ] WS streams: `/ws/v1/agent/:id`, `/ws/v1/events`.
- [ ] Audit append wiring — `omw-remote` and `omw-agent` write to `omw-server`'s single audit-writer endpoint.
- [ ] Web Controller: agent view with streaming + tool calls.
- [ ] Web Controller: approvals tray (depends on `omw-policy` from v0.2).
- [ ] Web Controller: diff view.
- [ ] Web Controller: settings page (read-only providers, device info, permissions).
- [ ] Web Controller: responsive layout (dense on phone, multi-pane on desktop).
- [ ] Web Controller: PWA service worker for fast re-attach.
- [ ] Audit "Activity" view in the stripped client (depends on `omw-audit` from v0.2).
- [ ] **External protocol/design review sign-off in repo** (or merged reviewer-driven rework if v0.4-thin proceeded in parallel).

**Exit criteria:** pair a single host via QR, attach to a GUI terminal session over Tailscale Serve, ask agent something (bash tool executes in the Warp terminal pane), approve a write, see the audit entry. Protocol review sign-off in repo.

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
