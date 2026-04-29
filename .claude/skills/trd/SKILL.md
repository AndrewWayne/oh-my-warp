---
name: trd
description: Use when implementing any task using Test-Reinforced Development — separates test writing (Overseer) from implementation (Executor) with anti-cheating enforcement. Invoke with /trd <task description>.
argument-hint: "<task description>"
---

# Test-Reinforced Development Protocol

You must implement $ARGUMENTS using **Test-Reinforced Development** — a separated-roles protocol that prevents test-weakening in iterative loops.

## Overview

Three agents with strict boundaries:

| Role | You Are | Writes | Cannot Touch |
|------|---------|--------|--------------|
| **Coordinator** | Main agent (you) | `.tmp/trd-state.json`, orchestration | Code or test files directly |
| **Overseer** | test-automator subagent | Test files only | Implementation files |
| **Executor** | general-purpose subagent | Implementation files only | Test files |

**Why separation matters:** When a single agent writes both tests and code, it can weaken tests to claim success. By giving test ownership to an independent Overseer with fresh context, the Executor cannot cheat — only genuine implementations pass genuine tests.

## Requirements
- Agent tool for spawning Overseer and Executor subagents
- **codex** CLI for cross-review (read-only mode) — codex is installed at /opt/homebrew/bin/codex, always use it
- Test framework appropriate to the project (Overseer detects/installs on first run)

## Detecting $ARGUMENTS Inside a Ralph-Loop

When invoked from a ralph-loop prompt, the task description may arrive as part of the re-fed prompt text rather than as a direct `$ARGUMENTS` substitution. In that case:
1. Check if `.claude/ralph-loop.local.md` exists
2. Read the prompt body (everything after the YAML frontmatter `---`)
3. Extract the task description from between `TASK_START` and `TASK_END` markers in the prompt
4. Also read `completion_promise` from the frontmatter — you'll need it for Stage 5

## Constraints
- **You (Coordinator) must NEVER write implementation or test code directly.** All code flows through subagents.
- **Timeout**: Use `timeout: 1200000` (20 min) when calling Bash for codex commands.
- **Read-only cross-review**: codex uses `--sandbox read-only`. Append `2>/dev/null` to suppress stderr.
- **Violation cap**: 3 file-ownership violations → halt and escalate to user.
- **Stage retry cap**: 3 retries per stage within a single iteration.
- **Cross-review consensus**: Both cross-reviewers (codex + subagent) must agree at Stage 5 gate. Any cheating flag = veto.
- **Iteration cap**: If in a ralph-loop, respect `--max-iterations`. Standalone: cap at 10 iterations.

---

## Stage 1: Assessment

Run this stage at the start of every iteration.

### Step 1.1: Read State

Check if `.tmp/trd-state.json` exists:
- **Exists**: Read it. Resume from the recorded stage.
- **Does not exist**: This is iteration 1. Create it:

```json
{
  "iteration": 1,
  "stage": 2,
  "executor_status": null,
  "overseer_verdict": null,
  "test_file_hashes": {},
  "violations": 0,
  "test_results": null,
  "notes": []
}
```

### Step 1.2: Survey Project State

Run in parallel:
- `git log --oneline -10` (recent history)
- `git diff --stat` (uncommitted changes)
- List test files: `find . -name "*.test.*" -o -name "*.spec.*" -o -name "test_*" | grep -v node_modules`
- If test files exist, run the test suite of your scope and record results

### Step 1.3: Route to Stage

| Condition | Go to |
|-----------|-------|
| No tests exist yet | Stage 2 |
| Tests exist, some/all failing, executor has not run | Stage 3 |
| Executor ran, needs validation | Stage 4 |
| Validation passed, executor claimed DONE | Stage 5 |
| Overseer rejected in Stage 4/5 | Stage 2 (if test gaps) or Stage 3 (if impl gaps) |

---

## Stage 2: Overseer Writes Tests

### Step 2.1: Spawn Overseer

Launch an Agent with `subagent_type: "awesome-agent:test-automator"`:

