#!/usr/bin/env node
// Mock omw-agent kernel for omw-server integration tests.
//
// Speaks line-delimited JSON-RPC 2.0 on stdio. Implements the same method
// surface as the real `omw-agent --serve-stdio`, but the responses are
// deterministic and don't actually call any provider. The point is to
// exercise omw-server's spawn / bridge / handler logic without the
// pi-agent / pi-ai dependency tree.
//
// Behaviour:
//   - session/create   -> { sessionId: <generated> }
//   - session/prompt   -> { ok: true } then emits two `assistant/delta`
//                          notifications (`Hello`, ` world`) and a final
//                          `turn/finished { cancelled: false }`.
//   - session/cancel   -> { ok: true } then emits `turn/finished
//                          { cancelled: true }` for the cancelled session.
//   - approval/decide  -> { ok: true } (Phase 5 stub passthrough)
//   - bash/data        -> notification, no reply (Phase 5a stub)
//   - bash/finished    -> notification, no reply (Phase 5a stub)
//   - any other method -> JSON-RPC error code -32601 "unknown method"
//
// Test-observability side channel: every inbound JSON-RPC frame is also
// echoed as a `kernel/received` notification scoped to the synthetic
// sessionId `__mock_control__`. Tests subscribe to that bus to assert
// what params the server forwarded for each method.
//
// Used by `crates/omw-server/tests/agent_session.rs`.

import { randomUUID } from "node:crypto";
import { createInterface } from "node:readline";

const send = (frame) => {
	process.stdout.write(`${JSON.stringify(frame)}\n`);
};

const reply = (id, result) => send({ jsonrpc: "2.0", id, result });
const replyErr = (id, code, message) => send({ jsonrpc: "2.0", id, error: { code, message } });
const notify = (method, params) => send({ jsonrpc: "2.0", method, params });

const sessions = new Map(); // sessionId -> { cancelled?: true }
const MOCK_CONTROL_SESSION = "__mock_control__";

const rl = createInterface({ input: process.stdin, crlfDelay: Infinity });
for await (const line of rl) {
	if (!line.trim()) continue;
	let req;
	try {
		req = JSON.parse(line);
	} catch {
		replyErr(null, -32700, "parse error");
		continue;
	}
	const id = req.id ?? null;
	// Test-observability echo: every inbound frame is published on the
	// synthetic mock-control session bus so tests can assert what the
	// server forwarded.
	notify("kernel/received", {
		sessionId: MOCK_CONTROL_SESSION,
		method: req.method,
		params: req.params ?? null,
		hasId: id !== null,
	});
	switch (req.method) {
		case "session/create": {
			const sessionId = (req.params && req.params.sessionId) || randomUUID();
			sessions.set(sessionId, {});
			reply(id, { sessionId });
			break;
		}
		case "session/prompt": {
			const sessionId = req.params?.sessionId;
			if (!sessions.has(sessionId)) {
				replyErr(id, -32602, `unknown sessionId: ${sessionId}`);
				break;
			}
			reply(id, { ok: true });
			// Async cadence so the WS handler observes streaming order.
			(async () => {
				await new Promise((r) => setTimeout(r, 5));
				const session = sessions.get(sessionId);
				if (!session) return;
				notify("assistant/delta", { sessionId, delta: "Hello" });
				await new Promise((r) => setTimeout(r, 5));
				if (session.cancelled) {
					notify("turn/finished", { sessionId, cancelled: true });
					return;
				}
				notify("assistant/delta", { sessionId, delta: " world" });
				await new Promise((r) => setTimeout(r, 5));
				notify("turn/finished", { sessionId, cancelled: !!session.cancelled });
			})();
			break;
		}
		case "session/cancel": {
			const sessionId = req.params?.sessionId;
			if (!sessions.has(sessionId)) {
				replyErr(id, -32602, `unknown sessionId: ${sessionId}`);
				break;
			}
			sessions.get(sessionId).cancelled = true;
			reply(id, { ok: true });
			break;
		}
		case "approval/decide":
			reply(id, { ok: true });
			break;
		case "bash/data":
		case "bash/finished":
			// Notifications — no reply expected. The kernel/received echo
			// above is what tests assert against.
			break;
		default:
			if (id === null) {
				// Unknown notification — silently drop. Real kernels would
				// log; tests just want this to not error out.
				break;
			}
			replyErr(id, -32601, `unknown method: ${req.method}`);
	}
}
