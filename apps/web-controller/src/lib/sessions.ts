// POST /api/v1/sessions helper.
//
// `ws_handler` in `crates/omw-remote/src/server.rs` requires the WS path's
// `:session_id` to be (a) a valid UUID and (b) registered in
// `omw_server::SessionRegistry`. So before navigating to /terminal/:hostId/:id
// we have to ask the host to spawn a shell session for us — it returns the
// UUID of the freshly-registered session, which the Terminal page then uses
// for the WS upgrade.
//
// Without this step, /ws/v1/pty/<anything> 404s with "session_not_found".

import { ApiClient } from "./api/client";
import { importPrivateKeyJwk } from "./crypto/ed25519";
import type { PairingRecord } from "./storage/idb";

/** Create a fresh shell session on the host. Returns the UUID. */
export async function createDefaultSession(
  pairing: PairingRecord,
  name = "main",
): Promise<string> {
  const privateKey = await importPrivateKeyJwk(pairing.privateKeyJwk);
  const client = new ApiClient({
    deviceId: pairing.deviceId,
    privateKey,
    capabilityTokenB64: pairing.capabilityTokenB64,
    baseUrl: pairing.hostUrl,
    protocolVersion: 1,
  });
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
