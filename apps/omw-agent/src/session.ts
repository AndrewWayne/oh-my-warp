// Session — wraps a single pi-agent-core `agentLoop` invocation against an
// OMW-shaped provider config. Phase 1: text-only, no tools.
//
// One Session per omw-agent session id. Multiple sessions share a single
// stdio process; each carries its own message history and its own
// AbortController so `session/cancel` can abort one without disturbing
// others.
//
// Threat-model invariants (specs/threat-model.md):
// - I-1: API keys are resolved per-call via the keychain bridge; never
//   logged, never folded into emitted events. The `getApiKey` callback
//   returns the raw secret to pi-ai's stream layer, which uses it for the
//   Authorization header and discards it.

// Path: relative imports into the vendored pi-agent-core directory so the
// emitted ESM resolves at runtime. tsconfig `paths` is compile-only — Node
// cannot follow `@pi-agent-core` without a real package or imports map.
import {
	agentLoop,
	type AgentContext,
	type AgentEvent,
	type AgentLoopConfig,
	type AgentMessage,
} from "../vendor/pi-agent-core/index.js";
import { getModel, type Message, type Model } from "@mariozechner/pi-ai";

import { DEFAULT_POLICY, type PolicyConfig } from "./policy.js";
import {
	makeBeforeToolCallHook,
	type ApprovalDecision,
	type ApprovalRequestNotification,
	type PendingApprovalMap,
} from "./policy-hook.js";

export type ProviderKind = "openai" | "anthropic" | "openai-compatible" | "ollama";

export interface ProviderConfig {
	kind: ProviderKind;
	/** Keychain reference resolved via the helper-bridge factory. */
	key_ref?: string;
	/** Override base URL for openai-compatible / ollama. Ignored for openai/anthropic. */
	base_url?: string;
}

/** Async resolver for a keychain secret reference. Returns undefined for NotFound. */
export type GetApiKey = (keyRef: string) => Promise<string | undefined>;

export interface SessionSpec {
	sessionId: string;
	providerConfig: ProviderConfig;
	model: string;
	systemPrompt?: string;
	cwd?: string;
	/** Per-session policy. Falls back to AskBeforeWrite if omitted. */
	policy?: PolicyConfig;
}

/** Per-session callbacks injected by the JSON-RPC layer. */
export interface SessionDeps {
	getApiKey: GetApiKey;
	/** Emit an approval/request notification upstream. The handler
	 * (serve.ts::runPrompt) wraps this around its `notify` so the
	 * caller-supplied JSON-RPC writer sees a proper notification frame. */
	notifyApprovalRequest: (req: ApprovalRequestNotification) => void;
}

/**
 * Live session state — one per omw-agent session id.
 *
 * `prompt(text, emit)` runs `agentLoop` with the given user input, threads
 * AgentEvents through `emit` (we translate them to JSON-RPC notifications
 * upstream in serve.ts), and updates the in-session message history with
 * the loop's accumulated AgentMessage[] on completion.
 *
 * Cancellation: `cancel()` aborts the current AbortController. The
 * abort propagates through the streamFn into pi-ai's HTTP client which
 * surfaces it as `stopReason: "aborted"` on the final assistant message.
 * The for-await loop in `prompt` sees `agent_end` and exits cleanly.
 */
export class Session {
	readonly id: string;
	readonly cwd: string | undefined;
	private readonly providerConfig: ProviderConfig;
	private readonly model: Model<any>;
	private readonly systemPrompt: string;
	private readonly getApiKey: GetApiKey;
	private readonly policy: PolicyConfig;
	private readonly notifyApprovalRequest: (req: ApprovalRequestNotification) => void;
	private readonly pendingApprovals: PendingApprovalMap = new Map();
	private readonly messages: AgentMessage[] = [];
	private currentAbort?: AbortController;

	constructor(spec: SessionSpec, deps: SessionDeps) {
		this.id = spec.sessionId;
		this.cwd = spec.cwd;
		this.providerConfig = spec.providerConfig;
		this.systemPrompt = spec.systemPrompt ?? "";
		this.getApiKey = deps.getApiKey;
		this.policy = spec.policy ?? DEFAULT_POLICY;
		this.notifyApprovalRequest = deps.notifyApprovalRequest;
		this.model = buildModel(spec.providerConfig, spec.model);
	}

