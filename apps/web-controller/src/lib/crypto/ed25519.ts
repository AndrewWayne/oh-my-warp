// Ed25519 helpers.
//
// We use @noble/ed25519 (pure-JS) for keygen/sign/verify. By default
// @noble/ed25519 v2 calls WebCrypto's `crypto.subtle.digest('SHA-512', ...)`
// for its internal hashing step — which only works in a "secure context"
// (HTTPS or localhost). On a phone hitting the daemon over plain HTTP via a
// tailnet IP, `crypto.subtle` is undefined and pairing throws
// "crypto.subtle must be defined".
//
// Fix: wire @noble/hashes/sha512 into noble-ed25519's `etc.sha512Async` /
// `sha512Sync` hook so SHA-512 runs in pure JS, with no secure-context
// requirement. Bundle cost: ~5 KB. The phone now pairs over plain HTTP.

import * as nobleEd from "@noble/ed25519";
import { sha512 } from "@noble/hashes/sha512";

// Wire pure-JS SHA-512 into noble-ed25519. Both Async and Sync paths are
// set so that future call-site changes (sign vs signAsync) keep working.
// `etc.concatBytes` is the documented helper for joining the variadic
// message chunks noble passes us.
nobleEd.etc.sha512Async = (...m) =>
  Promise.resolve(sha512(nobleEd.etc.concatBytes(...m)));
nobleEd.etc.sha512Sync = (...m) => sha512(nobleEd.etc.concatBytes(...m));

export interface Ed25519PrivateKey {
  readonly type: "ed25519-private";
  readonly raw: Uint8Array; // 32 bytes seed
}

export interface Ed25519PublicKey {
  readonly type: "ed25519-public";
  readonly raw: Uint8Array; // 32 bytes
}

// Re-export under CryptoKey-ish names so callers can refer to a
// uniform type across the app even though we don't use SubtleCrypto.
export type CryptoPrivateKey = Ed25519PrivateKey;
export type CryptoPublicKey = Ed25519PublicKey;

export async function generateKeypair(): Promise<{
  privateKey: Ed25519PrivateKey;
  publicKey: Ed25519PublicKey;
}> {
  const seed = nobleEd.utils.randomPrivateKey();
  const pub = await nobleEd.getPublicKeyAsync(seed);
  return {
    privateKey: { type: "ed25519-private", raw: seed },
    publicKey: { type: "ed25519-public", raw: pub },
  };
}

export async function sign(
  privateKey: Ed25519PrivateKey,
  data: Uint8Array
): Promise<Uint8Array> {
  return await nobleEd.signAsync(data, privateKey.raw);
}

export async function verify(
  publicKey: Ed25519PublicKey,
  data: Uint8Array,
  sig: Uint8Array
): Promise<boolean> {
  return await nobleEd.verifyAsync(sig, data, publicKey.raw);
}

export async function exportPublicKeyRaw(
  publicKey: Ed25519PublicKey
): Promise<Uint8Array> {
  return new Uint8Array(publicKey.raw);
}

export async function importPublicKeyRaw(
  raw: Uint8Array
): Promise<Ed25519PublicKey> {
  if (raw.length !== 32) {
    throw new Error(`Ed25519 public key must be 32 bytes, got ${raw.length}`);
  }
  return { type: "ed25519-public", raw: new Uint8Array(raw) };
}

export async function exportPrivateKeyRaw(
  privateKey: Ed25519PrivateKey
): Promise<Uint8Array> {
  return new Uint8Array(privateKey.raw);
}

export async function importPrivateKeyRaw(
  raw: Uint8Array
): Promise<Ed25519PrivateKey> {
  if (raw.length !== 32) {
    throw new Error(`Ed25519 private seed must be 32 bytes, got ${raw.length}`);
  }
  return { type: "ed25519-private", raw: new Uint8Array(raw) };
}

// JsonWebKey-shaped helpers for IndexedDB persistence. We piggy-back on
// the JWK shape so the storage layer types match the eventual WebCrypto
// migration; the `d` and `x` fields hold base64url-encoded raw bytes.

function b64uEncode(bytes: Uint8Array): string {
  let s = "";
  for (const b of bytes) s += String.fromCharCode(b);
  return btoa(s).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

function b64uDecode(s: string): Uint8Array {
  const pad = s.length % 4 === 0 ? "" : "=".repeat(4 - (s.length % 4));
  const b64 = s.replace(/-/g, "+").replace(/_/g, "/") + pad;
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

export async function exportPrivateKeyJwk(
  privateKey: Ed25519PrivateKey
): Promise<JsonWebKey> {
  const pub = await nobleEd.getPublicKeyAsync(privateKey.raw);
  return {
    kty: "OKP",
    crv: "Ed25519",
    d: b64uEncode(privateKey.raw),
    x: b64uEncode(pub),
  };
}

export async function importPrivateKeyJwk(
  jwk: JsonWebKey
): Promise<Ed25519PrivateKey> {
  if (!jwk.d) throw new Error("JWK missing private 'd'");
  return { type: "ed25519-private", raw: b64uDecode(jwk.d) };
}

export const _b64u = { encode: b64uEncode, decode: b64uDecode };
