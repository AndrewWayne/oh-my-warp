// Client-side pair-redeem. See specs/byorc-protocol.md §3.2 and §3.5.

import {
  generateKeypair,
  exportPrivateKeyJwk,
  exportPublicKeyRaw,
  _b64u,
  type CryptoPrivateKey,
} from "./crypto/ed25519";

export interface PairUrl {
  baseUrl: string; // e.g. "https://hostname.tailnet.ts.net" or "http://127.0.0.1:8787"
  token: string; // base32 Crockford
}

export interface RedeemResult {
  hostId: string;
  hostUrl: string;
  hostPubkey: Uint8Array;
  deviceId: string;
  capabilityTokenB64: string;
  capabilities: string[];
  privateKey: CryptoPrivateKey;
  privateKeyJwk: JsonWebKey;
}

export class PairError extends Error {
  public readonly code: string;
  public readonly httpStatus?: number;
  public readonly details?: string;

  constructor(code: string, httpStatus?: number, details?: string) {
    super(details ? `${code}: ${details}` : code);
    this.name = "PairError";
    this.code = code;
    this.httpStatus = httpStatus;
    this.details = details;
  }
}

export function parsePairUrl(input: string): PairUrl | null {
  if (typeof input !== "string") return null;
  const s = input.trim();
  if (s.length === 0) return null;
  let u: URL;
  try {
    u = new URL(s);
  } catch {
    return null;
  }
  if (u.protocol !== "https:" && u.protocol !== "http:") return null;
  if (u.pathname !== "/pair") return null;
  const token = u.searchParams.get("t");
  if (!token) return null;
  // Crockford base32 alphabet (case-insensitive); reject anything outside.
  if (!/^[0-9A-HJKMNP-TV-Za-hjkmnp-tv-z]+$/.test(token)) return null;
  // Build baseUrl as origin (scheme + host + port), no trailing slash.
  const baseUrl = `${u.protocol}//${u.host}`;
  return { baseUrl, token };
}

interface RedeemResponseShape {
  v?: number;
  device_id?: string;
  capabilities?: string[];
  capability_token?: string;
  host_pubkey?: string;
  host_name?: string;
  host_id?: string;
  issued_at?: string;
  expires_at?: string;
}

interface ErrorBodyShape {
  error?: { code?: string; message?: string; trace_id?: string };
}

const HTTP_STATUS_TO_CODE: Record<number, string> = {
  400: "invalid_body",
  404: "token_unknown",
  409: "token_already_used",
  410: "token_expired",
};

export async function redeemPairing(
  pairUrl: PairUrl,
  deviceName: string,
  platform: string,
  fetchImpl: typeof fetch = fetch,
): Promise<RedeemResult> {
  const { privateKey, publicKey } = await generateKeypair();
  const pubBytes = await exportPublicKeyRaw(publicKey);
  const privateKeyJwk = await exportPrivateKeyJwk(privateKey);

  const nonceBytes = new Uint8Array(16);
  crypto.getRandomValues(nonceBytes);

  const body = {
    v: 1,
    pairing_token: pairUrl.token,
    device_pubkey: _b64u.encode(pubBytes),
    device_name: deviceName,
    platform,
    client_nonce: _b64u.encode(nonceBytes),
  };

  const url = `${pairUrl.baseUrl}/api/v1/pair/redeem`;

  let res: Response;
  try {
    res = await fetchImpl(url, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });
  } catch (e) {
    throw new PairError(
      "network_error",
      undefined,
      e instanceof Error ? e.message : String(e),
    );
  }

  if (!res.ok) {
    let code = HTTP_STATUS_TO_CODE[res.status] ?? "bad_response";
    let details: string | undefined;
    try {
      const errJson = (await res.json()) as ErrorBodyShape;
      if (errJson?.error?.code) code = errJson.error.code;
      if (errJson?.error?.message) details = errJson.error.message;
    } catch {
      /* response wasn't JSON; keep status-derived code */
    }
    throw new PairError(code, res.status, details);
  }

  let parsed: RedeemResponseShape;
  try {
    parsed = (await res.json()) as RedeemResponseShape;
  } catch {
    throw new PairError("bad_response", res.status, "non-JSON response");
  }

  const {
    device_id,
    capabilities,
    capability_token,
    host_pubkey,
    host_id,
    host_name,
  } = parsed;

  if (
    typeof device_id !== "string" ||
    !Array.isArray(capabilities) ||
    typeof capability_token !== "string" ||
    typeof host_pubkey !== "string"
  ) {
    throw new PairError("bad_response", res.status, "missing required fields");
  }

  let hostPubkey: Uint8Array;
  try {
    hostPubkey = _b64u.decode(host_pubkey);
  } catch {
    throw new PairError("bad_response", res.status, "invalid host_pubkey b64");
  }
  if (hostPubkey.length !== 32) {
    throw new PairError(
      "bad_response",
      res.status,
      `host_pubkey wrong length: ${hostPubkey.length}`,
    );
  }

  const hostId = host_id ?? host_name ?? pairUrl.baseUrl;

  return {
    hostId,
    hostUrl: pairUrl.baseUrl,
    hostPubkey,
    deviceId: device_id,
    capabilityTokenB64: capability_token,
    capabilities: capabilities.map((c) => String(c)),
    privateKey,
    privateKeyJwk,
  };
}
