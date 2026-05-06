// `omw-agent --serve-stdio` — line-delimited JSON-RPC 2.0 server over stdio.
//
// Exposes the pi-agent kernel to omw-server (Phase 2). One Node process per
// omw-server; sessions are multiplexed by sessionId.
//
// Frame protocol: each line of stdin is one full JSON-RPC request; each line
// of stdout is one response or one notification. Newline framing chosen for
// debuggability via `tee` and to avoid Node's default block-buffered stdout.
//
// Methods (request/response, ids round-trip):
//   - session/create   { providerConfig, model, systemPrompt?, cwd?, sessionId? }
//                      -> { sessionId }
//   - session/prompt   { sessionId, prompt }     -> { ok: true }
//   - session/cancel   { sessionId }             -> { ok: true }
//   - approval/decide  { approvalId, decision }  -> { ok: true }   (Phase 5 stub)
//
// Notifications (no id, server -> client):
//   - assistant/delta      { sessionId, delta }
//   - tool/call_started    { sessionId, toolCallId, toolName, args }
//   - tool/call_finished   { sessionId, toolCallId, toolName, isError }
//   - turn/finished        { sessionId, cancelled }
//   - approval/request     { sessionId, approvalId, toolCall }   (Phase 5)
//   - error                { sessionId?, message }
//
// Threat-model invariants:
// - I-1: API keys NEVER appear in any frame. They flow only from the
//   keychain bridge into pi-ai's stream layer; emitted events carry text
//   deltas, tool call metadata, and ids — never headers or auth values.

import { randomUUID } from "node:crypto";
import { createInterface } from "node:readline";

import type { AgentEvent } from "../vendor/pi-agent-core/index.js";

import type { ApprovalDecision } from "./policy-hook.js";
import type { PolicyConfig } from "./policy.js";
import { Session, type GetApiKey, type ProviderConfig, type SessionSpec } from "./session.js";

export interface RunStdioServerOptions {
	stdin: NodeJS.ReadableStream;
	stdout: NodeJS.WritableStream;
	stderr: NodeJS.WritableStream;
	getApiKey: GetApiKey;
}

interface JsonRpcRequest {
	jsonrpc: "2.0";
	id?: string | number | null;
	method: string;
	params?: unknown;
}

interface JsonRpcResult {
	jsonrpc: "2.0";
	id: string | number | null;
	result: unknown;
}

interface JsonRpcError {
	jsonrpc: "2.0";
	id: string | number | null;
	error: { code: number; message: string; data?: unknown };
}

interface JsonRpcNotification {
	jsonrpc: "2.0";
	method: string;
	params: unknown;
}

type JsonRpcFrame = JsonRpcResult | JsonRpcError | JsonRpcNotification;

/**
 * Run the JSON-RPC stdio server. Returns when stdin closes.
 *
 * In production stdio is process.stdin/stdout/stderr; in tests we pipe
 * arbitrary streams in.
 */
export async function runStdioServer(opts: RunStdioServerOptions): Promise<void> {
	const sessions = new Map<string, Session>();

	const writeFrame = (frame: JsonRpcFrame): void => {
		// JSON.stringify + "\n" is sufficient framing because no JSON value
		// emits an unescaped raw newline at the top level. All embedded
		// newlines in string fields are encoded as `\n`.
		opts.stdout.write(`${JSON.stringify(frame)}\n`);
	};

	const notify = (method: string, params: unknown): void => {
		writeFrame({ jsonrpc: "2.0", method, params });
	};

	const reply = (id: string | number | null, result: unknown): void => {
		writeFrame({ jsonrpc: "2.0", id, result });
	};

	const replyError = (
		id: string | number | null,
		code: number,
		message: string,
	): void => {
		writeFrame({ jsonrpc: "2.0", id, error: { code, message } });
	};

	const handle = async (req: JsonRpcRequest): Promise<void> => {
		const id = req.id ?? null;
		try {
			switch (req.method) {
				case "session/create":
					return handleSessionCreate(req, id, sessions, opts.getApiKey, notify, reply, replyError);
				case "session/prompt":
					return handleSessionPrompt(req, id, sessions, reply, replyError, notify);
				case "session/cancel":
					return handleSessionCancel(req, id, sessions, reply, replyError);
				case "approval/decide":
					return handleApprovalDecide(req, id, sessions, reply, replyError);
				default:
					return replyError(id, -32601, `unknown method: ${req.method}`);
			}
		} catch (e) {
			replyError(id, -32000, errMessage(e));
		}
	};

	const rl = createInterface({ input: opts.stdin, crlfDelay: Infinity });
	for await (const line of rl) {
		if (!line.trim()) continue;
		let req: JsonRpcRequest;
		try {
			req = JSON.parse(line) as JsonRpcRequest;
		} catch (e) {
			replyError(null, -32700, `parse error: ${errMessage(e)}`);
			continue;
		}
		if (req.jsonrpc !== "2.0" || typeof req.method !== "string") {
			replyError(req.id ?? null, -32600, "invalid request");
			continue;
		}
		// Don't await here — `session/prompt` runs the agent loop in the
		// background and emits notifications as it streams. Sequencing is
		// handled per-session by `Session.prompt` (it rejects concurrent
		// prompts on the same session).
		void handle(req);
	}
}