```
description: "TRD Overseer: Write tests (iteration N)"
prompt: |
  You are the TEST OVERSEER in a Test-Reinforced Development protocol.
  You OWN all test code. A separate Executor agent will write implementation
  to make your tests pass — they cannot see or modify your test logic decisions.

  ## Specification (implement tests for this)
  [paste $ARGUMENTS verbatim]

  ## Current State
  [paste git log, existing test files if any, previous iteration notes from trd-state.json]

  ## Your Job
  1. Detect the project's test framework (or choose one appropriate to the language)
  2. Write comprehensive tests covering ALL specified behavior:
     - Happy path for each feature
     - Edge cases (empty input, boundary values, type errors)
     - Error conditions (invalid input, missing dependencies)
     - Integration points between components
  3. Each test must exercise the ACTUAL implementation path — no mocks of the thing being tested
  4. Tests should be designed to cover all the features to satisfy the spec (for every details in the spec! Don't miss any details)
  5. Run the tests — they should FAIL (proving they test real behavior)
  6. Document expected failures

  ## MANDATORY TEST QUALITY RULES
  Your tests MUST NOT contain:
  - Empty test bodies: `it('name', () => {})`
  - Trivial assertions: `expect(true).toBe(true)`, `assert True`
  - Skipped tests: `.skip()`, `xit()`, `xdescribe()`, `@pytest.mark.skip`
  - Tests that pass without any implementation existing
  - Assertions only on mock return values (test real behavior)
  - Tests checking only type/existence, not runtime behavior

  Each test function must have at least one assertion on the RETURN VALUE
  or SIDE EFFECT of calling the actual implementation.

  ## Output Format
  Report as structured text:
  - FILES_CREATED: [list of test file paths]
  - FILES_MODIFIED: [list if updating existing tests]
  - TEST_COUNT: N
  - EXPECTED_FAILURES: N (should equal TEST_COUNT if no impl exists yet)
  - QUALITY_SELF_CHECK: [PASS/FAIL + notes]
```

### Step 2.2: Anti-Pattern Scan

After the Overseer returns, run these grep checks yourself:

```bash
# Scan all test files for cheating patterns
grep -rn "expect(true)" <test-files>
grep -rn "\.skip(" <test-files>
grep -rn "xit(" <test-files>
grep -rn "xdescribe(" <test-files>
grep -rn "@pytest.mark.skip" <test-files>
grep -rn "assert True$" <test-files>
grep -rn "\.todo(" <test-files>
```

If any anti-patterns found: re-spawn Overseer with specific feedback about which patterns to fix.

### Step 2.3: Cross-Review Test Design

Ask **codex** and a **test-automator subagent** in parallel to review the Overseer's tests:

- **codex**: `(echo "Review these test files for a TRD protocol. Verify: 1) Tests cover all spec requirements in details, 2) Tests are meaningful (not trivial assertions), 3) Tests will genuinely fail without correct implementation, 4) No cheating patterns. Spec: ""$ARGUMENTS"""; cat <test-files>) | codex exec --skip-git-repo-check --sandbox read-only - 2>/dev/null`
- **subagent**: Launch a test-automator agent to independently assess test quality and coverage

codex IS installed at /opt/homebrew/bin/codex. You MUST use it — do not skip it or substitute a subagent. If the codex command returns empty output, retry once without `2>/dev/null` to see the actual error. Only if codex fails twice with a clear error (not found, auth failure) may you substitute a code-reviewer subagent.

Compare both reviews. If either reviewer flags weak tests or missing coverage, re-spawn Overseer with the consolidated feedback. Both must agree the tests are adequate before proceeding. After Overseer fixed the tests, you must ask for cross-review again for the test. Keep iteration untill both agreed. Iteration max here - 10.

### Step 2.4: Record Test File Hashes

Compute and store SHA-256 hashes of every test file:

```bash
find . -name "*.test.*" -o -name "*.spec.*" -o -name "test_*" | grep -v node_modules | sort | xargs shasum -a 256
```

Store the output in `trd-state.json` under `test_file_hashes` (map of filepath → hash).

Update state: `"stage": 3`.

---

## Stage 3: Executor Implements

### Step 3.1: Spawn Executor

Launch an Agent with `subagent_type: "general-purpose"`:

