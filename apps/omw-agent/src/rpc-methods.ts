// Closed vocabulary of omw's agent JSON-RPC *control-contract* method names
// crossing the omw-agent <-> omw-server boundary. Single source of truth:
// specs/agent-rpc-methods.txt (a golden test asserts parity). Matching Rust
// constants live in crates/omw-server/src/agent/rpc_methods.rs.
//
// Scope: control contract only. Kernel observability events relayed from
// pi-mono (assistant/delta, tool/*, turn/*, error) are the kernel's
// vocabulary and are intentionally excluded — see issue #13. They pass
// through the generic `notify(method: string, …)` helper unchanged.

export const AgentRpcMethod = {
  AGENT_CRASHED: "agent/crashed",
  APPROVAL_DECIDE: "approval/decide",
  APPROVAL_REQUEST: "approval/request",
  BASH_CANCEL: "bash/cancel",
  BASH_DATA: "bash/data",
  BASH_EXEC: "bash/exec",
  BASH_FINISHED: "bash/finished",
  SESSION_CANCEL: "session/cancel",
  SESSION_CREATE: "session/create",
  SESSION_PROMPT: "session/prompt",
} as const;

export type AgentRpcMethod = (typeof AgentRpcMethod)[keyof typeof AgentRpcMethod];

/** All control-contract method names (values of `AgentRpcMethod`). */
export const AGENT_RPC_METHODS: readonly AgentRpcMethod[] =
  Object.values(AgentRpcMethod);
