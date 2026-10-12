# Codex Mouse Reporting And Bare File Links

Date: 2026-10-08. Installed application: oh my warp 0.0.13.
Patch base: `6840ec9`; published Mac source: `47ed36e813f08da521a428267ce77856da8c9dc8`.

## Report And Evidence

The user reports that an existing Markdown file with a Chinese filename is
fully visible on one row but has no link hover and does not open with Cmd-click.
The file exists. The installed binary matches the published 0.0.13 build.

A probe linked against the published production library finds the complete
reported path at every visible cell at 120 columns. This rules out ordinary
single-row candidate reconstruction as the cause for this path. The same probe
also finds separate soft-wrap limitations at 40, 60 and 80 columns. The first
repair addressed the unwrapped symptom; the follow-up below covers wrapping.

A background pseudo-terminal running the installed Codex CLI 0.161.0 captures
alternate-screen mode 1049 and mouse modes 1000, 1002, 1003 and 1006 being enabled.
No model request is sent. Existing application logs confirm that the running
sessions are detected as Codex.

`AltScreenElement::mouse_moved` classifies every SGR mouse program as
`TerminalEditor::Yes`. `TerminalView::scan_for_file_path` then skips bare path
scanning for that classification. No file link is highlighted, so Cmd-click
returns without opening a file. OSC 8 links and ordinary web URL detection take
earlier branches and are not affected by this gate.

## Patch Boundary

Reuse the existing active-command detection to allow bare path scanning for
Codex in the Mac `omw_local` build. Other SGR mouse programs retain the existing
editor behavior. Windows and Linux retain their previous policy. The existing
local-session, working-directory and filesystem validation gates remain.

Mouse reporting, selection, file-opening preferences, OSC 8 handling and Codex
configuration are unchanged. No upstream tree update is included.

## Verification Status

Regression tests cover the policy and an actual ANSI-parsed, unwrapped Chinese
Markdown path in SGR mouse mode at 160 and 240 columns, including both cells of
wide characters. Before the fix, both new cases fail at the scan policy and the
six existing link cases pass. After the fix, all eight cases pass.

The Mac application builds with the production `omw_local` feature set and
without `test-exports`. Its embedded release tag remains 0.0.13. This QA build
uses app-only opt-level 0/debug 0 overrides for fast validation; it is not a
replacement public release package. Native dependencies load successfully
(`--help` exits 0), strict deep ad-hoc signature verification passes, and the
existing no-cloud hostname audit passes with zero hits for all eight patterns.

Isolated test bundle: `.tmp/file-link-qa/omw-file-link-QA.app`.
Executable SHA-256: `ae10a4177285ca9934158a84ea2476621dbf3385a214035ba8df293b912f50b0`.
Launch through `sh .tmp/run-file-link-qa.sh` from this worktree so HOME, config,
data and keychain are isolated; do not launch the bundle directly for acceptance.
The bundled Agent JavaScript is identical to the installed release.

Foreground click acceptance is pending: the available screenshot helper lacks
Screen Recording permission, AppleScript lacks Accessibility permission, and
the computer-use RPC service is not configured in this session. No TCC settings
are modified. The installed application and public release assets are unchanged.
The installed executable still has its published SHA-256
`3f44db0b6e25bf67b74106174606f4bc7088f3606ffe0760020f71d7f96cc9e0`.

## User-Assisted Click Preparation

The user authorized preparing an independent window for their own mouse test.
The QA app is running with an isolated `home-click` directory and has been
activated. The `Codex File Link QA` tab starts the installed Codex CLI with a
separate CODEX_HOME, read-only sandbox and no shared daemon. Its authentication
file is a private local copy; the original Codex configuration is unchanged.
The app log confirms active-command detection as Codex, and the new session
records the exact reported path as a standalone user message. An assistant
response has not been verified; the user can click the visible user-message path.
This is the installed Codex CLI, not a simulated mouse-reporting wrapper.

The launcher must run as a detached process (for example with Node's
`spawn(..., { detached: true, stdio: "ignore" })` and `unref()`) if the terminal
that prepares the test is to close while the QA window stays open. Launching
through launchctl or LaunchServices was unsuccessful in this environment;
those temporary launchctl jobs have been removed. The launch script keeps all
app configuration and data overrides in the QA directory.

