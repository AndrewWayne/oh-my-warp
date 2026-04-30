import { openDB, type IDBPDatabase } from "idb";

const DB_NAME = "omw-web-controller";
const DB_VERSION = 1;
const STORE_PAIRINGS = "pairings";

export interface PairingRecord {
  hostId: string;
  hostUrl: string;
  hostPubkey: Uint8Array;
  deviceId: string;
  privateKeyJwk: JsonWebKey;
  capabilityTokenB64: string;
  pairedAt: string;
  capabilities: string[];
}

let dbPromise: Promise<IDBPDatabase> | undefined;

function getDb(): Promise<IDBPDatabase> {
  if (!dbPromise) {
    dbPromise = openDB(DB_NAME, DB_VERSION, {
      upgrade(db) {
        if (!db.objectStoreNames.contains(STORE_PAIRINGS)) {
          db.createObjectStore(STORE_PAIRINGS, { keyPath: "hostId" });
        }
      },
    });
  }
  return dbPromise;
}

export async function savePairing(p: PairingRecord): Promise<void> {
  const db = await getDb();
  await db.put(STORE_PAIRINGS, p);
}

export async function getPairing(
  hostId: string
): Promise<PairingRecord | undefined> {
  const db = await getDb();
  return (await db.get(STORE_PAIRINGS, hostId)) as
    | PairingRecord
    | undefined;
}

export async function listPairings(): Promise<PairingRecord[]> {
  const db = await getDb();
  return (await db.getAll(STORE_PAIRINGS)) as PairingRecord[];
}

export async function deletePairing(hostId: string): Promise<void> {
  const db = await getDb();
  await db.delete(STORE_PAIRINGS, hostId);
}

// Test-only hook: close + drop the cached handle so fake-indexeddb can
// be reset between tests without a `blocked` deletion.
export async function _resetDbHandleForTests(): Promise<void> {
  if (dbPromise) {
    try {
      const db = await dbPromise;
      db.close();
    } catch {
      /* ignore */
    }
  }
  dbPromise = undefined;
}
