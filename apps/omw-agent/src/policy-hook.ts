// pi-agent `beforeToolCall` hook that gates tool execution through the
// policy classifier and the GUI approval queue.
//
// Decision flow:
//   classify(args.command, policy) ->
//     allow  -> return undefined  (pass through to executor)
//     deny   -> return { block: true, reason: "policy denied <mode>" }
//     ask    -> emit approval/request notification, await approval/decide
//              -> approve  -> return undefined
//              -> reject   -> return { block: true, reason: "rejected by user" }
//              -> cancel   -> return { block: true, reason: "approval cancelled" }
//
// Only the bash tool is classified today (it's the only command-shaped
// tool in our scope). Other tools pass through. Until Phase 5 wires the
// bash AgentTool, this hook never fires in production — Phase 4c3 is
// preparation work.
//
// Threat-model invariants:
// - I-1: the hook never logs args or context.toolCall verbatim. Tool
//   call summaries forwarded over the WS are the caller's responsibility
//   to redact (Phase 4c4 GUI side).
//
// File-boundary note: tests live in test/policy-hook.test.ts and are
// owned by the Test Overseer under the TRD protocol.

import { randomUUID } from "node:crypto";

import type {
	BeforeToolCallContext,
	BeforeToolCallResult,
} from "../vendor/pi-agent-core/types.js";

import { classify, type PolicyConfig } from "./policy.js";

export type ApprovalDecision = "approve" | "reject" | "cancel";

/** Resolver for one in-flight approval Promise. */
export type ApprovalResolver = (decision: ApprovalDecision) => void;

/** Map of approvalId -> Promise resolver. Owned by `Session`. */
export type PendingApprovalMap = Map<string, ApprovalResolver>;

/** Body of an approval/request notification. */
export interface ApprovalRequestNotification {
	approvalId: string;
	toolCall: unknown;
}

export interface PolicyHookDeps {
	/** Effective policy. Per-session, immutable for the session's life. */
	policy: PolicyConfig;
	/** Shared map for pending Ask approvals; the JSON-RPC handler resolves
	 * via `session/applyApprovalDecision` when the GUI replies. */
	pendingApprovals: PendingApprovalMap;
	/** Emit an approval/request notification upstream over the JSON-RPC
	 * surface. The body is the recipient's responsibility to wire to the
	 * outbound stdout writer; the hook only calls this. */
	notifyApprovalRequest: (req: ApprovalRequestNotification) => void;
}

export type BeforeToolCallHook = (
	context: BeforeToolCallContext,
	signal?: AbortSignal,
) => Promise<BeforeToolCallResult | undefined>;

/** Build a `beforeToolCall` hook bound to `deps`. */
export function makeBeforeToolCallHook(deps: PolicyHookDeps): BeforeToolCallHook {
	return async (context, signal) => {
		const command = extractCommand(context);
		if (command === null) {
			// Non-bash tool; nothing for the policy classifier to gate.
			return undefined;
		}
		const decision = classify(command, deps.policy);
		if (decision === "allow") return undefined;
		if (decision === "deny") {
			return {
				block: true,
				reason: `policy: command denied (${deps.policy.mode})`,
			};
		}
		// ask: queue an approval and block on the resolver. AbortSignal
		// integration: if the loop's signal fires while we're waiting,
		// resolve as "cancel" and let the caller block normally.
		const approvalId = randomUUID();
		const result = await new Promise<ApprovalDecision>((resolve) => {
			deps.pendingApprovals.set(approvalId, resolve);
			if (signal) {
				const onAbort = () => {
					if (deps.pendingApprovals.delete(approvalId)) {
						resolve("cancel");
					}
				};
				if (signal.aborted) {
					onAbort();
					return;
				}
				signal.addEventListener("abort", onAbort, { once: true });
			}
			deps.notifyApprovalRequest({
				approvalId,
				toolCall: context.toolCall,
			});
		});
		if (result === "approve") return undefined;
		if (result === "reject") {
			return { block: true, reason: "rejected by user" };
		}
		return { block: true, reason: "approval cancelled" };
	};
}

/**
 * Pull the bash command string out of a tool call's validated arguments.
 * Returns null if this isn't a command-shaped tool (no `command` string
 * field on `args`). Called outside the hook factory so tests can verify
 * the heuristic.
 */
export function extractCommand(context: BeforeToolCallContext): string | null {
	const args = context.args;
	if (args && typeof args === "object" && "command" in args) {
		const v = (args as { command: unknown }).command;
		return typeof v === "string" ? v : null;
	}
	return null;
}
