import { describe, it, expect, beforeEach } from "vitest";
import {
  savePairing,
  getPairing,
  listPairings,
  deletePairing,
  _resetDbHandleForTests,
  type PairingRecord,
} from "../src/lib/storage/idb";

function makeRecord(hostId: string): PairingRecord {
  return {
    hostId,
    hostUrl: `https://${hostId}.example`,
    hostPubkey: new Uint8Array([1, 2, 3, 4]),
    deviceId: `dev-${hostId}`,
    privateKeyJwk: {
      kty: "OKP",
      crv: "Ed25519",
      d: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
      x: "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBA",
    },
    capabilityTokenB64: "cap-token",
    pairedAt: "2026-04-29T15:00:00Z",
    capabilities: ["session.create", "session.read"],
  };
}

describe("storage/idb", () => {
  beforeEach(async () => {
    // Close the cached handle before deleting so fake-indexeddb doesn't
    // emit `blocked`.
    await _resetDbHandleForTests();
    const req = indexedDB.deleteDatabase("omw-web-controller");
    await new Promise<void>((resolve, reject) => {
      req.onsuccess = () => resolve();
      req.onerror = () => reject(req.error);
      req.onblocked = () => resolve();
    });
  });

  it("savePairing + getPairing round-trips", async () => {
    const r = makeRecord("host-1");
    await savePairing(r);
    const got = await getPairing("host-1");
    expect(got).toBeDefined();
    expect(got?.hostId).toBe("host-1");
    expect(got?.capabilityTokenB64).toBe("cap-token");
    expect(Array.from(got!.hostPubkey)).toEqual([1, 2, 3, 4]);
    expect(got?.privateKeyJwk.kty).toBe("OKP");
  });

  it("listPairings returns all saved records", async () => {
    await savePairing(makeRecord("h-a"));
    await savePairing(makeRecord("h-b"));
    const all = await listPairings();
    const ids = all.map((p) => p.hostId).sort();
    expect(ids).toEqual(["h-a", "h-b"]);
  });

  it("deletePairing removes the record", async () => {
    await savePairing(makeRecord("h-x"));
    await deletePairing("h-x");
    expect(await getPairing("h-x")).toBeUndefined();
  });
});