The production app and its terminal server and Agent processes remain running.
Hover, actual Cmd-click, the resulting opened file, and subsequent typing,
selection and scrolling still await the user's observation. No foreground
acceptance result is inferred from window activation or command detection.

## Follow-Up Range And Wrapping Repair

The user confirmed that the initial QA app opens the file, but reported that
the final `d` is excluded from the blue range and narrow windows only recognize
part of the path. The initial eight tests did not assert the complete range or
exercise current Codex's rendered narrow-window output.

The link-related grid, renderer and alternate-screen code was retained from
`omw-local-preview-v0.0.11` (`24c06811dee6cb6b22d715c69657af909ade8d4d`)
in the source base. The evidence does not establish a macOS-only regression:
the old continuation rules do not cover all current Codex rendering behavior.

Two additional defects were reproduced before repair:

- Hovering the second cell of a Chinese wide character undercounts the range
  by one cell, excluding the final `d`. Both cells now use the base cell anchor.
- Ordinary scanning only collects neighboring soft-wrapped rows. Existing
  hard-line reconstruction expects a directory separator at the line boundary,
  while Codex can split a Chinese filename near its right margin. Reconstruction
  now handles the complete soft-wrapped line and indented hard continuations at
  that margin.

The added reconstruction is restricted to Mac `omw_local` absolute paths and
at most eight physical rows / 8192 bytes. Limits are checked before enumerating
suffix fragments. Relative paths, legacy directory-boundary continuations and
Windows/Linux behavior retain their previous rules. Existing filesystem and
file-opening preference checks are still applied.

A real Codex CLI 0.161.0 PTY capture supplies user and assistant path fixtures at
40, 60, 80 and 160 columns. Its visible rows are replayed into the real terminal
grid with SGR mouse reporting. Every path cell, including both halves of Chinese
characters and the final `d`, resolves to the complete target and range:
162 cells per width, 81 for each message. Soft-wrap and unrelated-line boundary
tests also pass. Final result: 13 passed, 0 failed in
`.tmp/codex-link-boundaries-final-tests.log`. Independent source review completed;
its early performance-limit finding was corrected before final tests and build.

The refreshed QA binary builds with `omw_local`, without `test-exports`, using
the same app-only opt-level 0/debug 0 overrides. Strict deep signature check,
command-line loading and all eight no-cloud hostname patterns pass. The source
patch now includes the previously untracked Codex fixture as well as the code
and tests. First-round executable, patch and build record backups are retained.

Refreshed executable SHA-256:
`a8dd3d0f87e1c950467b65e07b6223b9366cbd540858054f785739352107f88d`.
Source patch SHA-256:
`bd2536018c2826b5fd81e83d077b78f64fb7c83c4e7a4ebff584a448cb8fb2eb`.
Build log: `.tmp/codex-link-boundaries-qa-build.log`.
Build record: `.tmp/file-link-qa/build-record.json`.

The independent QA window was relaunched through the detached launcher with
`home-click-r2` and resumed the isolated Codex session containing both messages.
At preparation its main PID was 15907; logs confirm detection as Codex, and
guarded window activation succeeded. The test keychain is isolated and contains
no API keys, so its secure-storage error does not indicate a production config
change. Actual final-character hover, each narrow-window segment's Cmd-click,
correct file opening, input, selection and scrolling await the user's retest.

Only the independent QA app was restarted. Production main/terminal-server/Agent
PIDs 46135/46137/46150 remain running and the installed binary still matches the
published SHA-256 above. The installed app, its configuration and public release
packages have not been changed by this follow-up.

## User Retest And Relative Path Boundary

On 2026-10-09 the user reported that the refreshed QA links now work. Record
this as user-assisted acceptance of the reported link defects; it is not an
automated screenshot/mouse result or separate confirmation of typing, scrolling
and selection. The installed app and public release packages are still unchanged.

The existing scanner supports ordinary relative paths such as `src/main.rs`,
`./src/main.rs` and `../README.md`, subject to filesystem validation. In Codex's
alternate screen the base is the active local terminal session's recorded
working directory. Ordinary output blocks use the directory recorded for that
block. Neither path reads Codex's workspace root or finds the Git root; an agent
workspace that differs from the terminal's recorded directory is not guaranteed
to resolve correctly. This is a source inspection, not a new relative-path
mouse acceptance test.

