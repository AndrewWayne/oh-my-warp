# macOS 27 Maintenance Diagnosis

Date: 2026-10-05. Host: macOS 27.0 (26A428), arm64.
Candidate: 0.0.13. Baseline: `omw-local-preview-v0.0.11`,
`24c06811dee6cb6b22d715c69657af909ade8d4d`.

The initial diagnostic pass investigated acceptance failures without changing
production behavior. The user subsequently authorized repairs; the follow-up
now preserves Markdown locations, clears recovered permission errors, repairs
notification links, and completes native notification callbacks. The installed
app remains untouched. Current acceptance evidence is in the
[verification report](macos27-maintenance-verification.md).

## Results

| Symptom | Established cause or result | Status |
| --- | --- | --- |
| Native close/minimize/fullscreen buttons fail on macOS 27 | The old custom window handler forwarded every left mouse-up/drag directly to the terminal content view, including interactions begun on native controls. The maintenance backport preserves AppKit handling for native controls and resize edges while retaining terminal dragging. | Repaired in the existing candidate; foreground window checks passed. |
| Codex Markdown opens but ignores line 12 | Location parsing succeeds, but the MarkdownViewer branch drops both location and code-source information. Raw mode later creates a code source without a start position. | Repaired: positioned links open Raw; preview and Raw retain link metadata. |
| Internal source column is one character too far right | The file-jump callback passes a one-based source column directly to the buffer's zero-based column offset. A link requesting column 3 selected the prefix `Lin`, proving the caret was at column 4. | Repaired at the file-jump boundary; the same readback now selects `Li`, including after Raw/rendered switching. |
| Notification click appears to leave another tab selected | The initial synthetic click did not establish a notification response: a grouped notification first expands, and the helpers did not explicitly set single-click state. The corrected event and AXPress both reached the original callback and restored the issuing tab. | The tested current-session tab case passes; no navigation rewrite is justified by this result. |
| Code-file links appear not to open | The default file and `vscode://` handlers both resolve to installed VS Code. Corrected single-click events and sufficient hover time opened both colon-range and OSC 8 hash-range links from the QA app; the editor showed line 12, column 3. | The VS Code case passes. This does not establish other editors' behavior. |
| Permission error remains after notifications start working | An automatic denial adds a persistent toast. The accepted permission branch does nothing, and successful sending has no callback to clear prior error state. | Repaired: denial deduplicates; accepted authorization clears only permission feedback. |
| Troubleshooting link produces Finder -50 | The target constant is an empty string, passed unchanged to the native URL opener. The baseline contains the same empty constant. | Repaired target: macOS Notifications Settings. |
| Notification response does not finish | The native delegate never calls its completion handler. | Repaired; extracted production method fails before repair and passes all six action/wrapper combinations afterward. |
| Installed agent hook emits no notification | The host's `/usr/bin/python3` launcher was blocked by the unaccepted Xcode license. Setup only checked installation, so it reported success while event dispatch silently failed. | Repaired previously by preferring the installed CLT Python; installed-dispatcher red/green regression and packaged lifecycle checks passed. |

## Markdown Position Trace

`app/src/workspace/view.rs:5740` handles `FileTarget::MarkdownViewer`
through `open_file_notebook`, without forwarding `line_col` or `code_source`.
`open_file_notebook` at line 7190 constructs `FilePane` with `None` as its
code source.

`app/src/notebooks/file/mod.rs:907` switches to Raw by emitting
`ReplaceWithCodePane` with that absent source. In
`app/src/pane_group/mod.rs:4644`, the replacement falls back to
`CodeSource::Link { range_start: None, range_end: None }`. The resulting
caret starts at line 1. The viewer's OpenInEditor action also emits
`line_col: None` at line 883.

These modules match the exact 0.0.11 baseline. The parser repair can
recognize Codex's newer syntax, but recognition alone cannot restore
position information discarded by this viewer path.

The repair preserves the existing `CodeSource` at the Markdown entry point,
uses its start position when creating a Raw view or opening from the preview,
and selects the existing Raw editor for positioned Markdown links. Rendered
Markdown has no exact source-line mapping; unpositioned links retain the
preview preference, and users can still switch between both display modes.

