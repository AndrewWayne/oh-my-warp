// Phase 4c3 — vitest unit tests for the beforeToolCall policy hook.
//
// Drives makeBeforeToolCallHook in isolation: deny / allow / ask paths
// plus approval-resolver round-trip and AbortSignal-driven cancellation.
//
// File-boundary note: tests in this file are owned by the Test Overseer
// under the TRD protocol. Implementation lives in src/policy-hook.ts.

import { describe, expect, it } from "vitest";

import {
	extractCommand,
	makeBeforeToolCallHook,
	type ApprovalRequestNotification,
	type PendingApprovalMap,
} from "../src/policy-hook.js";
import type { PolicyConfig } from "../src/policy.js";

import type { BeforeToolCallContext } from "../vendor/pi-agent-core/types.js";

// Build a minimal BeforeToolCallContext for the hook. Only `args` and
// `toolCall` are observed by the hook implementation; the other fields
// are required by the type but otherwise ignored.
function fakeContext(command: string | null, toolCallId = "tc1"): BeforeToolCallContext {
	const args = command === null ? {} : { command };
	const toolCall = {
		type: "toolCall" as const,
		id: toolCallId,
		name: "bash",
		arguments: args,
	};
	return {
		assistantMessage: {} as never,
		toolCall: toolCall as never,
		args,
		context: { systemPrompt: "", messages: [] } as never,
	};
}

const askDefault: PolicyConfig = { mode: "ask_before_write" };
const readOnly: PolicyConfig = { mode: "read_only" };
const trusted: PolicyConfig = { mode: "trusted" };

interface Harness {
	pendingApprovals: PendingApprovalMap;
	requests: ApprovalRequestNotification[];
}

function newHarness(): Harness {
	return { pendingApprovals: new Map(), requests: [] };
}

function makeHook(policy: PolicyConfig, h: Harness) {
	return makeBeforeToolCallHook({
		policy,
		pendingApprovals: h.pendingApprovals,
		notifyApprovalRequest: (req) => h.requests.push(req),
	});
}

describe("makeBeforeToolCallHook", () => {
	it("allow path returns undefined (pass through)", async () => {
		const h = newHarness();
		const hook = makeHook(askDefault, h);
		const result = await hook(fakeContext("ls"), undefined);
		expect(result).toBeUndefined();
		expect(h.requests).toHaveLength(0);
	});

	it("deny path under read_only blocks with policy reason", async () => {
		const h = newHarness();
		const hook = makeHook(readOnly, h);
		const result = await hook(fakeContext("rm foo"), undefined);
		expect(result?.block).toBe(true);
		expect(result?.reason).toContain("denied");
		expect(h.requests).toHaveLength(0);
	});

	it("trusted mode never blocks", async () => {
		const h = newHarness();
		const hook = makeHook(trusted, h);
		const result = await hook(fakeContext("rm -rf /"), undefined);
		expect(result).toBeUndefined();
		expect(h.requests).toHaveLength(0);
	});

	it("ask path emits approval request and awaits resolver", async () => {
		const h = newHarness();
		const hook = makeHook(askDefault, h);
		const promise = hook(fakeContext("rm foo"), undefined);

		// The hook is awaiting; one request should have been emitted.
		// Yield a microtask so the synchronous notify in the hook fires.
		await Promise.resolve();
		expect(h.requests).toHaveLength(1);
		const approvalId = h.requests[0].approvalId;
		expect(typeof approvalId).toBe("string");
		expect(h.pendingApprovals.has(approvalId)).toBe(true);

		// Resolve with approve; hook should pass through.
		h.pendingApprovals.get(approvalId)!("approve");
		h.pendingApprovals.delete(approvalId);
		const result = await promise;
		expect(result).toBeUndefined();
	});

	it("ask -> reject blocks with user-rejected reason", async () => {
		const h = newHarness();
		const hook = makeHook(askDefault, h);
		const promise = hook(fakeContext("rm foo"), undefined);
		await Promise.resolve();
		const id = h.requests[0].approvalId;
		h.pendingApprovals.get(id)!("reject");
		h.pendingApprovals.delete(id);
		const result = await promise;
		expect(result?.block).toBe(true);
		expect(result?.reason).toContain("rejected");
	});

	it("ask -> cancel blocks with cancelled reason", async () => {
		const h = newHarness();
		const hook = makeHook(askDefault, h);
		const promise = hook(fakeContext("rm foo"), undefined);
		await Promise.resolve();
		const id = h.requests[0].approvalId;
		h.pendingApprovals.get(id)!("cancel");
		h.pendingApprovals.delete(id);
		const result = await promise;
		expect(result?.block).toBe(true);
		expect(result?.reason).toContain("cancelled");
	});

	it("AbortSignal mid-wait resolves as cancel", async () => {
		const h = newHarness();
		const hook = makeHook(askDefault, h);
		const ac = new AbortController();
		const promise = hook(fakeContext("rm foo"), ac.signal);
		await Promise.resolve();
		expect(h.requests).toHaveLength(1);
		ac.abort();
		const result = await promise;
		expect(result?.block).toBe(true);
		expect(result?.reason).toContain("cancelled");
	});

	it("non-bash tool call (no args.command) passes through", async () => {
		const h = newHarness();
		const hook = makeHook(askDefault, h);
		const result = await hook(fakeContext(null), undefined);
		expect(result).toBeUndefined();
		expect(h.requests).toHaveLength(0);
	});
});

describe("extractCommand", () => {
	it("returns the command string for bash-shaped args", () => {
		expect(extractCommand(fakeContext("ls"))).toBe("ls");
	});

	it("returns null for missing command field", () => {
		expect(extractCommand(fakeContext(null))).toBeNull();
	});
});
