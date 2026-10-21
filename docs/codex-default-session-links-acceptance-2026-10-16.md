# Default Codex Links Acceptance

Current revision: default-links-r2. Native, focused, production-linked replay,
narrow GUI mouse acceptance and same-pane switch invalidation passed.
Mounted r2 package verification passed. Native title setup was authorized
and enabled on 2026-10-16. No production app installation. Public 0.0.14 remains
unchanged. The following r1 history records the earlier acceptance limitations;
the r2 results at the end supersede its narrow/switch pending items.

The original default shared-daemon TUI has no process-owned rollout. The existing
scanner recognizes the complete Chinese reference at 160/24 columns when supplied
the correct directory; the failing boundary is current-thread association.

The implementation uses Codex's native terminal-title thread ID and the Unix
socket actually connected by the pane's native Codex process. Only initialize,
bounded loaded-thread enumeration and metadata-only thread/read are requested.
The latter reads live config_snapshot.cwd, avoiding stale or relocated SQLite
state. Queries have a 250 ms total timeout and 1 MiB message limit. Unique prefix
resolution, process/connection recheck and async title invalidation prevent
cross-thread results. Unknown contexts skip relative references.

No PATH adapter, CLI argument change, prompt change, shared-daemon disabling or
new SQLite source-selection logic remains. Existing embedded process-owned
resolution is retained. Native titles require an opt-in setting. The user
authorized enabling it after the effect on other terminals was explained; actual
configuration preservation checks are recorded below.

Focused tests currently pass for metadata hiding, live-directory RPC identity,
read-only request scope, prefix collisions, real connected-peer discovery,
multiple-peer ambiguity and absent/silent-server refusal. Tests import the actual
resolver module and compile the actual C helper using the macOS SDK.

Real native PTY runs have passed ordinary startup, --yolo, explicit -C, three
simultaneous clients and linked-worktree resume. A private-config `/status` probe
confirmed `Local background server`. Earlier title CLI-override results do not
count: review found that Codex switches those invocations to embedded mode, so
that approach was removed.

The full native matrix passed in `.tmp/default-session-matrix-native-r7.log`:
ordinary startup, --yolo, -C, three simultaneous clients, linked resume, newly
created and persisted managed-worktree resume, interactive switching to root and
back to linked, and native resume choice to use the current directory. Process
exit checks returned no context. The old managed fixture was excluded: native
`/status` agreed with its changed runtime directory.

Embedded ordinary/-C/resume passed in `.tmp/default-session-embedded-r3.log`.
Setup while another native client remained connected passed in
`.tmp/default-session-enable-r2.log`. The relocated SQLite counterfactual retained
a deliberately wrong persisted cwd while metadata resolution and native status
agreed on the actual linked directory (`.tmp/default-session-sqlite.log`).

Seven resolver/SDK regressions passed (`.tmp/default-context-green.log`); three
CLI configuration tests passed (`.tmp/default-cli-green.log`). Review found and
fixed two concrete defects, with failing pre-fix tests: state-only fallback could
use old -C during disconnect, and dangling config symlinks could be replaced.
Fallback now requires a writable matching rollout and never uses launch -C for a
known title. A further native test found canonical/symlink rollout path mismatch;
its bounded exact-ID query must match the same canonical file and a fresh row.
The final independent re-review found no remaining blocking issue.

GUI attempt: an isolated `omw.local.defaultLinksQA` app was prepared with separate
HOME/config/data and native `codex resume` commands, without -C or no-daemon.
The host was locked (frontmost process loginwindow PID 609); its QA windows were
off-screen, capture failed and AX exposed no windows. The QA process was stopped.
No actual hover, range, same-position switch or mouse-open acceptance is claimed
for this revision. These remain pending until the desktop is unlocked.

At 15:15 the desktop became available. The final-source isolated QA bundle
rendered a normal shared-daemon resumed Codex thread. A window-only capture and
AX readback showed the ROOT-A transcript, complete Chinese relative references,
and literal custom `Root QA` / `Worktree QA` titles with no identity prefix shown.
PID-targeted background mouse events did not establish an observable highlight
or file-open result while the app was inactive and its windows moved into system
thumbnails. This does not count as successful mouse acceptance. The QA app was
closed normally; production sessions were untouched. Evidence is under
`.tmp/default-links-gui/` and the window-only temporary screenshots.

Final normal optimized `omw_local` package was built successfully with Rust 1.92.0
and no QA profile overrides (`.tmp/default-session-package-build-r3.log`). The
artifact is `dist/omw-warp-oss-v0.0.14-default-links-r1-aarch64-apple-darwin.dmg`:

- DMG SHA256: `68165c40987a75ae2f755fe0b45b5bd1e67dcbdb4ad9b637ede5dfe793f26272`
- Packaged executable: `6d7e20b49bdb5e92e5d8dd6c776d7aec7503d7a2eb4bec1ac776f25575a022ec`
- Packaged CLI: `abe1154bfe0418bb0aca1b0961943462ae0ac55a2982eda09fe6e1c2b3f21e02`