The latest full soft-wrap and right-margin hard-wrap enhancements deliberately
accept absolute paths only. Relative paths retain neighboring-row scanning and
the legacy indented directory-separator continuation rule; complete narrow-window
relative-path behavior remains outside the verified enhancement. No relative
path implementation change was made in response to this explanatory question.

## Relative Path Feasibility Assessment

On 2026-10-10 the user asked about implementation effort, practical value and
whether Codex generally emits paths relative to the workspace root. No further
implementation was requested or performed.

Official CLI documentation describes a session working directory: resuming or
forking can choose the current or saved directory, with `--cd` taking precedence.
It does not establish that generated file references always use Git-root-relative
paths or provide a frequency estimate for relative versus absolute references.
Source: <https://learn.chatgpt.com/docs/developer-commands#codex-resume>.
The installed model cache's file-reference guidance for `gpt-6.1-sol` and
`gpt-5.6-sol` prefers clickable Markdown links with absolute local targets.
A short visible label therefore does not establish a relative destination.
This is local instruction evidence, not a statistical survey of user sessions.

Extending wrapped relative-path detection against a known directory can reuse
the current grid reconstruction and filesystem validation and has moderate
implementation scope. Discovering the correct Codex session base is a separate,
more involved problem, especially for `--cd`, resumed sessions, subdirectories,
multiple projects and worktrees. The main correctness risk is an existing but
wrong same-named file in another base directory. Other risks are joining unrelated
output and excessive filesystem work during hover.

Recommended first scope: preserve `./` and `../` semantics, use a known per-session
base, retain local-session and bounded-scan checks, and avoid broad directory
search or automatic selection among ambiguous base directories. This would help
clicking bare paths in code summaries and command output. Root discovery should
only be added with reliable session context and tests covering different roots,
directory overrides, resumed sessions, duplicate files and negative wrap cases.

## Multi-Environment Experiment Follow-Up

On 2026-10-10 the user authorized real Codex tests across different directories
and worktrees. The eight-environment matrix produced 16 unforced answers with
48 correct absolute requested targets, plus two relative-path controls.
However, the actual Codex TUI renders these Markdown file references as relative
visible paths. All 31 completed PTY captures emitted neither OSC 8 destinations
nor OSC 7 directory updates. Raw Markdown targets alone therefore understate
the practical need for relative-path handling in the terminal.

The current QA library resolves all three requested reference files at 80
columns in each of the eight environments when supplied the actual session
directory. For `--cd` and Codex-managed worktrees, supplying the parent terminal
directory instead resolves the same visible references to existing wrong
main-repository files. This is a reproduced resolver risk, not production GUI
mouse acceptance. Session-directory correctness should precede broader
relative-path reconstruction; Git-root discovery alone is insufficient.

The wider sample also reveals remaining narrow-output shapes: Chinese relative
filenames split across rows and long absolute paths hard-wrapped early at hyphens.
These were not covered by the earlier accepted reported-path fixture. No new
application fix or installation was performed in this testing turn.
See [the experiment report](codex-path-environment-experiments-2026-10-10.md)
for the matrix, evidence, exclusions and implementation boundaries.

## Direct Relative-Reference Adaptation

On 2026-10-10 the user chose application-side recognition instead of prompt
changes. The Mac `omw_local` Codex alternate-screen path now reconstructs
relative references, `./`, `../`, Chinese filenames, filename spaces, early
hyphen hard wraps and consecutive path lists. Candidates retain the complete
source-location range and are validated through the existing file resolver.
Other programs, Windows, URL handling and file-opening preferences retain their
existing behavior. No upstream synchronization or release change is included.

The terminal manager exposes its existing PTY PID to the background link worker.
The worker finds a unique native Codex descendant and enumerates its open files
with libproc. Explicit `--cd`/`-C` overrides the launch directory; otherwise the
associated rollout supplies the newest structured context. A resumed TUI can
choose the current directory before another turn updates the rollout, so these
sessions use only the fresh `threads.cwd` row matching that open rollout. SQLite
is opened read-only with a 10 ms busy timeout. The process search, open-file list,
rollout tail and reconstructed candidates are bounded. No session content is
logged and no global session-directory search is performed.

