import { describe, it, expect } from "vitest";
import {
  canonicalBytes,
  bodyHashHex,
  type CanonicalRequest,
} from "../src/lib/crypto/canonical";

const sample: CanonicalRequest = {
  method: "GET",
  path: "/api/v1/sessions",
  query: "",
  ts: "2026-04-29T15:00:00.123Z",
  nonce: "AAAAAAAAAAAAAAAAAAAAAA",
  bodySha256Hex:
    "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
  deviceId: "device-abc",
  protocolVersion: 1,
};

describe("canonicalBytes", () => {
  it("emits exactly 8 lines each terminated with \\n (per byorc §4.1)", () => {
    const bytes = canonicalBytes(sample);
    const text = new TextDecoder().decode(bytes);
    // The canonical format includes a trailing \n after the version line
    // — same shape the Rust server reconstructs in
    // crates/omw-remote/src/auth.rs::CanonicalRequest::to_bytes via
    // `format!("{}\n{}\n...{}\n", ...)`. Without the trailing \n the
    // server rejects the signature as `signature_invalid`.
    expect(text.endsWith("\n")).toBe(true);
    // 8 fields + trailing \n => split("\n") yields 9 elements with the
    // last being "".
    const parts = text.split("\n");
    expect(parts).toHaveLength(9);
    expect(parts[0]).toBe("GET");
    expect(parts[1]).toBe("/api/v1/sessions");
    expect(parts[2]).toBe("");
    expect(parts[3]).toBe(sample.ts);
    expect(parts[4]).toBe(sample.nonce);
    expect(parts[5]).toBe(sample.bodySha256Hex);
    expect(parts[6]).toBe(sample.deviceId);
    expect(parts[7]).toBe("1");
    expect(parts[8]).toBe(""); // the trailing newline
  });

  it("uppercases the method", () => {
    const bytes = canonicalBytes({ ...sample, method: "post" });
    const text = new TextDecoder().decode(bytes);
    expect(text.split("\n")[0]).toBe("POST");
  });

  it("changes when any field is tampered", () => {
    const base = canonicalBytes(sample);
    const baseHex = Array.from(base)
      .map((b) => b.toString(16).padStart(2, "0"))
      .join("");

    const fields: Array<keyof CanonicalRequest> = [
      "method",
      "path",
      "query",
      "ts",
      "nonce",
      "bodySha256Hex",
      "deviceId",
    ];
    for (const f of fields) {
      const tampered = { ...sample, [f]: String(sample[f]) + "X" };
      const out = canonicalBytes(tampered);
      const outHex = Array.from(out)
        .map((b) => b.toString(16).padStart(2, "0"))
        .join("");
      expect(outHex, `tampering ${f}`).not.toBe(baseHex);
    }

    const versionTamper = canonicalBytes({ ...sample, protocolVersion: 2 });
    const versionHex = Array.from(versionTamper)
      .map((b) => b.toString(16).padStart(2, "0"))
      .join("");
    expect(versionHex).not.toBe(baseHex);
  });
});

describe("bodyHashHex", () => {
  it("hashes empty body to the well-known SHA-256-of-zero-bytes value", async () => {
    const h = await bodyHashHex(new Uint8Array(0));
    expect(h).toBe(
      "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
  });

  it('hashes "abc" to the well-known value', async () => {
    const h = await bodyHashHex(new TextEncoder().encode("abc"));
    expect(h).toBe(
      "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
  });
});
