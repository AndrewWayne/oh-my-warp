# Codex Relative File Links Implementation Plan

**Goal:** Let Mac oh my warp recognize Codex's relative file references and
wrapped paths against the correct pane/session directory, without prompt changes.

**Stable references:** `docs/codex-path-environment-experiments-2026-10-10.md`,
`docs/codex-mouse-file-links-diagnosis.md`, source base `6840ec9` plus the existing
four-file link repair. Preserve Windows and other programs' behavior.

**Repository operations:** Continue inline in `worktrees/macos27-maintenance`.
No commit, push, publication, production installation or restart in this task.
User authorized implementation and background testing. Preserve running QA too;
package any new executable separately. No new agents.

**Recovery state:** Read this plan and experiment report first. Existing 13
tests and user-assisted QA acceptance cover the earlier reported absolute path,
not new relative/worktree behavior. New process probes show native Codex CWD
remains its launch directory even with `--cd`; its open rollout identifies the
session reliably in the sampled no-daemon runs.

### Task 1: Correct Codex directory

**Project meaning:** Prevent opening the main repository's existing same-named
file when Codex operates in another directory or worktree.

**Architecture relationship:** The local terminal manager owns the shell PID;
the link view passes it to the existing background scan thread. A Mac-only helper
finds the unique descendant Codex process and its open rollout using libproc,
then parses structured session context. No global session-file search.

**Technical contract:** Add a gated `terminal/omw_codex_file_links.rs` helper and
expose the already stored integration-test PID for Mac `omw_local`. Explicit
`--cd`/`-C` takes precedence unless worktree creation changes the directory.
Read bounded tail JSONL data; use the newest complete `turn_context`, then
`session_meta` only for initial context. Unknown or ambiguous association means
relative candidates are skipped, while absolute candidates continue to work.
Do all process/filesystem queries off the UI thread. Do not log paths/content.

**Dependencies:** Existing local-session and Codex command detection.

**Semantic acceptance:** Duplicate file fixtures must resolve to the Codex
worktree, including `--cd`, explicit directory on resume, current turn versus
saved metadata, and private CODEX_HOME. Unknown context must not use the parent
directory. Background real CLI readback must confirm process-to-rollout binding.

**Recovery checkpoint:** Record red/green tests and real PID resolver results;
verify daemon mode separately before claiming general support.

### Task 2: Codex relative and word-boundary wraps

**Project meaning:** Make displayed relative references and early hyphen hard
wraps clickable through the complete filename and source location.

**Architecture relationship:** Add a Codex-specific grid candidate method;
the view selects it only for active local Mac Codex alternate screens. Retain
legacy candidate scanning and all other programs/platforms' policies.

**Technical contract:** Bound reconstruction before enumeration to eight
physical rows and 8192 bytes; bound candidate count. Join real soft wraps, and
indented hard continuations near the right margin or after a path separator or
hyphen. Preserve `./` and `../`, Chinese wide-character anchors and full ranges.
Separate paths and descriptions/list items must not become a joined target.

**Dependencies:** Correct per-session base from Task 1.

**Semantic acceptance:** Use the real 24/40/80/240-column captured outputs,
Chinese relative files, absolute hyphen breaks, same-name worktree fixtures,
line suffixes, unrelated rows and scan-limit negative tests. Earlier 13 cases
must remain green. Demonstrate new tests fail against old implementation first.

**Recovery checkpoint:** Preserve source patch and current evidence; replay
the complete captured matrix through the refreshed production-feature library.

### Task 3: Build and isolated acceptance artifact

**Project meaning:** Provide a usable independently testable application without
ending the user's running Codex terminals.

**Architecture relationship:** Build `omw_local` production features without
`test-exports`, use the existing app-only fast QA profile overrides, and package
with a new isolated bundle/config/data/keychain identity.

**Technical contract:** Preserve installed and existing QA binary hashes and
processes. Verify loading, strict signature, no-cloud audit and source whitespace.
Update the diagnosis/report and knowledge card/index with accurate status.

**Dependencies:** Tasks 1 and 2 regression and capture acceptance.

**Semantic acceptance:** Automated real CLI/context and grid readback must pass.
Actual GUI mouse/selection/typing acceptance remains a separate user-assisted
step because Accessibility and Screen Recording are unavailable; do not claim
it from a build or parser replay. Do not activate windows in this task.

**Recovery checkpoint:** Record exact tests, binary/source hashes, bundle and
launcher paths, verified capabilities and remaining acceptance gap.

### Verified Implementation Checkpoint

- Source implementation is complete for positively associated local Mac Codex
  sessions. Resumed sessions without an explicit directory read only the fresh
  `threads.cwd` row matching the process's open rollout, using the existing
  SQLite dependency. This covers the interactive current/saved directory choice
  before a new `turn_context` is written. No global "latest thread" lookup.
- 23 integration regressions pass. Red evidence covers the original relative
  and hyphen wraps, filename spaces, misleading directory flags, and the 24-column
  consecutive-path list. The earlier 13 absolute-path tests remain green.
- Final production-feature replay: 31 captured screens, 101 references, all
  4737 path cells have the correct complete target and range. The additional
  11 parent-directory capture controls remain diagnostic counterfactuals.
- 456 synthetic cases: all 430 correct-base cases within the eight-row limit
  pass; 24 wrong-base controls and two paths spanning nine rows are recorded
  separately. Do not report 456 passing positive cases.
- Eight real process-context checks meet expectations: seven resolved-directory
  cases and one unknown-context refusal. Default daemon mode with no explicit
  directory does not expose a reliable per-client rollout association in the
  sampled CLI 0.161.0. It intentionally produces no relative links. Explicit
  `-C` is verified in daemon mode. Do not claim unrestricted daemon support.
- Evidence: `.tmp/codex-relative-final-tests.log`,
  `.tmp/codex-relative-production-build.log`, and
  `.tmp/codex-path-lab-20261010/relative-verification-summary.json`.
- Independent bundle: `.tmp/codex-relative-file-qa/omw-relative-file-QA.app`;
  launcher: `.tmp/run-codex-relative-qa.sh`. No GUI was launched by this task.
  Actual mouse, selection, scrolling and typing acceptance remains pending.
  Production and prior QA executable hashes are unchanged. The prior QA process
  was already absent when checked; do not restart it. Only isolated experiment
  daemons were stopped to release fixture session locks.

### User-Requested GUI Acceptance Checkpoint

On 2026-10-10 the user requested acceptance. The isolated launcher started QA
PID 80059 and terminal-server PID 80065; the bundle was activated successfully.
Recorded source and executable hashes still match. Live context probes resolve
the root tab to `repos/base` and the worktree tab to `worktrees/linked`, despite
both Codex processes retaining the root process CWD. Production PIDs and the
installed executable are unchanged. Accessibility and screen-capture checks
remain false. After receiving the root, complete/wrapped Chinese path, worktree
copy and interaction checklist, the user replied "没有问题". Those cases are
recorded as `user-reported-pass`, with aggregate user confirmation and no
independent screenshot or mouse observation. Both tabs use explicit `-C`.
See the QA `gui-acceptance.json` and `build-record.json`.
