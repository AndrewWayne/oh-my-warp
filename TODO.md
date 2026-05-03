# omw — TODO

Tracker for the phases defined in [PRD.md](./PRD.md). Mark `[x]` when done.

Brand: **omw** (product) · Repo codename: **oh-my-warp**.

---

## Phase 0 — Decisions & specs (no code)

Phase 0 is *only* about getting decisions and specs written down. No application code starts until Phase 0 closes.

- [x] Brand decision (omw + oh-my-warp codename)
- [x] Initiate legal review (AGPL compliance, trademark posture, Homebrew distribution)
- [x] Write `specs/threat-model.md` — actors, surfaces, invariants
- [x] Write `specs/byorc-protocol.md` — auth, signing, replay, capability scopes (used in v0.4)
- [x] Write `specs/fork-strategy.md` — in-tree fork policy, manual upstream sync, AGPL compliance (rewritten 2026-05-01)
- [x] Write `specs/test-plan.md` — trust tiers, property/fuzz catalog, cassette strategy
- [x] Component ownership map (already in PRD §8.3 — confirm with engineering leads)
- [x] Repo skeleton: Cargo workspace with empty `omw-*` crates
- [x] Add `LICENSE-AGPL` file referencing combined-distribution terms
- [x] CI: build, fmt, clippy, test
- [x] ~~CI: nightly `upstream-rebase.yml` workflow~~ — removed 2026-05-01 with the sibling-fork plan; upstream sync is manual (see `specs/fork-strategy.md` §2)

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