	async prompt(
		text: string,
		emit: (event: AgentEvent) => void,
	): Promise<{ cancelled: boolean }> {
		if (this.currentAbort) {
			throw new Error(`session ${this.id} is already streaming a prompt`);
		}
		const abort = new AbortController();
		this.currentAbort = abort;

		const beforeToolCall = makeBeforeToolCallHook({
			policy: this.policy,
			pendingApprovals: this.pendingApprovals,
			notifyApprovalRequest: this.notifyApprovalRequest,
		});

		const config: AgentLoopConfig = {
			model: this.model,
			// AgentMessage = Message in our config (no CustomAgentMessages
			// declaration merging) — identity is correct.
			convertToLlm: (msgs) => msgs as Message[],
			getApiKey: async () => {
				const keyRef = this.providerConfig.key_ref;
				if (!keyRef) return undefined;
				return this.getApiKey(keyRef);
			},
			beforeToolCall,
		};

		const userMessage: AgentMessage = {
			role: "user",
			content: text,
			timestamp: Date.now(),
		};

		const context: AgentContext = {
			systemPrompt: this.systemPrompt,
			messages: this.messages,
		};

		const stream = agentLoop([userMessage], context, config, abort.signal);
		try {
			for await (const event of stream) {
				emit(event);
			}
			const finalMessages = await stream.result();
			// The loop already mutates `context.messages` for the streaming
			// AssistantMessage, so we replay the returned slice (prompt +
			// final assistant message + tool results) into our durable list.
			// `context.messages === this.messages` so any prefix is already
			// stored; we capture additions via `finalMessages` only.
			for (const msg of finalMessages) {
				if (!this.messages.includes(msg)) {
					this.messages.push(msg);
				}
			}
			return { cancelled: abort.signal.aborted };
		} finally {
			this.currentAbort = undefined;
		}
	}

	cancel(): void {
		this.currentAbort?.abort();
		// Resolve every in-flight approval as "cancel" so beforeToolCall
		// hooks unblock and the agent loop can finish abort-cleanly.
		for (const [id, resolve] of this.pendingApprovals) {
			this.pendingApprovals.delete(id);
			resolve("cancel");
		}
	}

	/** Resolve a pending approval. Returns true if the approvalId was
	 * known. Called by serve.ts when the GUI replies with approval/decide. */
	applyApprovalDecision(approvalId: string, decision: ApprovalDecision): boolean {
		const resolve = this.pendingApprovals.get(approvalId);
		if (!resolve) return false;
		this.pendingApprovals.delete(approvalId);
		resolve(decision);
		return true;
	}

	/** True while a prompt is in flight. Used by serve.ts to reject a
	 * second `session/prompt` synchronously rather than emit a synthetic
	 * `error` + `turn/finished` after the fact. */
	get isStreaming(): boolean {
		return this.currentAbort !== undefined;
	}

	/** Snapshot of the durable message log (defensive copy). */
	transcript(): AgentMessage[] {
		return [...this.messages];
	}
}

function buildModel(cfg: ProviderConfig, modelId: string): Model<any> {
	switch (cfg.kind) {
		case "openai": {
			// Try the registry; fall back to a hand-built openai-completions Model
			// so unknown ids (e.g. preview models not yet in the generated registry)
			// still work as long as the upstream API accepts them.
			const m = getModel("openai" as never, modelId as never);
			if (m) return m as Model<any>;
			return manualOpenAICompletions("openai", modelId, cfg.base_url ?? "https://api.openai.com/v1");
		}
		case "anthropic": {
			const m = getModel("anthropic" as never, modelId as never);
			if (m) return m as Model<any>;
			throw new Error(`unknown anthropic model: ${modelId} (not in pi-ai registry)`);
		}
		case "openai-compatible": {
			if (!cfg.base_url) {
				throw new Error("openai-compatible provider requires base_url");
			}
			return manualOpenAICompletions("openai-compatible", modelId, cfg.base_url);
		}
		case "ollama": {
			// Ollama exposes an OpenAI-compatible /v1/chat/completions surface;
			// reuse the openai-completions API path rather than build a separate
			// provider in pi-ai.
			const baseUrl = cfg.base_url ?? "http://127.0.0.1:11434/v1";
			return manualOpenAICompletions("ollama", modelId, baseUrl);
		}
	}
}

function manualOpenAICompletions(
	provider: string,
	id: string,
	baseUrl: string,
): Model<"openai-completions"> {
	return {
		id,
		name: id,
		api: "openai-completions",
		provider: provider as never,
		baseUrl,
		reasoning: false,
		input: ["text"],
		// Cost surface is filled in v0.1 via provider_pricing snapshots in
		// omw-cli; the agent kernel itself doesn't price requests. Zeros here
		// keep pi-ai's calculateCost a no-op.
		cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
		contextWindow: 0,
		maxTokens: 4096,
	};
}
