# BYORC Protocol

Status: Draft v0.1 — pre-implementation, **not yet externally reviewed**
Last updated: 2026-04-29
Owners: TBD

This spec defines the wire protocol between the **Web Controller** (and any future client) and the **`omw-remote`** daemon: pairing handshake, request signing, replay defense, capability scopes, WebSocket framing, origin pinning, and revocation.

It is the v0.4 implementation gate per [PRD §13](../PRD.md#13-phased-roadmap):

> **Gate:** `specs/byorc-protocol.md` reviewed externally and merged before any code work in this phase.

The threat model this protocol defends against lives in [`specs/threat-model.md`](./threat-model.md) §3.2 and §2.3. The invariants this protocol enforces are I-2, I-3, I-4, I-5, I-6, I-7, I-9, I-10, I-11, I-15 from the threat model.

---

## 0. Goals & Non-Goals

### Goals

- App-layer authentication on every HTTP request and every WebSocket frame, regardless of transport.
- Tailnet trust alone is insufficient — a malicious app on the same tailnet must not be able to forge requests.
- Per-device identity with fast, total revocation (≤1 second).
- Capability scopes that prevent privilege escalation between feature classes (PTY ≠ agent ≠ audit ≠ pair-admin).
- Replay-resistant under typical clock skew.
- Pairing must be possible with the user's bare hands and a phone camera — no separate password manager round-trip.
- Versioned wire format; we can add a v2 without breaking v1 clients in the field.
- Implementable in Rust on the host side and in browser-grade JS (Web Controller) on the client side, with no exotic crypto primitives.

### Non-Goals (v1)

- End-to-end encryption between the Web Controller and `omw-remote`. Tailscale provides transport confidentiality; we don't double-encrypt.
- Forward secrecy. Tailscale's WireGuard provides this at the transport layer.
- Public-key-infrastructure / certificate authorities. Pairing is direct, peer-to-peer.
- OAuth / OIDC. We are not a federated-identity system.
- Multi-user authorization on a single host (one host = one user; PRD §11.4).
- Defending against a host whose private key has been exfiltrated (at that point, the OS is compromised; out of threat model per §1).

---

## 1. Vocabulary

| Term | Meaning |
|---|---|
| **Host** | The machine running `omw-remote` (the user's home Mac in Journey B). Owns a long-lived **host pairing key** (§3.1). |
| **Device** | A paired client — the user's phone, second laptop, etc. Each device has its own **device key** generated at pair time. |
| **Pairing token** | A short-lived secret displayed by the host (`omw pair qr`) and consumed once by a device to bootstrap the pair. |
| **Device record** | Server-side state for a paired device: device id, public key, capabilities, paired-at, last-seen, revocation status. |
| **Capability token** | A short JSON object signed by the **host pairing key** that names the device id, its capabilities, and an expiry. The device presents it on every request. |
| **Request signature** | An Ed25519 signature by the **device key** over a canonical-request representation. Proves possession of the device key for this specific request. |
| **Nonce** | A 128-bit random value, single-use within the replay window. |
| **WS session** | A long-lived WebSocket connection. Has its own session id and sequence numbers. |

---

## 2. Cryptographic Choices

| Use | Algorithm | Library hints |
|---|---|---|
| All signatures (host pairing key, device key, capability tokens, requests, WS frames) | **Ed25519** (RFC 8032) | Rust: `ed25519-dalek` v2 with `pkcs8`. Browser: `WebCrypto` `Ed25519` (widely supported in 2026) or `@noble/ed25519`. |
| Body hashing, pairing-token hashing, audit chain | **SHA-256** | Rust: `sha2`. Browser: `WebCrypto`. |
| Random nonces, pairing tokens | OS RNG (`getrandom` / `crypto.getRandomValues`) | — |
| Encoding | **Base64url (no padding)** for binary fields; **Base32 Crockford** for the pairing token (QR / human-friendly) | — |

No other primitives are permitted in v1. Adding a new one (e.g. ECDH for E2E encryption) requires a protocol revision.

---

## 3. Pairing

### 3.1 Host pairing key

When `omw-remote` is first initialized on a host, it generates a **host pairing key** — a long-lived Ed25519 keypair stored in the OS keychain via `omw-keychain`. The host pairing key never leaves the host. It is used only to sign **capability tokens** (§5).

The host pairing public key is exposed via `GET /api/v1/host-info` (unauthenticated; needed for unpaired clients to verify capability tokens during the redeem step).

### 3.2 Pair-flow overview

```
User runs `omw pair qr` on the host.
  ↓
Host generates a 256-bit random PAIRING_TOKEN.
Host stores SHA-256(PAIRING_TOKEN) with TTL = 10 min, used = false.
Host renders QR encoding the pairing URL:
  https://<hostname.tailnet.ts.net>/pair?t=<base32(PAIRING_TOKEN)>
  ↓
User scans QR on the device (phone camera, or pastes URL on desktop).
  ↓
Device generates Ed25519 keypair (DEV_PRIV, DEV_PUB).
  ↓
Device POSTs /api/v1/pair/redeem with body:
  {
    "v": 1,
    "pairing_token": "<base32(PAIRING_TOKEN)>",
    "device_pubkey": "<base64url(DEV_PUB)>",
    "device_name": "Mark's iPhone",
    "platform": "ios",
    "client_nonce": "<base64url(16 random bytes)>"
  }
  ↓
Host validates:
  - hash(token) exists, not used, not expired
  - device_pubkey is valid Ed25519 (32 bytes)
  - body well-formed
  ↓
Host marks token used. Creates a device record:
  device_id = first 16 hex chars of SHA-256(DEV_PUB)
  capabilities = ["pty:read", "agent:read", "audit:read"]   (default = read_only)
  paired_at = now()
  ↓
Host issues a CAPABILITY_TOKEN (§5).
  ↓
Host responds 200:
  {
    "v": 1,
    "device_id": "<hex>",
    "capabilities": ["pty:read", "agent:read", "audit:read"],
    "capability_token": "<base64url(JSON capability token)>",
    "host_pubkey": "<base64url(host pairing pubkey)>",
    "host_name": "home-mac",
    "issued_at": "2026-04-29T15:00:00Z",
    "expires_at": "2026-05-29T15:00:00Z"
  }
  ↓
Device stores DEV_PRIV, DEV_PUB, capability_token, host_pubkey in IndexedDB
(or platform secure storage on a native shim — Beyond v1).
```

### 3.3 Pairing token format

- **Length:** 256 bits, base32 Crockford = 52 characters. Trailing checksum optional in v1.
- **Storage:** SHA-256 hash only (column `pairings.token_hash` per PRD §10).
- **TTL:** 10 minutes from issuance.
- **Single-use:** the row is marked `used_at` on first successful redeem; a second attempt returns `409 token_already_used`.
- **Revocation on host shutdown:** all unused tokens are invalidated when `omw remote stop` runs. Device pairings (capability tokens) survive — only pending pairings die.

### 3.4 What the pairing token is *not*

- Not a capability — it cannot be presented on `/api/v1/sessions` or any other endpoint.
- Not stored client-side after redeem — the device discards it.
- Not reusable across hosts — embedded host hostname.

### 3.5 Errors during pair

| HTTP | Condition |
|---|---|
| `400 invalid_body` | Malformed JSON, missing fields, bad base64. |
| `400 invalid_pubkey` | `device_pubkey` not a valid Ed25519 point. |
| `404 token_unknown` | Hash not found (mistyped or never issued). |
| `410 token_expired` | TTL elapsed. |
| `409 token_already_used` | Already redeemed. |

All redeem attempts (success and failure) are logged with the source IP, device name, and decision in the `request_log` table (PRD §10).

---

## 4. Request Signing (HTTP)

Every authenticated HTTP request carries:

- A **capability token** as `Authorization: Bearer <capability_token>`.
- A request **signature** in `X-Omw-Signature`.
- A **nonce** in `X-Omw-Nonce`.
- A **timestamp** in `X-Omw-Ts` (RFC 3339, UTC).

### 4.1 Canonical request

The canonical-request string the device signs is:

```
HTTP-METHOD       \n
PATH              \n
QUERY-STRING      \n
TS                \n
NONCE             \n
SHA256(BODY)      \n
DEVICE_ID         \n
PROTOCOL_VERSION  \n
```

Where:

- `HTTP-METHOD` is uppercase ASCII (`GET`, `POST`, ...).
- `PATH` is the URL path including a leading `/`, *not* URL-decoded.
- `QUERY-STRING` is the canonicalized query string: keys lex-sorted, values URL-encoded, joined with `&`. Empty → empty string.
- `TS` is the value of `X-Omw-Ts` verbatim.
- `NONCE` is the value of `X-Omw-Nonce` verbatim (base64url).
- `SHA256(BODY)` is hex (lowercase) of the raw request body bytes; for empty body, hex of SHA-256 of zero bytes.
- `DEVICE_ID` is the device id from the capability token.
- `PROTOCOL_VERSION` is the literal `1`.

The signature is `Ed25519(device_priv, canonical_request_bytes)`, base64url-encoded.

### 4.2 Server verification (in order)

1. Parse `Authorization: Bearer <capability_token>`. Decode JSON, verify outer signature against `host_pubkey` (loaded once at server start). Reject if invalid → `401 capability_invalid`.
2. Verify capability token is not expired → `401 capability_expired`.
3. Verify device id is not in revocation list → `401 device_revoked`.
4. Parse `X-Omw-Ts`. Compute `|now() - ts|`. Reject if > 30 seconds → `401 ts_skew`.
5. Parse `X-Omw-Nonce`. Look up in nonce store. Reject if seen → `403 nonce_replayed`.
6. Reconstruct canonical request from incoming method/path/query/body + headers.
7. Verify `X-Omw-Signature` against the device public key embedded in the capability token. Reject if invalid → `401 signature_invalid`.
8. Verify the requested route is permitted by the capability scopes (§5.2). Reject if not → `403 capability_scope`.
9. Insert nonce into the nonce store with TTL = 60 s (twice the window, to absorb out-of-order delivery).
10. Append a row to `request_log` (PRD §10) with `accepted = true`. Process the request.

Any failure also writes a `request_log` row with `accepted = false` and `reason`. Failures past step 1 are not signaled with which step failed beyond the broad code (avoids leaking diagnostic information).

### 4.3 Headers summary

| Header | Required on | Format |
|---|---|---|
| `Authorization: Bearer <cap_token>` | Every authenticated request | base64url JSON |
| `X-Omw-Signature` | Every authenticated request | base64url Ed25519(64) |
| `X-Omw-Nonce` | Every authenticated request | base64url(16 random bytes) |
| `X-Omw-Ts` | Every authenticated request | RFC 3339 UTC, e.g. `2026-04-29T15:00:00.123Z` |
| `X-Omw-Protocol-Version` | Optional; defaults to 1 | integer |
| `Content-Type` | Where applicable | standard |

### 4.4 Body hashing of streaming uploads

v1 does not support streaming request bodies; signed requests carry a finite body. Streaming is reserved for WebSocket (§7).

---

## 5. Capability Tokens

A capability token is a JSON object signed by the **host pairing key**.

### 5.1 Structure

```json
{
  "v": 1,
  "device_id": "a1b2c3d4e5f6a7b8",
  "device_pubkey": "<base64url(32)>",
  "host_id": "home-mac",
  "capabilities": ["pty:read", "agent:read", "audit:read"],
  "issued_at": "2026-04-29T15:00:00Z",
  "expires_at": "2026-05-29T15:00:00Z",
  "sig": "<base64url(64)>"
}
```

`sig` is `Ed25519(host_pairing_priv, canonical_token_bytes)`, where `canonical_token_bytes` is the JSON object **with the `sig` field omitted**, serialized with sorted keys and no whitespace.

The token is transmitted as `base64url(JSON-with-sig)`.

### 5.2 Scope strings

The v1 scope vocabulary is closed. Adding a new scope requires a protocol revision.

| Scope | Grants |
|---|---|
| `pty:read` | `GET /api/v1/sessions`, `WS /ws/v1/pty/:id` (read-only mode). |
| `pty:write` | `POST /api/v1/sessions/:id/input`, `POST /api/v1/sessions/:id/resize`, `POST /api/v1/sessions/:id/kill`, `WS /ws/v1/pty/:id` (read-write). |
| `agent:read` | `GET /api/v1/agent/tasks`, `WS /ws/v1/agent/:id` (observe only). |
| `agent:write` | `POST /api/v1/agent/tasks`, `POST /api/v1/agent/tasks/:id/approve`, `POST /api/v1/agent/tasks/:id/reject`. |
| `audit:read` | `GET /api/v1/audit`. |
| `pair:admin` | `GET /api/v1/devices`, `POST /api/v1/devices/:id/revoke`. (Only granted via `omw pair upgrade <id>` on the host CLI; never the default.) |

A scope grants nothing more than what the row says. There is no implicit hierarchy: `pty:write` does **not** imply `pty:read` for the purpose of scope checks (the route enforces the exact scope it requires; if a route requires either, both scopes are listed in the route's accepted-scope set).

### 5.3 Default scopes at pair time

- New devices receive `["pty:read", "agent:read", "audit:read"]`.
- The host can upgrade a device with `omw pair upgrade <device_id> --add pty:write,agent:write`.
- `pair:admin` is granted only via `omw pair upgrade <device_id> --add pair:admin` and prompts the user with a typed confirmation.

This satisfies invariant I-6 (default read-only).

### 5.4 Token lifetime and rotation

- Default `expires_at`: 30 days from issuance.
- The Web Controller automatically requests a renewed token via `POST /api/v1/cap/renew` (signed with the still-valid token + a fresh request signature) starting 7 days before expiry.
- An expired token cannot be renewed; the device must re-pair.

### 5.5 What if the host pairing key changes?

If the host pairing key is regenerated (lost keychain, fresh install), all existing capability tokens become unverifiable and every device must re-pair. The host CLI emits a warning when a regenerate is detected.

---

## 6. Replay Defense

### 6.1 Nonce store

- 128-bit random per request.
- Stored with TTL = 60 seconds (twice the timestamp window) in an in-memory LRU bounded at 1M entries (≈16 MB).
- On overflow: oldest evicted; nonces older than the window are already irrelevant.
- Persisted across `omw-remote` restarts? **No.** Restart drains the nonce store; the timestamp window of 30 s makes a cross-restart replay window only that wide, and request_log on disk records what was already accepted.

### 6.2 Timestamp window

- 30 seconds. Justified by typical clock skew on consumer devices (mobile NTP is usually ±10 s) plus network latency (tailnet RTT ≪ 1 s).
- Enforced as `|now_host − ts_client| ≤ 30s`, not strictly forward-only.
- Open question: should we tighten to 15 s once we see field data? Decide post-v0.4.

### 6.3 `request_log`

Per PRD §10. Every redeem and every authenticated request inserts a row. Used for:

- Forensic review (`omw audit search`).
- Detecting stuck or replayed nonces (large `accepted = false reason = nonce_replayed` counts → alert).
- Capacity planning.

### 6.4 What replay defense does *not* cover

- Two devices issuing the same nonce by chance — astronomically unlikely with 128 bits.
- A device re-using a nonce from outside the window — rejected by the timestamp check, not the nonce check.
- An attacker sniffing a request and replaying within 30 s — Tailscale's transport encryption prevents the sniff in the first place. If transport is compromised, the threat model changes.

---

## 7. WebSocket Framing

### 7.1 Handshake

The WS upgrade is a normal HTTP request, signed per §4. Headers MUST include:

- `Authorization`, `X-Omw-Signature`, `X-Omw-Nonce`, `X-Omw-Ts` (as for HTTP).
- `Origin: https://<hostname.tailnet.ts.net>` — checked against the host's pinned origin (§8).

A successful handshake assigns a `ws_session_id` (UUID) and starts the session at sequence 0.

### 7.2 Frame schema

Every WS message is a JSON envelope:

```json
{
  "v": 1,
  "seq": 42,
  "ts": "2026-04-29T15:00:00.500Z",
  "kind": "input" | "output" | "control" | "ping" | "pong",
  "payload": { ... },
  "sig": "<base64url(64)>"
}
```

- `seq` starts at 0, monotonically increases per direction. Server tracks an inbound high-water mark; rejects regressions.
- `ts` is RFC 3339 UTC.
- `kind` is one of the literals; payload schema is `kind`-specific.
- `sig` is `Ed25519(device_priv, canonical_frame)`, where `canonical_frame` is the JSON with `sig` omitted, serialized with sorted keys and no whitespace.

Server-to-client frames are signed with the **host pairing key**.

### 7.3 Per-frame verification

For each inbound frame, server:

1. Verifies signature against the device key for this WS session.
2. Verifies `seq > last_seen_seq`.
3. Verifies `|now − ts| ≤ 30 s`.
4. Verifies the device's capability token is still valid (not expired, not revoked) — checked at frame granularity, not handshake granularity (invariant I-9).
5. Updates `last_seen_seq`.

A failure on any step closes the WS with `close code 4401 auth_failed` and logs the cause to `request_log`.

### 7.4 Why per-frame and not per-handshake

- Revocation must propagate within 1 second (invariant I-7). Per-handshake auth can't cancel an in-flight stream. Per-frame auth enforces revocation at the frame layer.
- Sequence numbers protect against an attacker who somehow captures and re-injects a frame inside the live session (transport-layer compromise scenario).
- Protocol-level bugs in WebSocket implementations (frame fragmentation, etc.) don't have an authentication free lunch.

### 7.5 Heartbeats

- Client → server `kind: "ping"` every 15 s; server replies `kind: "pong"`.
- Three missed pongs → client tears down. Server tears down a session that hasn't sent a frame in 60 s.
- Heartbeats are signed like any other frame.

---

## 8. Origin Pinning, CORS

### 8.1 Pinned origin

At first listen, `omw-remote` records its expected origin as `https://<hostname>.<tailnet>.ts.net`. (Detected from the listener config; on Tailscale Serve start, the user's tailnet hostname is known.)

Loopback uses `https://127.0.0.1:8787` as the expected origin.

### 8.2 Handshake check

Every WS handshake MUST present `Origin: <pinned_origin>`. Mismatch → `403 origin_mismatch`.

HTTP requests do **not** require `Origin` because legitimate non-browser clients (the omw GUI, scripts, native shim) won't send it. CORS preflight responses, when applicable, allow only the pinned origin.

### 8.3 CORS posture

- `Access-Control-Allow-Origin: <pinned_origin>` (single allowed origin).
- `Access-Control-Allow-Credentials: false` (we don't use cookies).
- `Access-Control-Allow-Headers: Authorization, X-Omw-Signature, X-Omw-Nonce, X-Omw-Ts, Content-Type, X-Omw-Protocol-Version`.
- `Access-Control-Allow-Methods: GET, POST, OPTIONS`.

No wildcards. No `null` origin.

### 8.4 What this does *not* protect against

- A user explicitly opening `omw-remote` from a different origin (rebinds, ngrok-style detours) — out of scope; documented as user error.
- Browser extensions on the user's paired device — same threat surface as a malicious extension on any web app. See threat-model §2.3.

---

## 9. Revocation

### 9.1 Mechanism

The host's `omw-remote` keeps a **revocation list** in memory and on disk: a set of `(device_id, revoked_at)` pairs. Sourced from the `devices.revoked_at` column in SQLite.

On revoke (`omw pair revoke <id>` or `POST /api/v1/devices/:id/revoke`):

1. Update `devices.revoked_at = now()`.
2. Insert into in-memory revocation set.
3. Iterate active WS sessions; close any whose `device_id` matches (`close code 4401 auth_failed`).
4. Append audit entry.

### 9.2 1-second propagation guarantee

- HTTP requests check revocation before signature verification result is acted on (§4.2 step 3).
- WS frames check revocation per-frame (§7.3 step 4).
- The active-sessions tear-down (step 9.1.3) is synchronous in the revoke handler; the user's CLI returns only after all matched sessions are closed.

This satisfies invariant I-7.

### 9.3 What happens to a revoked device's data

- Its `pty_sessions` entries (if any) are owned by the host GUI, not the device. Sessions persist; only the device's *access* is revoked.
- Its capability token is rejected forever.
- It must re-pair to regain access.

### 9.4 Bulk revoke

`omw remote stop --all` revokes every device atomically (transaction in SQLite, broadcast tear-down). Used on suspected compromise.

---

## 10. Versioning

### 10.1 Wire-level

- Protocol version is encoded in the canonical request and in capability tokens as `v: 1`.
- A server that does not understand the version returns `426 upgrade_required` with `X-Omw-Min-Version` and `X-Omw-Max-Version`.
- A client SHOULD include `X-Omw-Protocol-Version` on every request; absent it defaults to 1.

### 10.2 Forward compatibility rules

For v1.x non-breaking additions:

- New optional fields in JSON envelopes — clients ignore unknown fields.
- New scopes — capability tokens may include them; route checks remain unchanged for old routes.
- New routes — older capability tokens won't have the relevant scopes; requests fail with `403 capability_scope`, not a protocol error.

For v2 breaking changes:

- Bump `v` in canonical request and capability tokens.
- v1 and v2 servers can coexist on the same host (different ports, or version-gated routes); the migration story is a v1.x deliverable.

### 10.3 What constitutes "breaking"

- Changing the canonical-request layout.
- Changing the signature algorithm.
- Removing a scope.
- Changing the WS frame schema in a way that v1 servers can't ignore.

---

## 11. Error Model

All error responses have the shape:

```json
{
  "error": {
    "code": "nonce_replayed",
    "message": "Request rejected for replay",
    "trace_id": "<uuid>"
  }
}
```

The `code` is from the closed v1 vocabulary. `message` is human-readable, never carries cryptographic detail (no "wrong signature byte 17"). `trace_id` correlates with `request_log`.

### 11.1 v1 error codes

| Code | HTTP | Meaning |
|---|---|---|
| `invalid_body` | 400 | Malformed JSON or missing required field. |
| `invalid_pubkey` | 400 | Pubkey is not a valid Ed25519 point. |
| `capability_invalid` | 401 | Capability token signature does not verify, or token malformed. |
| `capability_expired` | 401 | Capability token past `expires_at`. |
| `capability_scope` | 403 | Token scopes do not authorize the requested route. |
| `signature_invalid` | 401 | Request signature does not verify. |
| `ts_skew` | 401 | Client timestamp outside 30 s window. |
| `nonce_replayed` | 403 | Nonce already seen within window. |
| `device_revoked` | 401 | Device has been revoked. |
| `origin_mismatch` | 403 | Origin header does not match pinned origin (WS only). |
| `token_unknown` | 404 | Pairing token not found. |
| `token_expired` | 410 | Pairing token TTL elapsed. |
| `token_already_used` | 409 | Pairing token already redeemed. |
| `upgrade_required` | 426 | Protocol version unsupported. |
| `internal` | 500 | Server-side bug. |

The body of a 4xx response is always at most 256 bytes — no cryptographic detail leaks.

WS error close codes use `4xxx` (private range):

- `4401 auth_failed` — any authentication failure mid-stream.
- `4403 capability_scope` — scope downgrade mid-session would close the relevant streams.
- `4426 upgrade_required` — protocol negotiation failure.

---

## 12. Wire Examples

### 12.1 Signed HTTP request

```
POST /api/v1/sessions/abc123/input HTTP/1.1
Host: home-mac.tail-net-1234.ts.net
Authorization: Bearer eyJ2IjoxLCJkZXZpY2VfaWQiOiJhMWIyYzNkNGU1ZjZhN2I4Iiwi...
X-Omw-Signature: K8N0wL7m0Lr3...d3Q
X-Omw-Nonce: bGqCZUaJ8TJ1qnLqzv5ENg
X-Omw-Ts: 2026-04-29T15:00:01.234Z
X-Omw-Protocol-Version: 1
Content-Type: application/json
Content-Length: 24

{"data": "ls -la\n"}
```

Canonical request signed:

```
POST
/api/v1/sessions/abc123/input

2026-04-29T15:00:01.234Z
bGqCZUaJ8TJ1qnLqzv5ENg
e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
a1b2c3d4e5f6a7b8
1
```

(Note: empty query string → empty line after path; SHA-256 shown is for empty body. The example body `{"data": "ls -la\n"}` would give a different hash — illustration intentionally inconsistent to highlight the canonicalization, not the math.)

### 12.2 Signed WS frame (input)

```json
{
  "v": 1,
  "seq": 17,
  "ts": "2026-04-29T15:00:02.001Z",
  "kind": "input",
  "payload": { "data": "echo hello\n" },
  "sig": "L1iC4mG..."
}
```

### 12.3 Capability token (decoded)

```json
{
  "v": 1,
  "device_id": "a1b2c3d4e5f6a7b8",
  "device_pubkey": "VHGm2bL3...",
  "host_id": "home-mac",
  "capabilities": ["pty:read", "pty:write", "agent:read"],
  "issued_at": "2026-04-29T15:00:00Z",
  "expires_at": "2026-05-29T15:00:00Z",
  "sig": "QlR0w7..."
}
```

---

## 13. Test Surface

This protocol's correctness is asserted by the test targets in [`specs/test-plan.md`](./test-plan.md):

| Test target | Surface covered |
|---|---|
| Contract tests (test-plan §A.2) | Every documented route must have positive + negative cases (auth failure, replay, scope violation, expired token, malformed body, origin mismatch). |
| BYORC validator fuzzer (test-plan §3.1) | Never panics; rejects malformed signatures; rejects expired/replayed nonces; capability scope enforced. |
| Pairing token property tests (test-plan §2.5) | Single-use, expiry, hashed-at-rest. |
| E2E Journey B (test-plan §B.2) | Full pair → attach → input → output → revoke (≤1s) loop. |
| Tier D protocol review (test-plan §D) | This document, before v0.4 implementation begins. |

---

## 14. Open Questions

1. **Capability-token revocation lookup vs replay.** Revocation list is consulted before signature verification (§4.2 step 3). Should we instead reject after signature verification, to avoid leaking "valid revoked device" vs "invalid junk request" timing? Decide with reviewer.
2. **WS sequence-number rollover.** 64-bit u64; wraps after 2^63 messages. Rollover handling: reset on session new only. Document or harden?
3. **Token format — JSON vs CBOR.** JSON is friendlier to debug; CBOR is shorter on the wire. v1 = JSON; reconsider if QR-token-equivalent embedded use cases emerge.
4. **Pairing token base-32 vs base-64url.** Base-32 Crockford is QR-friendlier and human-typeable; base-64url is shorter. v1 = Crockford for the user-typed case. Confirm with the Web Controller designer.
5. **Renewal endpoint authentication.** `/api/v1/cap/renew` requires the still-valid token plus a request signature. Should it also require a separately-authenticated user-presence step (push prompt to host)? Probably yes for capability-token *upgrade* (`pair:admin`); probably no for plain renewal. Decide before v0.4.
6. **Multiple capability tokens per device.** v1 = one. Beyond v1: short-lived per-task tokens?
7. **Audit-export streaming.** `/api/v1/audit` returns finite paginated results in v1. Streaming export (NDJSON over WS) is a v1.x extension; ensure scope `audit:read` is sufficient (no separate `audit:export` scope needed).
8. **Time source.** Server uses local clock for `now`. NTP drift on the host directly affects the 30 s window. Hard requirement or graceful tolerance?
9. **Browser Ed25519 support.** `WebCrypto` `Ed25519` is broadly available in 2026, but if any target client lacks it the polyfill (`@noble/ed25519`) is mandatory. Confirm before v0.4.
10. **Origin pinning across Tailscale rename.** If the user renames their tailnet, the pinned origin needs to update. Provide a CLI rotation path (`omw remote reorigin`).

---

## 15. Review Checklist (for the external reviewer)

- [ ] Every threat in [`threat-model.md`](./threat-model.md) §3.2 has a corresponding mechanism in this spec.
- [ ] Every invariant I-2..I-11, I-15 from threat-model.md has an enforcement point in this spec.
- [ ] No cryptographic primitive outside §2.
- [ ] Every error code is listed in §11.1.
- [ ] Every route in PRD §9.2 has a scope assignment in §5.2.
- [ ] No inline secrets in any wire example.
- [ ] Open questions §14 acknowledged; reviewer's adjudication on contested items captured in PR.

---

*End of BYORC protocol spec v0.1.*
