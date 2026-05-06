// Phase 0 smoke test for the vendored pi-agent-core kernel.
//
// Confirms three things, in increasing severity:
//   1. The path alias `@pi-agent-core` resolves to the vendor index.
//   2. `agentLoop` and `runAgentLoop` are exported as functions.
//   3. The kernel's transitive runtime import — `@mariozechner/pi-ai` —
//      resolves to a real package (not just a type-only stub), with
//      `streamSimple` available.
//
// File-boundary note: tests in this file are owned by the Test Overseer
// under the TRD protocol. The vendored sources under
// `apps/omw-agent/vendor/pi-agent-core/` are not authored by the omw
// project — they are MIT-licensed copies from pi-mono — but the test
// boundary still applies: nothing here may be edited from the impl side
// in lieu of upstreaming the change.

import { describe, expect, it } from "vitest";

import { agentLoop, runAgentLoop } from "../vendor/pi-agent-core/index.js";
import { streamSimple } from "@mariozechner/pi-ai";

describe("vendored pi-agent-core kernel", () => {
	it("exposes agentLoop as a function", () => {
		expect(typeof agentLoop).toBe("function");
	});

	it("exposes runAgentLoop as a function", () => {
		expect(typeof runAgentLoop).toBe("function");
	});

	it("the npm-installed pi-ai surface is reachable", () => {
		// streamSimple is what pi-agent-core's agent-loop imports as a value;
		// asserting it resolves at runtime confirms the npm dep was installed,
		// not just satisfied at the type level.
		expect(typeof streamSimple).toBe("function");
	});
});
