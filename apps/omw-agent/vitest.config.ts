import { defineConfig } from "vitest/config";

// Vitest config for apps/omw-agent.
//
// We test Node child_process spawning, so the environment MUST be 'node'.
// We use explicit imports of describe/it/expect (no globals) so tests stay
// self-contained and friendly to type-checking.

export default defineConfig({
	test: {
		environment: "node",
		include: ["test/**/*.test.ts"],
		globals: false,
		testTimeout: 10000,
	},
});
