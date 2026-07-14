// Canonical-request encoding per byorc-protocol §4.1.
//
// Hashing was previously done via `crypto.subtle.digest('SHA-256', ...)`
// — which only works in a secure context (HTTPS / localhost / file://).
// On the phone hitting the daemon over plain HTTP via a tailnet IP,
// `crypto.subtle` is undefined and `connectPty` fails with
// "Cannot read properties of undefined (reading 'digest')".
//
// Use @noble/hashes/sha256 instead — pure JS, no secure-context
// requirement. Same fix shape as crypto/ed25519.ts (which routes its
// SHA-512 through @noble/hashes via noble-ed25519's etc.sha512Async).

import { sha256 } from "@noble/hashes/sha256";

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
  // Per byorc-protocol §4.1, the canonical bytes are:
  //   METHOD\nPATH\nQUERY\nTS\nNONCE\nhex(SHA256(BODY))\nDEVICE_ID\nVERSION\n
  // Note the TRAILING \n after VERSION. The Rust server side
  // (`crates/omw-remote/src/auth.rs::CanonicalRequest::to_bytes`) builds
  // it via `format!("{}\n{}\n...{}\n", ...)`, so each line terminates with
  // \n including the last. A previous version of this file used
  // `lines.join("\n")` which omits the trailing newline — that produced a
  // valid-looking signature that the server rejected as
  // "signature_invalid" because the canonical bytes differed by exactly
  // one byte. Spotted while running JS-client-vs-Rust-server end-to-end
  // for the first time (existing tests were JS-vs-mock or Rust-vs-Rust).
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
  return new TextEncoder().encode(lines.join("\n") + "\n");
}

export async function bodyHashHex(body: Uint8Array): Promise<string> {
  const bytes = sha256(body);
  let hex = "";
  for (const b of bytes) hex += b.toString(16).padStart(2, "0");
  return hex;
}
