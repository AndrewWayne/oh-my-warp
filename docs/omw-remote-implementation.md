# omw-remote Implementation Solution

Status: Design proposal — pre-implementation
Last updated: 2026-04-29

---

## 1. Core Insight: The GUI Is the Session Anchor

The PRD (§5.2, §8.3) specifies tmux control-mode as the session backend for `omw-remote`, motivated by durability: sessions should outlive daemon restarts and network drops. But there is a simpler anchor that is already present in the v1.0 architecture — **the omw GUI process itself**.

Warp's upstream remote-control feature demonstrates this model. When a remote client connects, the warp app swaps the session's command executor at runtime (`Session::set_command_executor()`) from a local executor to a `RemoteServerCommandExecutor`. The session's PTY and shell process continue to live in the GUI; the remote path just redirects input and taps output. Session state is durable across network drops because the GUI keeps running — not because tmux keeps running.

For v1 BYORC (Journey B: phone → home Mac), the omw GUI is always running on the host. This is the right anchor for v1. The tmux path is still the right answer if we ever need **headless** BYORC (no GUI), but that is a Beyond-v1 use case not listed in §3.1 Committed Scope.

---

## 2. Proposed Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    omw GUI (host machine)                │
│  Rust + WarpUI                                          │
│  Holds PTY sessions, shell processes, executor registry │
└──────────────────────────┬──────────────────────────────┘
                           │ HTTP + WS (loopback)
                           ▼
┌─────────────────────────────────────────────────────────┐
│                       omw-server                        │
│  Session registry: exposes session list + PTY event bus │
│  Single audit-log writer                                │
└──────────┬──────────────────────────┬───────────────────┘
           │                          │
           ▼                          ▼
   omw-agent                    omw-remote
   (unchanged)                  Rust / axum
                                Subscribes to session PTY events
                                Pairing, signed requests
                                Web Controller bundle
                                    │
                                    │ Tailscale Serve
                                    ▼
                            User's tailnet
                         Web Controller (browser)
