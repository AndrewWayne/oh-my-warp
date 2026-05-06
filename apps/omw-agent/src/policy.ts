// Bash-command classifier — TypeScript port of omw-policy.
//
// Mirrors crates/omw-policy/src/lib.rs decision-for-decision so the GUI's
// PolicyConfig serialises identically across the JSON-RPC + HTTP surfaces.
//
// File-boundary note: tests live in test/policy.test.ts and are owned by
// the Test Overseer under the TRD protocol.

export type Decision = "allow" | "ask" | "deny";

export type ApprovalMode = "read_only" | "ask_before_write" | "trusted";

export interface PolicyConfig {
	mode: ApprovalMode;
	/** Heads that always classify as "allow" regardless of mode. */
	allow?: string[];
	/** Heads that always classify as "deny". */
	deny?: string[];
}

export const DEFAULT_POLICY: PolicyConfig = { mode: "ask_before_write" };

const READ_ONLY_HEADS = new Set([
	"pwd",
	"date",
	"ls",
	"rg",
	"cat",
	"head",
	"tail",
	"wc",
	"echo",
	"true",
	"false",
	"uptime",
	"whoami",
	"hostname",
	"uname",
	"tty",
	"id",
	"groups",
	"which",
	"type",
	"env",
]);

const GIT_READ_ONLY_SUBCOMMANDS = new Set([
	"status",
	"diff",
	"log",
	"show",
	"branch",
	"remote",
	"config",
	"blame",
]);

/**
 * Classify a bash-style command string against `cfg`.
 *
 * Decision algorithm (parallel to omw-policy::classify):
 *   1. Empty command -> deny.
 *   2. Per-config deny list (head match) -> deny outright.
 *   3. Per-config allow list (head match) -> allow outright.
 *   4. Mode "trusted" -> allow.
 *   5. Shell metacharacters force the command out of the read-only fast
 *      path. Same set as the Rust side: > | ; ` && || $().
 *   6. Built-in read-only set (single-token + git sub-commands) -> allow.
 *   7. Otherwise: deny under read_only, ask under ask_before_write.
 */
export function classify(cmd: string, cfg: PolicyConfig = DEFAULT_POLICY): Decision {
	const trimmed = cmd.trim();
	if (trimmed.length === 0) {
		return "deny";
	}

	const head = headToken(trimmed).toLowerCase();
	const denyList = (cfg.deny ?? []).map((s) => s.toLowerCase());
	const allowList = (cfg.allow ?? []).map((s) => s.toLowerCase());

	if (denyList.includes(head)) return "deny";
	if (allowList.includes(head)) return "allow";

	if (cfg.mode === "trusted") return "allow";

	const readOnly = !hasMetacharacters(trimmed) && isReadOnly(head, trimmed);
	if (readOnly) return "allow";

	return cfg.mode === "read_only" ? "deny" : "ask";
}

function headToken(cmd: string): string {
	const m = cmd.match(/^\s*(\S+)/);
	return m ? m[1] : "";
}

function hasMetacharacters(cmd: string): boolean {
	for (let i = 0; i < cmd.length; i++) {
		const c = cmd[i];
		if (c === ">" || c === "|" || c === ";" || c === "`") return true;
	}
	if (cmd.includes("$(")) return true;
	if (cmd.includes("&&") || cmd.includes("||")) return true;
	return false;
}

function isReadOnly(head: string, fullCmd: string): boolean {
	if (READ_ONLY_HEADS.has(head)) return true;
	if (head === "git") {
		const tokens = fullCmd.split(/\s+/);
		const sub = tokens[1];
		if (sub) return GIT_READ_ONLY_SUBCOMMANDS.has(sub);
		return false;
	}
	return false;
}