- [x] Maintain Warp source in-tree at `vendor/warp-stripped/` (initial tree added 2026-04-30)
- [x] Add `omw_local` Cargo feature (initial scaffolding pre-existing; expanded 2026-05-01 to gate AI/cloud UI surfaces and exclude cloud-only crates from the binary)
- [x] **Cloud-strip cascade** — `--no-default-features --features omw_local` compiles cleanly; `audit-no-cloud.sh` reports zero hits on all six patterns. Default cloud build still passes. Plan: [`specs/cloud-strip-plan.md`](./specs/cloud-strip-plan.md). Completed 2026-05-01 in ~5 hours rather than the projected 4 days — see commit `aadae83`. The cloud crates were misclassified as needing source-level removal; in fact they are pure-types/local-utility crates with no forbidden strings.
- [x] **Mac preview release scaffolding** — umbrella-level `scripts/build-mac-dmg.sh` produces `omw-warp-oss-v<version>-aarch64-apple-darwin.dmg` from the audit-clean `omw_local` build. First tag: `omw-local-preview-v0.0.1` (2026-05-01). Naming conventions documented in [CLAUDE.md §5.1](./CLAUDE.md#51-release-naming-conventions-omw_local-previews). Does not modify `vendor/warp-stripped/`.
- [x] **Strip residual signup / Warp-brand UI for v0.0.2 preview** — Per [`docs/superpowers/specs/2026-05-01-strip-residual-signup-design.md`](./docs/superpowers/specs/2026-05-01-strip-residual-signup-design.md). Cfg-gated under `omw_local`: inline AI signup banner, Settings Account & About pages, Help menu, Get Started tab, GITHUB_ISSUES_URL constant, Toggle Warp AI label, and dead-but-compiled-in "Warp" strings in unreachable auth/billing flows. Default cloud build unchanged. Done 2026-05-01 on branch `omw/strip-residual-signup`.
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

- [~] **Gap 1**: `omw-server::SessionRegistry` external-source variant + `PtyController` I/O tap in warp-stripped + per-pane "Share this pane" UI. (~2-3 hours.)
  - [x] Part A — `register_external` + `ExternalSessionSpec` on `SessionRegistry` (commits 01c5594, 8a62ba0; 9 + 12 omw-server tests green).
  - [x] Part B — `omw::pane_share::share_pane` bridge with input/output pumps; `OmwRemoteState::pty_registry()` accessor; `local_tty::TerminalManager::{event_loop_tx,pty_reads_tx}()` accessors (commit 444e815; 5 unit tests green).
  - [ ] Part C — auto-share-on-Phone-click wiring. **Deferred** (commit f7513e2 has a TODO marker explaining why): the click handler lives in manager-agnostic `TerminalView`, and reaching the concrete `local_tty::TerminalManager` requires plumbing through `pane_stack → PaneView::child_data() → downcast Box<dyn TerminalManager>`, which is not a surgical change. Until wired, the daemon spawns a sibling shell on phone connect (original symptom remains).
- [~] **Gap 2**: pair surface in warp-stripped — QR helpers + clipboard auto-copy + tailnet/local pair URL surfacing on Phone-click (commit 939b5b5; 4 pair_modal + 2 qr unit tests green).
  - [x] `omw/qr.rs` — SVG + bool-grid QR renderers using existing `qrcode = "0.14"` dep.
  - [x] `omw/pair_modal.rs` — pure formatter for the "Status / Pair URL / Tailscale / Paired devices / Stop" text (4 tests covering Running+Tailscale, Running+no-Tailscale, Failed, no-emoji-codepoints).
  - [x] `ToggleOmwPair` arm wires `start()` → clipboard auto-copy of pair URL + stderr toast text. URL is now ON THE USER'S CLIPBOARD ready to paste anywhere.
  - [ ] Reactive `View<>`-backed Warp dialog rendering the QR canvas + Copy/Show-QR/Stop buttons. **Deferred:** workspace-view integration (~7 sites in `workspace/view.rs` ~22K lines) is not surgical. The pure-text formatter + QR helpers are ready for a future commit to plug in.
  - [ ] `Paired devices: N` live count. **Stubbed** as `?`: needs a `Pairings::redeem` hook in `omw-remote` to bump a watch channel back to `OmwRemoteState`. Stubbed with TODO in module docs.
- [x] **Gap 3**: `tokio::sync::watch` channel on `OmwRemoteState`; reactive Phone button label/tooltip (commit 5df3476; 4 new remote_state tests cover initial-value, full-transitions, late-subscriber, async bridge). Tooltip per state: Stopped → "Start phone pairing", Starting → "Starting…", Running → "Stop phone pairing", Failed → "Pairing failed — click to retry". Icon variation skipped per scope (single `Icon::Phone` enum variant; `set_active(is_running)` gives the on-state look).
- [x] **Gap 4**: `omw/tailscale.rs` module + multi-origin pinning + orchestration (commits 6eb3287, 9f75c1f, 5e7b06c; 71 → 74 omw-remote tests, 4 tailscale unit tests). `pinned_origin: String` → `pinned_origins: Vec<String>` (3 contract tests in `ws_origin_multi.rs`). Tailscale `detect_status` / `serve_https` / `unserve` shell out via `std::process::Command` with explicit args. `OmwRemoteState::start` orchestrates: bind → detect → serve → assemble pair URL using tailnet hostname → set pinned origins to `[loopback, tailnet]`. `stop` calls `unserve(8787)` best-effort.

**Exit criteria recap:** click Phone button in warp-oss → modal opens with QR + tailnet pair URL → phone scans QR → phone sees the *active Warp terminal pane* in real time → typing on phone echoes on laptop. No manual `tailscale serve` command, no stderr fishing, no fresh-shell confusion.

**Status as of 2026-05-01:** 4 commits land Gaps 3 + 4 fully (114 tests green across the slice). Gaps 1 and 2 each have substantive pieces landed but require one more polish pass: Gap 1 needs the `ToggleOmwPair` handler to plumb `event_loop_tx` + `pty_reads_tx` from the active pane and call `pane_share::share_pane`; Gap 2 needs the reactive Warp dialog wired into `workspace/view.rs`. Both deferrals are documented at the call site (Gap 1) and in the commit message + module doc (Gap 2). The pure-text + URL-on-clipboard path delivers a degraded-but-functional demo: click Phone → URL auto-copied → paste into any QR generator → phone scans → phone connects to a sibling shell (not the Warp pane until Gap 1 Part C lands). Tailscale Serve auto-bootstrap, multi-origin pinning, and reactive button label all work end-to-end.

**Recommended order:** Gap 1 → Gap 4 → Gap 3 → Gap 2 (Gap 2 depends on the others' wiring).

---

## v0.0.2 preview — known follow-ups (deferred from this preview)

Tagged 2026-05-03 as `omw-local-preview-v0.0.2`. The pair-share-attach flow ships as a working preview — phone or browser attaches to a real laptop pane via Tailscale, sees the running TUI, types back. Several tracked-but-not-fixed items came out of the preview testing:

- [ ] **iOS Safari + Tailscale cold-path connect delay.** First WS handshake to a peer can stall 10–30s when the WireGuard path / iOS connection pool is cold (no recent packets). Mitigated by client-side retry-with-timeout (3 × 6s) and an HTTP pre-warm fetch to `/api/v1/host-info` before the WS upgrade, but not eliminated. Empirical signature: clicking "← Sessions" then auto-tracing back to the same terminal connects instantly (warm path); entering fresh from session list always times out and then succeeds on the retry. Needs a Tailscale-side diagnostic (`tailscale ping`, `tailscale netcheck`) — not an app-level bug.
- [ ] **Reverse-direction resize during an active phone session.** When the phone attaches, the daemon ships the laptop pane's size and the phone matches (or asks the laptop to shrink for narrow phones). But if the laptop user resizes the warp window *while* the phone is connected, the new size is not propagated to the phone — the phone xterm stays at the size from initial attach. Wire a resize event from `local_tty::TerminalManager` → `pane_share` → broadcast a fresh `Control{type:"size",…}` frame to all attached subscribers.
- [ ] **Pre-existing test failures documented in plan §3.6.** `crates/omw-remote/tests/ws_connect_token.rs::expired_ts_in_ct_rejects_401` and `crates/omw-remote/tests/ws_pty_session.rs::ts_skew_inbound_rejects` both assert a 30s skew window, but the production code uses 300s for mobile-client clock drift (commit `99519b3`). Either the tests or the constant should align — currently red on `cargo test` but unrelated to v0.4-thin work.
- [ ] **Mac `.dmg` build for the v0.0.2 tag.** All Windows-side wrap-up is on origin (branch + main + tag + release notes). The actual `.dmg` packaging is macOS-only via `bash scripts/build-mac-dmg.sh 0.0.2` (per [CLAUDE.md §5.1](./CLAUDE.md#51-release-naming-conventions-omw_local-previews)) and needs to run from an aarch64-apple-darwin host with full Xcode + Homebrew protoc.

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
- [x] ~~(Phase 0) Umbrella repo license — keep MIT or relicense AGPL~~ → AGPL-3.0 (closed 2026-05-01; see PRD §15 #9)
