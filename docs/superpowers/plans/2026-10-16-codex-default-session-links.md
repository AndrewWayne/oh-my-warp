# Default Codex Session Links

## Goal And Boundaries

Keep ordinary `codex` and `codex --yolo` launch commands while associating each
terminal with its displayed Codex thread and actual working directory. Accept
resume, thread switching, simultaneous panes, linked and managed worktrees, then
produce a revised local 0.0.14 Mac distribution retaining the existing features.

Worktree: `worktrees/macos27-maintenance`, branch `fix/macos27-maintenance`, base
`48b181c`. Preserve the pre-existing untracked `target` symlink. Do not install or
restart the production application, change prompts, force `-C`, disable the shared
daemon, alter Windows runtime, or replace public 0.0.14 assets/tag. Use separately
identified local package names with app version 0.0.14. Current revision is
`default-links-r2`; retain the original `default-links-r1` package and records.

The user authorized native session-ID titles after their effect on other
terminals was explained. On 2026-10-16 the verified packaged CLI enabled only
`tui.terminal_title` in `/Users/shuokong/.codex-cli/config.toml`, with a byte-exact
private backup and structured verification that unrelated settings and symlinks
were preserved. `.tmp/default-links-user-config-setup.json` records the readback.
Existing Codex clients were not restarted. Production application replacement
remains outside this action.

## Reproduction And Decisions

The production scanner resolves every cell of the reported reference
`02_concepts/服务器运维与AI工具入门讲义.md:1946`, at 160 and 24 columns, when
given the actual AI_Learn directory. Its ordinary shared-daemon TUI owns no
rollout, so the original resolver returns unknown. Process CWD, the globally
latest thread and file existence cannot establish the active thread.

Codex's native `tui.terminal_title` emits the TUI-selected `session-id`, usually
29 UUID characters followed by `...`. Require a unique full UUID match. Preserve
and hide only the metadata prefix in oh my warp, retaining custom visible titles.

Rejected: injecting `-c tui.terminal_title=...` through a PATH adapter. Codex
0.162.1 excludes that override from shared-daemon launches. Earlier injected-arg
passes only exercised embedded mode. The adapter, bootstrap edits and hidden
launcher have been removed, rather than shipped. Native title config in a private
CODEX_HOME was verified with `/status` showing `Local background server`.

Rejected: resolving the native thread ID through a newly read SQLite config.
The running daemon may retain another startup-captured database path; persisted
cwd may also lag runtime state. No new shared-daemon SQLite lookup remains.

Selected: macOS `proc_pidfdinfo` finds the Unix peer actually connected by the
pane's unique native Codex descendant. A small C helper uses SDK layouts. The
existing file-link worker connects through the existing WebSocket library and
calls only `initialize`, `thread/loaded/list` (maximum 128), and `thread/read`
with `includeTurns:false`. Read the exact loaded thread's runtime cwd. The entire
connection/query is limited to 250 ms, messages to 1 MiB, and reply frames to 32.
Refuse absent, ambiguous, unloaded or unsupported contexts and recheck the process
connection after the response. No model requests, subscriptions or mutation RPCs
are used. Native startup arguments and global config are untouched by this code.

Keep the process-owned rollout/state resolver for embedded contexts. When the
title identifies a thread, require a writable rollout with the same full UUID or
unique prefix before any no-socket fallback; never use launch -C in that branch.
Resolve a stored symlink alias only by the exact rollout UUID and a canonical
file-identity check, retaining the process-start freshness requirement.
Shared clients whose association is unavailable must not fall back to launch -C.
Title changes reject in-flight scans and clear both highlighted links and the
hover-fragment cache, including user-named panes.

Upstream inspected: OpenAI Codex `rust-v0.162.1`, commit
`092d3acd6bec3e3a14bdc7e7a2810ab628ab759d`. Native API compatibility currently
requires daemon 0.162 or later. Older combinations are not accepted yet. No
upstream code is copied; RPCs and native macOS SDK APIs are consumed directly.

## Status

1. Context source: implemented; standalone tests exercise real Unix peers,
   read-only metadata queries, prefix collision refusal and silent-server timeout.
2. Native matrix passed in `.tmp/default-session-matrix-native-r7.log`: ordinary,
   --yolo, -C, three simultaneous clients, linked resume, newly created/persisted
   managed worktree resume, root/linked interactive switching and current-directory
   resume choice. Embedded ordinary/-C/resume passed in
   `.tmp/default-session-embedded-r3.log`. Setup with an existing native daemon
   and client passed in `.tmp/default-session-enable-r2.log`; relocated/stale
   SQLite counterfactual passed in `.tmp/default-session-sqlite.log`.
3. Final optimized package build: `.tmp/default-session-package-build-r3.log`.
   Seven current resolver/SDK tests and three CLI configuration tests passed.
   Independent review findings were fixed and re-reviewed, including state-only
   fallback, dangling configuration links and canonical rollout aliases.
4. r2 real mouse acceptance passed at 640x800: both rows of the wrapped Chinese
   citation, including `.md:1`, opened ROOT-A. Native same-pane resume opened
   ROOT-B. A continuous foreground switch back to the root cleared the old
   highlight without pointer movement, and its old position did not open a
   stale file. Evidence: `.tmp/default-links-gui/acceptance-r2.json`. QA app and
   server exited; production PIDs 69238/69240 remain alive. r2 fixes real
   cursor-skipped NUL cells missed by r1 normalized replay; four current-library
   sparse/soft-wrap regressions passed, along with the existing 101-reference,
   430-eligible-case and eight-reported-wrap matrix. Independent source review
   found no blocker. r2 mounted verification passed, including source/GUI program
   correspondence, signature, no-cloud, notification and bundled Agent checks.
   Artifact SHA256:
   `13fcf966763806b6bb716bc4b8711fa66231907ab9087ccce8a1ec42f35164eb`.
5. User/native title preference: authorized and enabled; preservation and
   idempotence verified. No mouse scenario remains pending. Do not replace or
   restart the running production app while its terminal sessions remain live.

## Acceptance Contract

Verify native shared-daemon mode, exact directory and correct file copy for
ordinary launch, --yolo, explicit -C, resumed directory choices, interactive
thread switching, linked/managed worktrees and simultaneous panes. Exercise
process exit, stale/ambiguous metadata, context failure and existing embedded
behavior. Do not replace actual native cwd with an expected historical root.

Replay the reported Chinese reference through the current production grid at
wide/narrow widths; verify full suffix, line/column and every highlighted cell.
Use an isolated app to inspect hidden metadata, custom titles, same-position
hover invalidation and actual mouse/open behavior. Keep production sessions alive.

After semantic acceptance, package the normal optimized build. Verify signatures,
mounted bytes/source correspondence, version/revision, bundled dependencies,
no-cloud and preservation of Agent/notifications. Record exact artifact hash,
source hashes, current review and remaining compatibility limits. Keep public
0.0.14 immutable. Current trustworthy evidence must support every completion
claim, including the user's normal launch and full requested deliverable.
