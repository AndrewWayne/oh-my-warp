// Signed CRUD helpers for /api/v1/sessions.
//
// `ws_handler` in `crates/omw-remote/src/server.rs` requires the WS path's
// `:session_id` to be (a) a valid UUID and (b) registered in
// `omw_server::SessionRegistry`. The Sessions page lists what's currently
// registered (`GET`); a row's "Open" navigates to `/terminal/:hostId/:id`
// for that pre-existing UUID. "Stop" removes the registration (`DELETE`).
// `createDefaultSession` (POST) remains for callers that need a fresh
// daemon-spawned shell.

import { ApiClient } from "./api/client";
import { importPrivateKeyJwk } from "./crypto/ed25519";
import type { PairingRecord } from "./storage/idb";

export interface SessionMeta {
  id: string;
  name: string;
  createdAt: string;
  alive: boolean;
}

async function buildClient(pairing: PairingRecord): Promise<ApiClient> {
  const privateKey = await importPrivateKeyJwk(pairing.privateKeyJwk);
  return new ApiClient({
    deviceId: pairing.deviceId,
    privateKey,
    capabilityTokenB64: pairing.capabilityTokenB64,
    baseUrl: pairing.hostUrl,
    protocolVersion: 1,
  });
}

/** Create a fresh shell session on the host. Returns the UUID. */
export async function createDefaultSession(
  pairing: PairingRecord,
  name = "main",
): Promise<string> {
  const client = await buildClient(pairing);
  const res = await client.post("/api/v1/sessions", { name });
  if (!res.ok) {
    let detail = `${res.status}`;
    try {
      detail = (await res.text()) || detail;
    } catch {
      /* ignore */
    }
    throw new Error(`session_create_failed: ${detail}`);
  }
  const data = (await res.json()) as { id?: unknown };
  if (typeof data.id !== "string" || data.id.length === 0) {
    throw new Error("session_create_failed: missing id in response");
  }
  return data.id;
}

/** List the host's currently-registered sessions. */
export async function listSessions(
  pairing: PairingRecord,
): Promise<SessionMeta[]> {
  const client = await buildClient(pairing);
  const res = await client.get("/api/v1/sessions");
  if (!res.ok) {
    let detail = `${res.status}`;
    try {
      detail = (await res.text()) || detail;
    } catch {
      /* ignore */
    }
    throw new Error(`session_list_failed: ${detail}`);
  }
  const data = (await res.json()) as { sessions?: unknown };
  const raw = Array.isArray(data.sessions) ? data.sessions : [];
  const out: SessionMeta[] = [];
  for (const row of raw) {
    if (!row || typeof row !== "object") continue;
    const r = row as Record<string, unknown>;
    if (
      typeof r.id !== "string" ||
      typeof r.name !== "string" ||
      typeof r.created_at !== "string" ||
      typeof r.alive !== "boolean"
    ) {
      continue;
    }
    out.push({
      id: r.id,
      name: r.name,
      createdAt: r.created_at,
      alive: r.alive,
    });
  }
  return out;
}

/** Stop sharing / kill a session by id. 404 is treated as "already gone". */
export async function deleteSession(
  pairing: PairingRecord,
  id: string,
): Promise<void> {
  const client = await buildClient(pairing);
  const res = await client.delete(`/api/v1/sessions/${encodeURIComponent(id)}`);
  if (res.ok || res.status === 204 || res.status === 404) return;
  let detail = `${res.status}`;
  try {
    detail = (await res.text()) || detail;
  } catch {
    /* ignore */
  }
  throw new Error(`session_delete_failed: ${detail}`);
}