Mounted read-only acceptance passed in `.tmp/default-links-package-verification.log`:
image checksum, strict deep ad-hoc signature, 0.0.14 version and default-links-r1
revision, arm64/minimum-OS metadata, README/provenance/license payload, all source
hashes, signature-normalized program-byte correspondence, no-cloud audit, setup
dry-run and no QA tool leakage. Notification install/idempotence/detection/five
events/uninstall passed. Bundled Agent streaming/history, keyed and keyless Ollama,
cancellation, overlapping-prompt rejection, three approval decisions, pane routing,
provider errors and secret-free output passed using local controlled fixtures.

Replay probe linked the exact final production dependency `libwarp-c5053f1489999059.rlib`.
It passed 42 capture cases, including 31 correct-directory captures, 101 complete
references and 4737 checked reference cells; 430 eligible synthetic cases passed
within the existing scan bounds. The reported Chinese relative path with `:1946`
passed all eight hard/soft wrap combinations at 24, 40, 80 and 160 columns. This
is scanner/target/range evidence, not actual mouse evidence.
See `.tmp/default-links-replay-summary.json`.

Production application and original public DMG hashes were checked unchanged:
`b2c490d76da489740c560735b23222f817890a9bb6142dc20adaca087af27525` and
`76369ad9c90bcb9b552d71415574471664e4183c3af0222e29506905cf8140d4`.

User-authorized setup completed at 2026-10-16T07:39:38Z using the verified packaged
CLI. The effective configuration is `/Users/shuokong/.codex-cli/config.toml`.
Only `tui.terminal_title` changed to
`["session-id", "app-name", "activity", "thread-name", "project-name"]`.
Structured TOML comparison verified every unrelated option was unchanged; the
configuration symlink/target was preserved. The private backup matched all
original bytes and had mode 0600:
`/Users/shuokong/.codex-cli/config.toml.omw-links-61416-1792136378412723000.bak`.
Repeated setup returned already enabled and did not modify the file. No running
Codex client or production application was restarted. Full secret-free readback
is in `.tmp/default-links-user-config-setup.json`. Package provenance retains its
build-time snapshot, when user configuration had not yet been modified.

At 16:41-16:46 a clean isolated QA HOME used the final application program bytes
with the native shared daemon and ordinary `codex resume` arguments. No tracing
proxy or CLI title override was present. Foreground Command-click opened the
Chinese relative citation in the built-in file editor. The root thread opened
the file containing `Project marker: ROOT-A.`; after selecting the linked
session's native resume-directory choice, the same citation opened the distinct
file containing `Project marker: ROOT-B.`. A window-only screenshot also showed
the complete worktree citation highlighted through `.md:1`. Custom tab titles
continued to hide the native identity. The direct resolver independently returned
the root and linked runtime directories in 13 ms and 7 ms respectively.
Evidence and exact screenshot paths: `.tmp/default-links-gui/acceptance.json`.

The 640x800 logical narrow window rendered the citation across two rows, but a
focus change interrupted its hover test. User keyboard input was observed in the
QA prompt, so foreground testing stopped pending a coordinated short interval.
These attempts are not accepted as narrow hover/open evidence. Similarly,
inactive/background captures, a diagnostic tracing proxy, and earlier mismatched
same-title windows are excluded. The driver now selects the exact AX window ID
and refuses clicks after focus loss.

Subsequent background capture of a native resumed root session at 79 columns was
replayed through the final production library. Both Chinese path targets resolved;
the first reference's 34 cells and complete range were verified. In the actual
640x800 GUI the single-line Chinese label subsequently highlighted through `.md`,
but the wrapped citation's real hover/open remains unaccepted. Some synthetic
moves produced no highlight until a click; runtime debugging did not establish
the root cause, and optimized-out values are not evidence of an agent-detection
defect. No production code was changed during this GUI diagnostic round.

The desktop then locked (frontmost loginwindow PID 609), preventing further AX
window/capture work. The isolated QA process was terminated; its terminal server
and native clients exited. Production application/server PIDs 69238 and 69240
remained alive. Accepted screenshots are retained under
`.tmp/default-links-gui/evidence/`. The DMG and public-release hashes were
rechecked unchanged; the packaged CLI still reported native titles already enabled.

Pending: narrow wrapped hover/open and same-pane interactive thread change with
same-position invalidation. Do not claim whole-feature completion until these
are handled. The new app has not been installed. New Codex invocations read the
enabled native setting; existing clients keep their previously loaded settings.

Upstream inspected: `openai/codex`, tag `rust-v0.162.1`, commit
`092d3acd6bec3e3a14bdc7e7a2810ab628ab759d`. Current shared-daemon API guard is
0.162 or later. Older daemon compatibility is not accepted. No upstream code was
copied. Worktree and recovery details are in the implementation plan.

## default-links-r2, 2026-10-17

