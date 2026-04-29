// Global type augmentations for the omw-agent app.
//
// `@types/node@22` exposes `fetch`, `Headers`, `RequestInit`, etc. as
// globals via its `web-globals/fetch.d.ts`, but it does NOT declare
// `HeadersInit` globally — it lives only in `undici-types`. The test
// suite (test/cli.test.ts) refers to `HeadersInit` as a global, so we
// re-export it here so a strict `tsc` build resolves the symbol.

import type { HeadersInit as UndiciHeadersInit } from "undici-types";

declare global {
	type HeadersInit = UndiciHeadersInit;
}

export {};