Follow-up precision checking exposed the column conversion defect in
`app/src/code/editor/view.rs`. Only the `ScrollPosition::LineAndColumn`
consumer converts the supplied column to a zero-based buffer offset; file
parsing, external-editor arguments, and stored source metadata remain one-based.
The controlled line-12 fixture selected three characters before this repair
and two afterward, demonstrating the user-visible change without relying
only on a screenshot or the parser tests.

Consumer review identified two LSP producers that used zero-based columns
inside the same `LineAndColumnArg` contract: the normal editor and code-review
definition targets. Both now convert the column at their entry point, as they
already did for the line. A compiled boundary probe extracts their production
conversion expressions and the final scroll expression; five representative
buffer columns survive both routes unchanged. The pre-fix route shifts a
nonzero column left and fails the same probe. A real language-server navigation
session was not separately exercised.

## Notification Trace And Test Correction

The temporary library was injected only into `omw.local.macos27QA`.
It wrapped the native send and response methods, logged controlled data,
and called the original implementations without modifying focus logic.
Two responses carried the same context as their sends:

- `window_id`: 0
- `pane_group_id`: 3777
- `pane_id`: Terminal / 4119
- action: `com.apple.UNNotificationDefaultActionIdentifier`
- `rustWrapper` present; callback on the main thread

Screenshots after activation showed the tab containing the issuing shell
command, rather than the tab opened after it. The first verification used
AXPress. The second used physical mouse events after explicitly clearing
modifier flags, setting the single-click count, and expanding the group.
These findings invalidate the previous conclusion that the tested
notification failed to navigate.

The native response delegate does omit its completion-handler call.
That is a separate contract issue visible in the baseline, but the
observed navigation passed without adding that call. It is not the
established cause of the earlier tab-navigation result.

No cross-window, inactive split-pane, stale notification after restart,
or notification sound claim follows from this test.

## Permission Feedback And Invalid URL

`app/src/workspace/view.rs:12113` requests notification permission. Its
denied branch creates a persistent toast; its accepted branch is `()`.
The sending path at line 13079 reports errors only. Neither path removes
a previous permission error after recovery.

`app/src/terminal/view.rs:695` defines
`NOTIFICATIONS_TROUBLESHOOT_URL` as `""`. Both the workspace toast and the
terminal banner use it. The native `open_url` at
`crates/warpui/src/platform/mac/objc/window.m:999` converts the argument
with `NSURL URLWithString:` and calls `NSWorkspace.openURL` without
validation or error reporting. Both the constant and affected workspace
logic are inherited unchanged from 0.0.11.

The repair gives this toast a stable identifier, removes it on accepted
authorization, and clears only permission-related terminal banners across
the workspace's tabs. Unknown send errors remain visible. The troubleshooting
action now targets Notifications Settings; the general URL opener is unchanged.

## External Editor Evidence

The system default `.rs` handler and the `vscode://` scheme handler both
resolve to `/Applications/Visual Studio Code.app`
(`com.microsoft.VSCode`). The app's existing opener builds the same
start-location protocol URL accepted by VS Code on this host.

The final controlled GUI checks used:

- `.tmp/codex-code-fixture.rs:12:3-20:9`
- an OSC 8 file URL ending in `#L12C3-L20C9`

Each caused a new `Successfully launched VSCode` entry in the QA log,
activated the editor's controlled fixture window, and showed line 12,
column 3 in its status bar. The initial probes were inconclusive because
the mouse helpers lacked explicit click counts, path scanning is
asynchronous, and the screenshot targeted the QA window rather than the
editor. There is no established need to change the VS Code launcher for
this case.

Other editor mappings and generic system-default opening are outside
this result. For an unrecognized editor, the existing generic fallback
does not convey line information; that behavior must be evaluated with
the actual editor selected by the user.

## Boundaries And Remaining Acceptance

The window repair adopts upstream commits recorded in
[the provenance record](upstream/v0.0.13.json). No whole-tree upstream
sync, feature replacement, unrelated production refactor, installation,
push, or publication occurred. The authorized repair follow-up rebuilt the
optimized app and package from the existing 0.0.11 feature baseline.

Real provider inference, actual in-app agent tool execution, physical
phone/Tailscale operation, unexpected network reconnect, notification
sound, installed-app updater replacement, and authorized cross-process
Keychain reads remain acceptance gaps. Their missing external conditions
or authorization are not evidence that those implementations are broken.

The controlled editor fixture tab was closed. The QA app and recovery
process exited normally. Codex was reactivated, and installed app
processes 13611 and 13612 remained running.
