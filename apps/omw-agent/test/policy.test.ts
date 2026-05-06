// Phase 4c2 — vitest unit tests for the TS classifier mirror.
//
// File-boundary note: tests in this file are owned by the Test Overseer
// under the TRD protocol. Implementation lives in src/policy.ts and is
// authored by the Executor; this file may NOT be edited from the impl
// side.

import { describe, expect, it } from "vitest";

import { classify, type PolicyConfig } from "../src/policy.js";

const askDefault: PolicyConfig = { mode: "ask_before_write" };
const readOnly: PolicyConfig = { mode: "read_only" };
const trusted: PolicyConfig = { mode: "trusted" };

describe("classify (parallel with omw-policy)", () => {
	it("empty command is denied", () => {
		expect(classify("", askDefault)).toBe("deny");
		expect(classify("   ", askDefault)).toBe("deny");
	});

	it("read-only singletons allow under default", () => {
		for (const cmd of ["pwd", "date", "ls", "ls -la", "rg foo", "cat README.md"]) {
			expect(classify(cmd, askDefault)).toBe("allow");
		}
	});

	it("destructive singletons ask under default", () => {
		for (const cmd of ["rm -rf /", "mv a b", "chmod 0777 file", "kill 123"]) {
			expect(classify(cmd, askDefault)).toBe("ask");
		}
	});

	it("unknown command asks under default", () => {
		expect(classify("frobnitz --do-something", askDefault)).toBe("ask");
	});

	it("git status / diff allow; git push asks", () => {
		expect(classify("git status", askDefault)).toBe("allow");
		expect(classify("git diff", askDefault)).toBe("allow");
		expect(classify("git log --oneline", askDefault)).toBe("allow");
		expect(classify("git push origin main", askDefault)).toBe("ask");
		expect(classify("git reset --hard HEAD", askDefault)).toBe("ask");
	});

	it("shell metacharacters force ask even for safe heads", () => {
		expect(classify("ls > out.txt", askDefault)).toBe("ask");
		expect(classify("cat foo | sh", askDefault)).toBe("ask");
		expect(classify("echo hi; rm bar", askDefault)).toBe("ask");
		expect(classify("true && rm -rf /", askDefault)).toBe("ask");
		expect(classify("date `evilcmd`", askDefault)).toBe("ask");
		expect(classify("date $(evilcmd)", askDefault)).toBe("ask");
	});

	it("read_only mode denies anything not on the safe list", () => {
		expect(classify("pwd", readOnly)).toBe("allow");
		expect(classify("git status", readOnly)).toBe("allow");
		expect(classify("rm foo", readOnly)).toBe("deny");
		expect(classify("git push", readOnly)).toBe("deny");
		expect(classify("ls > out", readOnly)).toBe("deny");
		expect(classify("frobnitz", readOnly)).toBe("deny");
	});

	it("trusted mode always allows", () => {
		expect(classify("pwd", trusted)).toBe("allow");
		expect(classify("rm -rf /", trusted)).toBe("allow");
		expect(classify("git push --force", trusted)).toBe("allow");
	});

	it("config deny list overrides safe classification", () => {
		const cfg: PolicyConfig = { mode: "ask_before_write", deny: ["pwd"] };
		expect(classify("pwd", cfg)).toBe("deny");
		expect(classify("ls", cfg)).toBe("allow");
	});

	it("config allow list overrides destructive classification", () => {
		const cfg: PolicyConfig = { mode: "ask_before_write", allow: ["rm"] };
		expect(classify("rm foo", cfg)).toBe("allow");
		expect(classify("frobnitz", cfg)).toBe("ask");
	});

	it("deny list wins over allow list (fail closed)", () => {
		const cfg: PolicyConfig = {
			mode: "ask_before_write",
			allow: ["rm"],
			deny: ["rm"],
		};
		expect(classify("rm foo", cfg)).toBe("deny");
	});

	it("case-insensitive head matching", () => {
		expect(classify("LS", askDefault)).toBe("allow");
		expect(classify("Ls -la", askDefault)).toBe("allow");
	});
});
