// TS integration tests against the REAL omw-keychain-helper binary.
//
// Skipped unless OMW_KEYCHAIN_HELPER points at an existing executable.
// Success-path testing (returning a real secret) happens in v0.2 manual
// integration suite using the OS backend on macOS — the in-memory backend
// is per-process, so there is no way to seed a value for the helper to
// read in CI from another process.
//
// File-boundary note: tests in this file are owned by the Test Overseer
// under the TRD protocol.

import * as fs from "node:fs";
import { describe, expect, it } from "vitest";

const helperPath = process.env.OMW_KEYCHAIN_HELPER;
const helperExists = !!helperPath && fs.existsSync(helperPath);
const skipAll = !helperExists;

describe.skipIf(skipAll)("real omw-keychain-helper binary", () => {
	it("1. NotFound returns undefined (memory backend, fresh process)", async () => {
		const { getKeychainSecret } = await import("../src/keychain.js");
		const out = await getKeychainSecret(
			"keychain:omw/never-set-in-fresh-process",
			{ backend: "memory", binaryPath: helperPath! },
		);
		expect(out).toBeUndefined();
	});

	it("2. bad input throws KeychainHelperError with exitCode === 2", async () => {
		const { getKeychainSecret, KeychainHelperError } = await import(
			"../src/keychain.js"
		);
		try {
			await getKeychainSecret("not-a-keychain-uri", {
				backend: "memory",
				binaryPath: helperPath!,
			});
			throw new Error("expected throw");
		} catch (err) {
			expect(err).toBeInstanceOf(KeychainHelperError);
			expect((err as { exitCode: number }).exitCode).toBe(2);
		}
	});

	// Skipped on macOS because the OS backend IS available there and would
	// not produce exit 3 — that's a different code path documented in the
	// helper's CLI contract.
	it.skipIf(process.platform === "darwin")(
		"3. backend unavailable on Linux/Windows throws exitCode === 3",
		async () => {
			const { getKeychainSecret, KeychainHelperError } = await import(
				"../src/keychain.js"
			);
			try {
				await getKeychainSecret("keychain:omw/x", {
					backend: "os",
					binaryPath: helperPath!,
				});
				throw new Error("expected throw");
			} catch (err) {
				expect(err).toBeInstanceOf(KeychainHelperError);
				expect((err as { exitCode: number }).exitCode).toBe(3);
			}
		},
	);
});