Unknown or ambiguous context skips relative candidates and preserves absolute
paths. The default shared-daemon mode in Codex CLI 0.161.0 does not expose the
client's active rollout in the sampled runs. With no explicit directory, this
mode therefore remains outside verified relative-link support; using its parent
terminal directory would reproduce the wrong-copy bug. Explicit `-C` is verified
in daemon mode. Older Codex state schemas and directory-changing in-TUI workflows
have not been exhaustively verified. This is a bounded adaptation, not a claim
of universal Codex session discovery.

Verification against the final production-feature library:

- 23 integration tests pass, including the earlier 13 cases. New failures were
  reproduced before repairs, including the 24-column list whose preceding full
  row swallowed the Chinese reference start.
- 31 real captured screens contain 101 expected file references. All 4737 cells,
  including wide-character spacers, match the complete target and hover range.
  Eleven additional wrong-parent capture controls document the original risk.
- All 430 correct-base synthetic cases within eight physical rows pass. The
  full 456-case set also includes 24 deliberately wrong-base controls and two
  nine-row paths beyond the preserved scan limit.
- Seven real context cases resolve correctly: root, linked worktree, `--cd`,
  managed-worktree resume, current-directory choice, a new directory on resume,
  and daemon mode with explicit `-C`. An eighth verifies refusal when daemon
  context cannot be associated. Private test daemons were stopped afterward.

Independent QA bundle: `.tmp/codex-relative-file-qa/omw-relative-file-QA.app`.
Use `.tmp/run-codex-relative-qa.sh` to isolate HOME, configuration, application
data and keychain. Its two tabs resume private experiment fixtures for root and
linked-worktree references; no prompt change or new model request is required.
The bundle was initially prepared without opening or activating it. On
2026-10-10 the user requested acceptance and it was started through the isolated
launcher (main PID 80059, terminal-server PID 80065). The exact executable and
all eight recorded source hashes still match the build record. QA-only activation
succeeded, and window metadata reports two on-screen application windows; their
rendered contents have not been visually inspected. Both private Codex sessions
are running. Live directory probes resolve root shell PID 80083 to `repos/base`
and worktree shell PID 80084 to `worktrees/linked`, although both native Codex
processes retain `repos/base` as their process CWD.

Accessibility and screen-capture permission checks are still false. No permission
request or simulated input was issued. The user was given the root click, complete
`.md` hover range, narrow-window Chinese first/middle/final segment clicks,
worktree-copy click and input/selection/scrolling checklist, then replied
"没有问题". Acceptance is recorded as `user-reported-pass` for that checklist,
based on aggregate user confirmation rather than independent screenshots or
mouse observations. The root fixture contains `ROOT-A` and the linked-worktree
copy contains `ROOT-B`, so opening a wrong same-name copy is observable. Both QA
tabs explicitly use `-C`, and the worktree tab launches from the parent root while
targeting the linked copy. The case list and confirmation are recorded in QA
`gui-acceptance.json`; launch and live probe evidence is in `build-record.json`.

The installed executable and the earlier QA binary remain byte-identical to
their previously recorded hashes. Production PIDs 46135/46137/46150 remain
running. No installed app, production configuration or public release asset
was changed. Final evidence is recorded in
`.tmp/codex-path-lab-20261010/relative-verification-summary.json` and the independent
QA `build-record.json`.

QA executable SHA-256:
`6cede7ce8b35367c4d6941f3570991aabaedf6c7ec6515aa161c4159424c7532`.
Source patch SHA-256:
`e7a26801e7555f688a9127021636c35863e5328d8b41b2983b3e079915c188a9`.
The production `omw_local` build excludes `test-exports`; app-only opt-level 0
and debug 0 overrides are retained for fast QA. Strict deep signature verification,
native `--help` loading, incremental-authorship headers and source whitespace
checks pass. The eight forbidden cloud-hostname patterns each have zero hits.
The bundled Agent JavaScript is byte-identical to the installed application.
