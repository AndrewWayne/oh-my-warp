import { fileURLToPath } from "node:url";
import { defineConfig } from "vitest/config";

// Vitest config for apps/omw-agent.
//
// We test Node child_process spawning, so the environment MUST be 'node'.
// We use explicit imports of describe/it/expect (no globals) so tests stay
// self-contained and friendly to type-checking.
//
// `resolve.alias` mirrors the `paths` mapping in tsconfig.json. Vitest does
// not automatically pick up tsconfig path aliases; without this, importing
// `@pi-agent-core` from a test file fails with "Failed to load url".

const vendorRoot = fileURLToPath(new URL("./vendor/pi-agent-core", import.meta.url));

export default defineConfig({
	resolve: {
		alias: {
			"@pi-agent-core/": `${vendorRoot}/`,
			"@pi-agent-core": `${vendorRoot}/index.ts`,
		},
	},
	test: {
		environment: "node",
		include: ["test/**/*.test.ts"],
		globals: false,
		testTimeout: 10000,
	},
});