The real narrow Codex grid exposed a reproduction gap in r1 replay: cursor
movement skips blank cells, leaving NUL rather than literal spaces. NUL has no
display width, and `trim_start` did not see the continuation indentation. An LLDB
inspection of the isolated QA process established the actual bytes. The original
replay normalized screen rows to spaces and could not reproduce this condition.

The fix normalizes each NUL to one ASCII space only in the existing macOS
`omw_local` Codex wrapped-reference reconstruction. Wide-character spacer cells
remain excluded by `line_to_fragments`; hard-wrap trailing blanks remain trimmed,
while soft-wrap filename spaces remain preserved. Existing eight-row, 8192-byte
and 128-candidate limits are unchanged. No Windows runtime changes were made.

The new integration regression failed against pre-fix production code only for
cursor-skipped blanks; the explicit-space control passed
(`.tmp/codex-sparse-grid-red.log`). Four tests passed against the rebuilt
production library (`.tmp/codex-sparse-grid-r2-green.log`): both indentation
representations, soft-wrapped filenames containing spaces at five widths, and
Chinese character boundaries at seven widths. Every reference cell, including
wide-character spacer columns and `.md:19`, must retain its exact full range.
The normal optimized application build completed in 12m 01s.

The rebuilt production-linked probe also passed 42 capture cases, 101 complete
references / 4737 reference cells, 430 eligible synthetic cases and all eight
reported-path hard/soft-wrap combinations. These are current r2 results in
`.tmp/default-links-r2-replay-summary.json`. Independent review found no blocker;
the requested current-library filename-space/CJK edge checks above passed.

Actual GUI acceptance used the r2 executable in an isolated bundle with private
HOME, data, CODEX_HOME and copied completed native Codex threads. No production
sessions or rollout files were reused for writable testing. The native daemon
and ordinary `codex resume` launch were retained, without a tracing proxy, title
CLI override or forced `-C`. Window 61810 was 640x800 logical points. Accepted:

- Hovering either row highlighted the entire wrapped Chinese citation through
  `.md:1`; Command-click on its first row opened the ROOT-A file.
- Command-click on the second row's `.md:1` opened the same ROOT-A file.
- Native `/resume` in the same terminal, selecting the linked session directory,
  opened the distinct ROOT-B file from its Chinese relative citation.
- A continuous single-process foreground driver established the blue linked
  hover, resumed the root thread without moving the pointer, and verified the
  old hover disappeared. A Command-click at that unchanged position left the
  root terminal visible and did not open the old worktree file. The driver
  checked foreground continuity and the ROOT-A transcript after the click.
- The custom Session Link QA title continued hiding native identity metadata.

Screenshots and the exact accepted/excluded evidence list are in
`.tmp/default-links-gui/acceptance-r2.json`. Earlier multi-process driver attempts
lost focus or used unsuitable HID-source keyboard events; they do not establish
stationary-hover acceptance. One later post-return click did not establish a
file-open result and is excluded. The accepted root-open, linked-open and
continuous stationary-switch runs above are separate evidence. No file contents
were saved by the GUI driver: the base fixture is clean, and linked Markdown
diffs contain only the pre-existing ROOT-B/SUB-B markers.

The QA app had unsaved experimental editor buffers and did not complete normal
quit; only its PID 25505 was terminated. Its server PID 25577 exited. Saved focus
was restored to production PID 69238; production application/server PIDs 69238
and 69240 remained alive. The final package's correspondence to the GUI-tested
executable is checked during mounted verification.

The r2 mounted package verification passed in
`.tmp/default-links-r2-package-verification.log`: DMG checksum, strict deep
ad-hoc signature, arm64 / minimum-macOS-11 metadata, version/revision, complete
README/license/provenance payload, source/program correspondence, no-cloud audit,
setup dry-run, notification install/idempotence/detection/five events/uninstall,
and bundled Agent streaming/history, keyless/keyed Ollama, cancellation,
overlapping-request rejection, approval decisions/routing and secret-free errors.
The mounted program and GUI-tested program have identical signature-normalized
bytes (`.tmp/default-links-r2-gui-package-correspondence.log`). All 12 source
hashes in the packaged provenance match the current source files.

- Artifact: `dist/omw-warp-oss-v0.0.14-default-links-r2-aarch64-apple-darwin.dmg`
- DMG SHA256: `13fcf966763806b6bb716bc4b8711fa66231907ab9087ccce8a1ec42f35164eb`
- Packaged executable: `7a3dcff37627e095c13c47eb107b9a72ec5ff7ce823c7383769cdafaec72d2ae`
- Packaged CLI: `abe1154bfe0418bb0aca1b0961943462ae0ac55a2982eda09fe6e1c2b3f21e02`

Final build readback is `.tmp/default-links-r2-build-record.json`. The native
title dry-run still reports already enabled. Production app/public 0.0.14 hashes
remain the earlier recorded values; production PIDs 69238/69240 remain alive.
The new revision is packaged and accepted, but is not installed or running in
place of the user's live production application. The old r1 artifact is retained.