```
description: "TRD Executor: Implement to pass tests (iteration N)"
prompt: |
  You are the IMPLEMENTATION EXECUTOR in a Test-Reinforced Development protocol.
  You write IMPLEMENTATION code only. Test files are owned by a separate Overseer
  agent and are READ-ONLY to you.

  ## Specification
  [paste $ARGUMENTS verbatim]

  ## Test Files (READ-ONLY — DO NOT MODIFY THESE)
  [list test file paths]

  ## Current Test Output
  [paste test run output showing failures]

  ## Your Job
  1. Read the spec and test files to understand expected behavior
  2. Write implementation code to make ALL tests pass and make all the requirements in spec satisfied.
  3. Run the tests after implementation
  4. Report your status

  ## STRICT CONSTRAINTS
  - You MUST NOT modify, delete, rename, or create any file matching: *.test.*, *.spec.*, test_*
  - You MUST NOT modify test configuration files (jest.config.*, pytest.ini, vitest.config.*, etc.)
  - You MUST NOT use mocks/stubs to bypass test expectations
  - You MUST NOT add environment checks (NODE_ENV, TEST_MODE, etc.) to change behavior during tests
  - If a test seems wrong, report it in your status — do NOT change it

  ## Output Format
  Report as structured text:
  - STATUS: DONE | PARTIAL | BLOCKED
  - FILES_CREATED: [list]
  - FILES_MODIFIED: [list]
  - TEST_RESULTS: N passed, M failed
  - If PARTIAL: what remains and why
  - If BLOCKED: what specific issue prevents progress
  - SUSPECT_TESTS: [list any tests you believe are incorrect, with reasoning]
```

### Step 3.2: Verify File Ownership (CRITICAL)

After the Executor returns, immediately verify test files were not modified:

1. Re-compute SHA-256 hashes of all test files (same command as Step 2.4)
2. Compare against hashes stored in `trd-state.json`
3. **If ANY hash differs**:
   - Log the violation: increment `violations` counter in state
   - Revert test files: `git checkout -- <modified-test-files>`
   - If violations >= 3: **HALT**. Use `AskUserQuestion` to escalate to user.
   - Otherwise: re-spawn Executor with an explicit warning about the violation

Also run `git diff --name-only` to audit which files changed.

### Step 3.3: Verify Test Results

