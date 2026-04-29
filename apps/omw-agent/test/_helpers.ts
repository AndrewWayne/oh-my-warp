// Shared test helpers for the omw-agent test suite.
//
// File-boundary note: this file is owned by the Test Overseer under the TRD
// protocol. The Executor MUST NOT modify it.
//
// `assertNoSecretLeak` performs a partial-prefix sweep: it asserts that no
// substring of the secret of length >= minWindow appears in the rendered
// blob. This is stricter than checking only the full secret because a buggy
// impl could truncate the secret (e.g. log the first 8 chars in an error
// message) and still pass a full-string check.

import { expect } from "vitest";

/**
 * Sweep every substring of `secret` of length >= `minWindow` and assert
 * none of them appear in `rendered`. Defaults to a 4-character window —
 * matches the Rust-side helper used by omw-keychain-helper tests so the two
 * layers enforce the same guarantee.
 */
export function assertNoSecretLeak(
	rendered: string,
	secret: string,
	minWindow = 4,
): void {
	if (secret.length < minWindow) {
		// Secret is shorter than the window; fall back to direct containment.
		expect(rendered).not.toContain(secret);
		return;
	}
	for (let start = 0; start + minWindow <= secret.length; start++) {
		for (let end = start + minWindow; end <= secret.length; end++) {
			const window = secret.slice(start, end);
			if (rendered.includes(window)) {
				throw new Error(
					`secret leak: window of length ${window.length} (${JSON.stringify(window)}) ` +
						`from secret ${JSON.stringify(secret)} found in rendered output ` +
						`${JSON.stringify(rendered)}`,
				);
			}
		}
	}
}

/**
 * Render an error to a single string blob covering the surfaces a caller
 * is most likely to log: `String(err)`, `err.message`, and the JSON form
 * including non-enumerable properties.
 */
export function renderError(err: unknown): string {
	const message = err instanceof Error ? err.message : "";
	let json = "";
	try {
		json = JSON.stringify(err, Object.getOwnPropertyNames(err as object));
	} catch {
		json = "<unserializable>";
	}
	return `${String(err)}\n${message}\n${json}`;
}
