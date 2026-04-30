// Canonical-request encoding per byorc-protocol §4.1.

export interface CanonicalRequest {
  method: string;
  path: string;
  query: string;
  ts: string; // RFC 3339
  nonce: string; // base64url
  bodySha256Hex: string;
  deviceId: string;
  protocolVersion: number; // 1
}

export function canonicalBytes(req: CanonicalRequest): Uint8Array {
  const lines = [
    req.method.toUpperCase(),
    req.path,
    req.query,
    req.ts,
    req.nonce,
    req.bodySha256Hex,
    req.deviceId,
    String(req.protocolVersion),
  ];
  return new TextEncoder().encode(lines.join("\n"));
}

export async function bodyHashHex(body: Uint8Array): Promise<string> {
  // Copy into a plain ArrayBuffer to satisfy lib.dom's BufferSource shape
  // across TS versions / lib targets.
  const buf = new ArrayBuffer(body.byteLength);
  new Uint8Array(buf).set(body);
  const digest = await crypto.subtle.digest("SHA-256", buf);
  const bytes = new Uint8Array(digest);
  let hex = "";
  for (const b of bytes) hex += b.toString(16).padStart(2, "0");
  return hex;
}