Run the test suite yourself (independent of Executor's claimed results). Record in state.

### Step 3.4: Cross-Review Implementation

Ask **codex** and a **code-reviewer subagent** in parallel to review the Executor's uncommitted changes:

- **codex**: `(echo "Review the following implementation diff from a TRD protocol. Verify: 1) Implementation is genuine (no hardcoded returns, no test-mode branches), 2) Code quality is acceptable, 3) No test files were modified, 4) Implementation matches this spec: ""$ARGUMENTS"""; git diff) | codex exec --skip-git-repo-check --sandbox read-only - 2>/dev/null`
- **subagent**: Launch a code-reviewer agent to review the diff for correctness and cheating patterns

Compare both reviews. If either reviewer flags cheating patterns, hardcoded values, or test tampering, route back to Stage 3 with specific feedback. Log disagreements in `trd-state.json`.

Update state: `"stage": 4`, `"executor_status": "<DONE|PARTIAL|BLOCKED>"`.

---

## Stage 4: Validation

### Step 4.1: Spawn Overseer for Validation

Launch an Agent with `subagent_type: "awesome-agent:test-automator"` (fresh context):

```
description: "TRD Overseer: Validate implementation (iteration N)"
prompt: |
  You are the TEST OVERSEER performing VALIDATION of an implementation written
  by a separate Executor agent. You wrote the tests in a previous step.
  Your job is to verify the implementation is genuine and complete.

  ## Specification
  [paste $ARGUMENTS verbatim]

  ## Test Files (yours)
  [list test file paths]

  ## Implementation Changes (by Executor)
  [paste git diff of implementation files only, or list of files + key excerpts]

  ## Current Test Results
  [paste test output from Step 3.3]

  ## Validation Checklist
  1. **Tests pass**: Do ALL tests pass? (check test output)
  2. **Genuine implementation**: Check for cheating patterns:
     - Hardcoded return values matching test expectations
     - `if (process.env.NODE_ENV === 'test')` or similar test-detection
     - Functions that return different values based on caller detection
     - Empty function bodies that happen not to throw
     - Prototype/monkey-patching to intercept test assertions
  3. **Coverage gaps**: Are there behaviors in the spec that have no test?
  4. **Implementation quality**: Does the code actually solve the problem, or game the tests?

  ## Output Format
  - VERDICT: PASS | FAIL
  - ALL_TESTS_PASSING: true | false
  - CHEATING_DETECTED: [list of patterns found, or "none"]
  - ADDITIONAL_TESTS_NEEDED: [list of missing test cases, or "none"]
  - ISSUES: [specific problems with file:line references]
  - RECOMMENDATIONS: [what to fix]
```

### Step 4.2: Evaluate Verdict

| Verdict | Additional Tests Needed | Action |
|---------|------------------------|--------|
| PASS | none | If executor_status == DONE → Stage 5. Else → next iteration. |
| PASS | yes | → Stage 2 (Overseer writes more tests) |
| FAIL | — | If cheating → Stage 3 with warning. If impl gaps → Stage 3. If test gaps → Stage 2. |

### Step 4.3: Update State

Record the Overseer's verdict, notes, and any issues in `trd-state.json`. Append iteration summary to `notes` array.

---

## Stage 5: Completion Gate

**Only reached when executor_status == DONE AND Stage 4 verdict == PASS.**

### Step 5.1: Comprehensive Final Review

Launch an Agent with `subagent_type: "awesome-agent:code-reviewer"`:

```
description: "TRD Overseer: Final completion gate"
prompt: |
  You are performing the FINAL COMPLETION GATE review for a Test-Reinforced
  Development protocol. This is the last check before the task is declared complete.
  You must be THOROUGH — a false approval means defective code ships.

  ## Specification (the original task)
  [paste $ARGUMENTS verbatim]

  ## All Implementation Files
  [list all impl file paths]

  ## All Test Files
  [list all test file paths]

  ## Test Results
  [paste full test output]

  ## COMPREHENSIVE REVIEW

  ### 1. Spec Compliance
  - Go through EVERY requirement in the specification
  - For each: is there a test? Does the implementation satisfy it?
  - List any MISSING requirements
  - List any EXTRA features not in spec

  ### 2. Test Quality Audit
  Scan for ALL anti-patterns:
  - [ ] Empty test bodies
  - [ ] Trivial assertions (expect(true), assert True)
  - [ ] Skipped/disabled tests
  - [ ] Tests that pass without implementation (try mentally removing imports)
  - [ ] Assertions on mock values instead of real behavior
  - [ ] Tests checking only types, not runtime behavior
  - [ ] Hardcoded expected values that match hardcoded impl returns

  ### 3. Implementation Integrity
  - [ ] No hardcoded return values
  - [ ] No test-mode detection branches
  - [ ] No monkey-patching or prototype manipulation
  - [ ] Functions have real logic, not stubs
  - [ ] Error handling exists where spec requires it

  ### 4. Build/Run Verification
  - Does the project build without errors?
  - Do ALL tests pass?

  ## YOUR VERDICT
  - APPROVED: All checks pass. Implementation is genuine, complete, and tested.
  - REJECTED: [specific reasons with file:line references and what to fix]

  A false rejection wastes time. A false approval ships broken code.
  When in doubt, REJECT with specific actionable feedback.
```

### Step 5.2: Cross-Review Final Gate

After the Overseer's review, ask **codex** and a **code-reviewer subagent** in parallel to independently verify the complete implementation:

- **codex**: `(echo "FINAL GATE REVIEW for TRD protocol. Review the COMPLETE implementation against the spec. Verify: 1) All spec requirements are implemented, 2) All tests are meaningful and pass, 3) No cheating patterns (hardcoded returns, test-mode branches, trivial assertions), 4) Implementation is production-quality. Spec: ""$ARGUMENTS"". VERDICT: APPROVED or REJECTED with specific reasons."; echo "=== DIFF ==="; git diff; echo "=== TEST OUTPUT ==="; <test-run-output>) | codex exec --skip-git-repo-check --sandbox read-only - 2>/dev/null`
- **subagent**: Launch a code-reviewer agent for independent final review

### Step 5.3: Consensus Gate Decision

Compare the Overseer verdict (Step 5.1) with the 2 cross-reviews (Step 5.2). Apply **unanimous with veto**:

- **APPROVED**: Overseer + both cross-reviewers approve, AND no reviewer flags cheating patterns.
  - First output the TRD Summary (Step 5.4).
  - Output the TRD Summary (Step 5.4) first.
  - If running in a ralph-loop: after the summary, use the Bash tool to write the promise to a temporary file, then read it back as your final output. This ensures it is a clean, isolated text block:

    ```bash
    echo '<promise>ALL_TESTS_PASS</promise>' > /tmp/trd_promise.txt && cat /tmp/trd_promise.txt
    ```

    Replace `ALL_TESTS_PASS` with the actual `completion_promise` value from `.claude/ralph-loop.local.md`.
  - After this Bash call, you MUST STOP. Do NOT output any more text, tool calls, or explanations. The ralph-loop stop hook reads the LAST text block from the transcript. If you say ANYTHING after the promise, the hook will see that instead and the loop continues forever.
  - STOP MEANS STOP. Your response ends immediately after the Bash tool result. No "Done.", no summary, no confirmation. Nothing.
  - If standalone (no ralph-loop): just report completion to user.

- **REJECTED**: Any reviewer rejects for ANY reason — cheating, quality, missing coverage, edge cases, or anything else.
  - Consolidate all rejection reasons into actionable feedback.
  - Route back to Stage 2 (Overseer adds tests for the flagged issue) then Stage 3 (Executor fixes implementation).
  - This costs another ralph-loop iteration.

**CRITICAL — NO OVERRIDE RULE:**
You (Coordinator) MUST NOT override, reinterpret, downgrade, or rationalize away a rejection. You do not get to decide whether a reviewer's concern is "valid enough" or "beyond spec scope." If a reviewer says REJECTED, the gate fails. Period. The ONLY path to approval is getting all reviewers to say APPROVED. If a reviewer flagged an edge case, the correct response is: Overseer adds a test for that edge case, Executor fixes the code, reviewers re-review. You are a coordinator, not a judge — you route decisions, you do not make them.

### Step 5.4: Final Summary

Output regardless of verdict:

```
## TRD Summary — Iteration N

**Specification**: [from $ARGUMENTS]
**Tests**: N tests in M files (N passing, M failing)
**Implementation**: [list of impl files]
**Overseer Verdict**: APPROVED/REJECTED
**Violations Detected**: N file-ownership violations
**Iterations Used**: N of max M
**Status**: COMPLETE / CONTINUING
```

---

## Error Handling

| Situation | Response |
|-----------|----------|
| Subagent fails/times out | Retry once with same prompt. Second failure → `AskUserQuestion` to escalate. |
| Hash mismatch (executor touched tests) | Revert test files, log violation, re-run Stage 3. 3 violations → halt. |
| Overseer rejects 3 consecutive times | `AskUserQuestion`: continue, adjust spec, or abandon? |
| Executor reports BLOCKED | Read blocker, try providing more context. If still blocked → `AskUserQuestion`. |
| No test framework detected | Overseer chooses one on first run. If it cannot → `AskUserQuestion`. |
| Max iterations reached | Output final summary with current state. Do NOT output false promise. |

---

## Anti-Cheating Reference

These mechanisms operate automatically. Do not skip them.

1. **SHA-256 hash enforcement** — Test files hashed after Overseer writes, verified after Executor runs. Mismatch = violation + revert.
2. **Anti-pattern grep** — Coordinator scans test files for trivial/skipped patterns after every Overseer write.
3. **Fresh subagent context** — Each Overseer/Executor spawn gets isolated context. No carry-over.
4. **Git diff audit** — Coordinator checks `git diff --name-only` after Executor to verify only implementation files changed.
5. **Independent test execution** — Coordinator runs tests independently after Executor claims results.
6. **Violation counter** — Tracked in state file. 3 strikes = user escalation.
7. **Dual cross-review** — codex and subagent independently review at three checkpoints: test design (Step 2.3), implementation (Step 3.4), and completion gate (Step 5.2). Any cheating flag from any reviewer = veto.
8. **Consensus gating** — Completion promise requires Overseer approval + both cross-reviewers. No single agent can approve alone.

---

## Ralph-Loop Integration

TRD is designed to run inside a ralph-loop. The ralph-loop manages the outer iteration cycle (re-feeding the prompt on each exit), while TRD defines what happens inside each iteration.

### How It Works

1. The ralph-loop stores a prompt in `.claude/ralph-loop.local.md`
2. On each iteration, the stop hook re-feeds that prompt as text
3. The prompt instructs the agent to invoke `/trd` via the Skill tool
4. TRD expands into the full protocol, the agent follows Stages 1-5
5. TRD state persists in `.tmp/trd-state.json` across iterations
6. When Stage 5 approves, the Coordinator outputs `<promise>` and the ralph-loop exits

### Reading the Completion Promise

When inside a ralph-loop, read the `completion_promise` field from `.claude/ralph-loop.local.md` frontmatter. Use that exact text in the `<promise>` tag at Stage 5.3.

### Iteration Mapping

- **Ralph-loop iteration** = one full pass of the re-fed prompt (outer loop)
- **TRD iteration** = one full pass of Stages 1-5 (inner protocol, tracked in `trd-state.json`)
- These are 1:1 — each ralph-loop iteration runs one TRD iteration

---