function handleSessionCreate(
	req: JsonRpcRequest,
	id: string | number | null,
	sessions: Map<string, Session>,
	getApiKey: GetApiKey,
	notify: (method: string, params: unknown) => void,
	reply: (id: string | number | null, result: unknown) => void,
	replyError: (id: string | number | null, code: number, message: string) => void,
): void {
	const params = (req.params ?? {}) as Record<string, unknown>;
	const providerConfig = parseProviderConfig(params.providerConfig);
	if (!providerConfig) {
		return replyError(id, -32602, "session/create requires { providerConfig: { kind, ... } }");
	}
	const model = params.model;
	if (typeof model !== "string" || model.length === 0) {
		return replyError(id, -32602, "session/create requires { model: string }");
	}
	const sessionId =
		typeof params.sessionId === "string" && params.sessionId.length > 0
			? params.sessionId
			: randomUUID();
	const cwd = typeof params.cwd === "string" ? params.cwd : undefined;
	const systemPrompt = typeof params.systemPrompt === "string" ? params.systemPrompt : undefined;
	const policy = parsePolicy(params.policy);

	if (sessions.has(sessionId)) {
		return replyError(id, -32602, `sessionId already exists: ${sessionId}`);
	}
	const spec: SessionSpec = { sessionId, providerConfig, model, cwd, systemPrompt, policy };
	let session: Session;
	try {
		session = new Session(spec, {
			getApiKey,
			notifyApprovalRequest: ({ approvalId, toolCall }) => {
				notify("approval/request", { sessionId, approvalId, toolCall });
			},
		});
	} catch (e) {
		return replyError(id, -32602, errMessage(e));
	}
	sessions.set(sessionId, session);
	reply(id, { sessionId });
}

function handleApprovalDecide(
	req: JsonRpcRequest,
	id: string | number | null,
	sessions: Map<string, Session>,
	reply: (id: string | number | null, result: unknown) => void,
	replyError: (id: string | number | null, code: number, message: string) => void,
): void {
	const params = (req.params ?? {}) as Record<string, unknown>;
	const sessionId = params.sessionId;
	const approvalId = params.approvalId;
	const decision = params.decision;
	if (
		typeof sessionId !== "string" ||
		typeof approvalId !== "string" ||
		(decision !== "approve" && decision !== "reject" && decision !== "cancel")
	) {
		return replyError(
			id,
			-32602,
			"approval/decide requires { sessionId, approvalId, decision: 'approve' | 'reject' | 'cancel' }",
		);
	}
	const session = sessions.get(sessionId);
	if (!session) {
		return replyError(id, -32602, `unknown sessionId: ${sessionId}`);
	}
	const matched = session.applyApprovalDecision(approvalId, decision as ApprovalDecision);
	reply(id, { matched });
}