```

`omw-remote` is not a standalone PTY manager. It is a **protocol bridge**: it subscribes to session events from `omw-server`, multiplexes them over authenticated WebSocket connections to the Web Controller, and routes input back.

---

## 3. IPC: How omw-remote Talks to GUI Sessions

`omw-server` already sits between the GUI and all backend processes. Extend it with two internal surfaces:

### 3.1 Session registry

```
GET  /internal/v1/sessions           → list of active GUI sessions with metadata
WS   /internal/v1/sessions/:id/pty   → PTY output stream (binary, terminal bytes)
POST /internal/v1/sessions/:id/input → write bytes into the session's PTY
POST /internal/v1/sessions/:id/resize → resize the PTY (cols × rows)
```

This is a **loopback-only** internal API — not part of the tailnet-exposed surface. `omw-remote` subscribes to it; nothing else does.

### 3.2 Executor model (mirrors warp's approach)

When the Web Controller sends input to a session, `omw-remote` posts to `omw-server`'s input endpoint. `omw-server` writes it directly into the PTY master fd of the GUI session — identical to what a local keypress does. No executor swap is required for v1 because the input path is always PTY writes, not command dispatch. (Executor swapping becomes relevant when we want agent sessions controllable over BYORC, deferred to a later phase.)

---

## 4. Session Lifecycle

```
1. omw GUI starts → sessions exist locally, not yet exposed
2. User runs `omw remote start`
3. omw-remote daemon boots, binds 127.0.0.1:8787
4. omw-remote subscribes to omw-server's session registry WS feed
5. User runs `omw pair qr` → one-time token issued, QR shown
6. Remote device scans QR → POST /api/v1/pair/redeem
7. omw-remote validates token, generates per-device Ed25519 record
8. Web Controller connects WS /ws/v1/pty/:session_id (signed)
9. omw-remote pipes omw-server PTY stream → Web Controller WS
10. Web Controller keystrokes → omw-remote → omw-server PTY input
11. User closes tab or runs `omw remote stop` → WS torn down
12. GUI session continues locally, unaffected
```

Sessions survive WS drops (step 11) because the GUI holds the PTY. Reconnect is a new WS handshake — the session is still there.

Sessions do **not** survive GUI process exit. This is the correct trade-off for v1: Journey B users have the omw GUI running on their home machine; headless recovery via tmux is a Beyond-v1 concern.

---

## 5. What omw-remote Owns

| Concern | Owner | Note |
|---|---|---|
| PTY session lifetime | omw GUI + omw-server | omw-remote is a consumer |
| PTY output streaming to Web Controller | omw-remote | Subscribes to omw-server event bus |
| Input routing to PTY | omw-remote → omw-server | omw-server writes to PTY fd |
| Resize propagation | omw-remote → omw-server | Forwarded as ioctl |
| Device pairing + Ed25519 validation | omw-remote | Per PRD §5.2 / specs/byorc-protocol.md |
| Request signing + replay window | omw-remote | Per specs/byorc-protocol.md |
| Web Controller bundle serving | omw-remote | Bundle embedded in binary |
| Audit entries for remote events | omw-server (sole writer) | omw-remote calls audit append API |

---

## 6. What Changes vs. the PRD

| PRD spec | Proposed change | Rationale |
|---|---|---|
| §8.3: "tmux control-mode is the only path" | GUI PTY is the path; tmux deferred to Beyond-v1 | GUI is always present in v1 use cases; tmux adds complexity for no v1 benefit |
| `omw-pty` crate with tmux backend | `omw-pty` becomes a thin PTY utility crate (portable-pty wrapper), not a tmux manager | Still needed for agent subprocesses; just not the session anchor |
| FR-4.4: "Sessions are tmux-backed and survive restarts" | Sessions survive WS drops and omw-remote restarts; survive GUI crashes only with tmux (Beyond-v1) | Narrower guarantee, but sufficient for Journey B |
| `pty_sessions` SQLite table with `tmux_session` column | `pty_sessions` references GUI session IDs, no tmux column | Schema simplifies |
| §9.2 route `POST /api/v1/sessions` (create new session) | Deferred; v1 only exposes existing GUI sessions | Creating headless sessions requires tmux or equivalent; not needed in v1 |

Everything else in the PRD's BYORC spec is unchanged: pairing protocol, Ed25519 keypairs, QR flow, signed requests, replay window, capability scopes, revocation, audit, Web Controller PWA.

---

## 7. Relation to the omw Product

### Fits v1.0 committed scope cleanly

The PRD's v1 exit criterion for BYORC is Journey B: pair one host, attach to a session over Tailscale Serve, ask the agent something, approve a write, see the audit entry. The GUI-anchored model satisfies this completely. The home Mac running omw is always up with the GUI open — that is the assumed topology.

### Simplifies the v0.4 implementation gate

The PRD gates v0.4 on `specs/byorc-protocol.md` being reviewed and merged. With the tmux requirement removed, the scope of v0.4 shrinks: no tmux control-mode integration, no headless session management, no `portable-pty` → tmux bridge. The Web Controller and pairing protocol remain the same.

### Preserves the upgrade path

The GUI-anchored model does not close off tmux. If a Beyond-v1 use case requires headless sessions (e.g., omw running as a server without a display), `omw-server` can add a second session source backed by tmux. `omw-remote` subscribes to the same internal session event bus regardless of source. The Web Controller does not care.

### One constraint to document

The omw GUI must be running on the host for BYORC to work in v1. This is a real limitation: if the GUI crashes, remote sessions are interrupted. Document this prominently in v1.0 release notes and in the `omw remote start` output. The tmux path resolves it for users who need headless reliability.

---

## 8. Implementation Steps (v0.4 scope)

```
1. [omw-server] Add internal session registry API
   verify: omw-remote can list GUI sessions and subscribe to PTY output stream

2. [omw-remote] HTTP + WS daemon skeleton (axum, 127.0.0.1:8787)
   verify: GET /api/v1/status returns 200

3. [omw-remote] Pairing flow — per specs/byorc-protocol.md
   verify: QR → redeem → device record persisted; replay rejected

4. [omw-remote] Signed-request middleware
   verify: unsigned request → 401; replayed nonce → 403

5. [omw-remote] PTY bridge: subscribe omw-server stream → WS /ws/v1/pty/:id
   verify: Web Controller sees terminal output; input round-trips

6. [omw-remote] Revocation: active WS drops within 1s
   verify: `omw pair revoke <id>` → open WS closes ≤ 1s

7. [apps/web-controller] Pairing, terminal (xterm.js), agent view, approvals
   verify: Journey B end-to-end over Tailscale Serve on a real device

8. [omw-server] Audit entries for remote events (connect, input, approval)
   verify: `omw audit tail` shows remote device id on each entry
```

Each step maps to an FR from the PRD (FR-4, FR-5, FR-7). The `specs/byorc-protocol.md` spec must be written and reviewed before step 3.
