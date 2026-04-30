import {
  type CryptoPrivateKey,
  sign,
  _b64u,
} from "../crypto/ed25519";
import { canonicalBytes, bodyHashHex } from "../crypto/canonical";

export interface AuthContext {
  deviceId: string;
  privateKey: CryptoPrivateKey;
  capabilityTokenB64: string;
  baseUrl: string; // e.g., "https://hostname.tailnet.ts.net"
  protocolVersion: number;
}

export class ApiClient {
  constructor(private auth: AuthContext) {}

  async request(
    method: string,
    path: string,
    body?: object
  ): Promise<Response> {
    const ts = new Date().toISOString();
    const nonceBytes = new Uint8Array(16);
    crypto.getRandomValues(nonceBytes);
    const nonce = _b64u.encode(nonceBytes);

    const bodyBytes = body
      ? new TextEncoder().encode(JSON.stringify(body))
      : new Uint8Array(0);
    const bodySha256Hex = await bodyHashHex(bodyBytes);

    const canonical = canonicalBytes({
      method,
      path,
      query: "",
      ts,
      nonce,
      bodySha256Hex,
      deviceId: this.auth.deviceId,
      protocolVersion: this.auth.protocolVersion,
    });

    const sigBytes = await sign(this.auth.privateKey, canonical);
    const sigB64u = _b64u.encode(sigBytes);

    const headers: Record<string, string> = {
      Authorization: `Bearer ${this.auth.capabilityTokenB64}`,
      "X-Omw-Signature": sigB64u,
      "X-Omw-Nonce": nonce,
      "X-Omw-Ts": ts,
      "X-Omw-Protocol-Version": String(this.auth.protocolVersion),
    };
    if (bodyBytes.length > 0) {
      headers["Content-Type"] = "application/json";
    }

    return await fetch(`${this.auth.baseUrl}${path}`, {
      method: method.toUpperCase(),
      headers,
      body: bodyBytes.length ? bodyBytes : undefined,
    });
  }

  get(path: string): Promise<Response> {
    return this.request("GET", path);
  }
  post(path: string, body: object): Promise<Response> {
    return this.request("POST", path, body);
  }
  delete(path: string): Promise<Response> {
    return this.request("DELETE", path);
  }
}