function handleSessionPrompt(
	req: JsonRpcRequest,
	id: string | number | null,
	sessions: Map<string, Session>,
	reply: (id: string | number | null, result: unknown) => void,
	replyError: (id: string | number | null, code: number, message: string) => void,
	notify: (method: string, params: unknown) => void,
): void {
	const params = (req.params ?? {}) as Record<string, unknown>;
	const sessionId = params.sessionId;
	const prompt = params.prompt;
	if (typeof sessionId !== "string" || typeof prompt !== "string") {
		return replyError(id, -32602, "session/prompt requires { sessionId, prompt }");
	}
	const session = sessions.get(sessionId);
	if (!session) {
		return replyError(id, -32602, `unknown sessionId: ${sessionId}`);
	}
	// Reject a second prompt while the first is still streaming, BEFORE
	// the OK response. Otherwise Session.prompt throws asynchronously
	// and we'd emit a synthetic `error` + `turn/finished` for the active
	// turn — clients then mis-attribute later deltas to a new turn.
	if (session.isStreaming) {
		return replyError(
			id,
			-32000,
			`session ${sessionId} is already streaming a prompt; cancel first`,
		);
	}
	reply(id, { ok: true });
	void runPrompt(session, prompt, notify);
}

async function runPrompt(
	session: Session,
	prompt: string,
	notify: (method: string, params: unknown) => void,
): Promise<void> {
	try {
		const { cancelled } = await session.prompt(prompt, (event) => {
			translateEvent(session.id, event, notify);
		});
		notify("turn/finished", { sessionId: session.id, cancelled });
	} catch (e) {
		notify("error", { sessionId: session.id, message: errMessage(e) });
		notify("turn/finished", { sessionId: session.id, cancelled: false });
	}
}

function handleSessionCancel(
	req: JsonRpcRequest,
	id: string | number | null,
	sessions: Map<string, Session>,
	reply: (id: string | number | null, result: unknown) => void,
	replyError: (id: string | number | null, code: number, message: string) => void,
): void {
	const params = (req.params ?? {}) as Record<string, unknown>;
	const sessionId = params.sessionId;
	if (typeof sessionId !== "string") {
		return replyError(id, -32602, "session/cancel requires { sessionId }");
	}
	const session = sessions.get(sessionId);
	if (!session) {
		return replyError(id, -32602, `unknown sessionId: ${sessionId}`);
	}
	session.cancel();
	reply(id, { ok: true });
}

function translateEvent(
	sessionId: string,
	event: AgentEvent,
	notify: (method: string, params: unknown) => void,
): void {
	switch (event.type) {
		case "message_update": {
			const ev = event.assistantMessageEvent;
			if (ev.type === "text_delta") {
				notify("assistant/delta", { sessionId, delta: ev.delta });
			}
			break;
		}
		case "tool_execution_start":
			notify("tool/call_started", {
				sessionId,
				toolCallId: event.toolCallId,
				toolName: event.toolName,
				args: event.args,
			});
			break;
		case "tool_execution_end":
			notify("tool/call_finished", {
				sessionId,
				toolCallId: event.toolCallId,
				toolName: event.toolName,
				isError: event.isError,
			});
			break;
		// Other events (agent_start, agent_end, turn_start, turn_end,
		// message_start, message_end, tool_execution_update) are not
		// surfaced as notifications in v0.1. The synthetic `turn/finished`
		// notification signals completion to the client.
	}
}

function parseProviderConfig(raw: unknown): ProviderConfig | null {
	if (!raw || typeof raw !== "object") return null;
	const obj = raw as Record<string, unknown>;
	const kind = obj.kind;
	if (
		kind !== "openai" &&
		kind !== "anthropic" &&
		kind !== "openai-compatible" &&
		kind !== "ollama"
	) {
		return null;
	}
	const cfg: ProviderConfig = { kind };
	if (typeof obj.key_ref === "string") cfg.key_ref = obj.key_ref;
	if (typeof obj.base_url === "string") cfg.base_url = obj.base_url;
	return cfg;
}

function parsePolicy(raw: unknown): PolicyConfig | undefined {
	if (!raw || typeof raw !== "object") return undefined;
	const obj = raw as Record<string, unknown>;
	const mode = obj.mode;
	if (mode !== "read_only" && mode !== "ask_before_write" && mode !== "trusted") {
		return undefined;
	}
	const allow = Array.isArray(obj.allow)
		? obj.allow.filter((s): s is string => typeof s === "string")
		: undefined;
	const deny = Array.isArray(obj.deny)
		? obj.deny.filter((s): s is string => typeof s === "string")
		: undefined;
	return { mode, allow, deny };
}

function errMessage(e: unknown): string {
	if (e instanceof Error) return e.message;
	return String(e);
}
