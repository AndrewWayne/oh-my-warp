# macOS 27 Maintenance Verification

Archive note (2026-10-07): this report covers the unpublished maintenance-only
candidate. Its DMG is now under `dist/archive/2026-10-07-unpublished/`.
The combined 0.0.13 also includes the four selected improvements; see
[the current verification](macos-four-backports-verification.md).

Date: 2026-10-05. Host: macOS 27.0 (26A428), arm64.
Branch: `fix/macos27-maintenance`.
Feature baseline: `omw-local-preview-v0.0.11`,
`24c06811dee6cb6b22d715c69657af909ade8d4d`.

Readiness: **identified maintenance defects repaired and verified; external
integration acceptance remains incomplete**. The final optimized 0.0.13 app
passed focused foreground checks after the last column-position repair.
The installed app was not replaced or stopped.

Follow-up diagnosis on the same date corrected two earlier inconclusive GUI
results: current-session notification clicks restore the originating tab,
and VS Code opens code links at the requested start position. The authorized
repair follow-up fixes Markdown position loss, the source-column offset,
stale permission feedback, empty notification URLs, and the native response
completion contract. See [the diagnosis](macos27-maintenance-diagnosis.md).
Fresh production GUI and package verification is recorded below.

## Preservation Boundary

Production changes are confined to native window event routing, Codex file
references/Markdown source positioning, notification hook setup/dispatch,
permission feedback and native response completion, and API-key Debug redaction.
Tests, test exports, build support, and release/provenance documents support
those repairs. Agent apps, provider configuration, remote/server/PTY crates,
app dependencies, AI assistant implementation, updater implementation,
omw modules, and all `warp_core/src` remain unchanged from 0.0.11. The
divergent 0.0.12 tree was not merged and the vendor tree was not replaced.

## Automated Acceptance

| Area | Evidence | Scope and limits |
| --- | --- | --- |
| Root Rust workspace | 370 passed, 1 ignored | Before the final dispatcher regression. That added test and the entire 8-test notification suite subsequently passed. The opt-in real-Claude capture test returns early without its flag; it is not real-Claude acceptance. |
| Documentation tests | 1 passed, 1 ignored | Root workspace. |
| Rust quality gates | Release/all-targets Clippy with `-D warnings`, formatting, tracked secret scan passed | Final CLI Clippy covers the new dispatcher regression. |
| CLI notification setup | 8 passed | Custom homes with spaces, legacy/quoted commands, idempotent install/status/uninstall, unrelated-hook preservation, actual installed dispatcher output. |
| Path parser | 59 passed | Codex colon/hash ranges, legacy forms and filenames. |
| App integration suites | 89 passed, 2 ignored | Fresh final-source run after both LSP producer repairs: settings/actions, 6 file-link/source cases, inline prompts, approval, command broker, pane isolation, transcript state, and 14 updater cases. Existing stub-server interaction cases were ignored. Production features plus `test-exports`; test-only app optimization/debug overrides. Run serially because command-broker cases share global state. |
| LSP column boundary | 10 round-trip assertions passed | Compiled production expressions from editor/code-review producers and the scroll consumer preserve five representative zero-based buffer columns. Pre-fix conversion failed. This is a boundary check, not a real language-server GUI session. |
| Native notification response | 6 passed | The extracted production Objective-C method is compiled and executed for default/dismiss/custom actions with and without the Rust wrapper. Before repair, completion count is 0; after repair, every path completes once and only the default action with a wrapper navigates. |
| Vendor terminal library | 85 passed, 2 ignored | Terminal parser/model regression, including existing notification behavior. |
| Agent | 91 passed, 1 skipped | Includes actual bundled helper missing-key/bad-input checks. Linux/Windows backend-unavailable case skipped on macOS. |
| Web Controller | 114 passed, 1 skipped; build passed | Existing skip retained. |
| BYORC client | Typecheck passed | Existing consumer compatibility. |
| Updater | 14 passed | Actual unchanged updater module: semver, release/assets, absent/bad releases, rate limit/server/JSON errors, correct/uppercase/malformed/mismatched/missing SHA. Local HTTP server and temporary package bytes; no installed-app swap. |
| Vendor license check | 31 files passed | AGPL authorship headers retained. |

Some baseline inline-prompt diagnostics pass even when a real HTTP call is
unsuccessful. Those cases are not counted as successful model inference.

Attempting the inherited upstream library's internal test harness exposed
compile-time test API gaps in the stripped fork (`E0599`, 157 errors). It is
not an accepted test suite for this build. App coverage above refers to the
project's integration suites against the compiled production library, invoked
with `--test '*'`; it does not imply all upstream internal tests passed.

The expanded app run exposed a stale settings test: incomplete non-default
drafts are allowed by 0.0.11, and mounting settings bootstraps a config file.
The test now selects the incomplete provider as default, verifies rejection,
and checks unchanged bootstrapped config bytes. A companion test confirms
non-default incomplete drafts are omitted on save. Production settings
behavior was not modified to satisfy the tests.

## Packaged Runtime And Browser

The bundled Node 22 and Agent ran as real subprocesses against a local HTTP
SSE provider with isolated config/HOME and a dummy key helper. Checks passed
for streamed output, retained conversation history, cancellation of an open
stream, overlapping-prompt rejection, approve/reject/cancel decisions,
approval before exec, originating terminal session/cwd/command routing,
mock broker data/finished events, provider 401 errors, and invalid/unknown
JSON-RPC methods/sessions. No dummy key appeared in protocol frames/logs.
This is not a real-provider or actual GUI tool-execution test.

The actual bundled CLI passed repeated installation, custom `CODEX_HOME`
detection, five dispatcher events, and uninstall against temporary homes.
Stop, PermissionRequest and actionable Notification emitted OSC 777;
idle/unknown events emitted nothing. Hook stdout remained empty: no automatic
permission decision was emitted.

This semantic check found a real failure: `/usr/bin/python3` was blocked by
the host's unaccepted Xcode license, so an installed hook silently produced
no notification despite positive setup status. The installed-script test
failed before the fix. Both dispatcher copies now select the executable
Command Line Tools Python when present; all 8 notification tests and the
rebuilt CLI lifecycle check passed without `DEVELOPER_DIR`. System Xcode
settings/license were not changed.

Headless Edge exercised browser pairing, connection, terminal input/Return,
shortcuts, Sessions back/reopen, touch scroll, resize, simulated keyboard and
iOS rubber-band behavior, and the 375px mobile layout. Screenshots were
inspected and showed usable nonblank terminal content without overflow.
Two host connections and no tiny terminal resize were asserted. This does
not exercise an unexpected network disconnect/reconnect, a real iPhone,
or a Tailscale path. Server reconnect regression tests passed separately.

## Foreground Acceptance

The user explicitly authorized foreground testing after the earlier
background-only phase. Tests used `omw.local.macos27QA`, temporary HOME,
omw config/data and app state, memory key storage, and disabled real agent
providers. GUI events checked the QA identity/frontmost app. Clipboard
contents were restored after paste; the existing Chinese input source was
used without changing input-source settings. The QA app exited normally.

| Interaction | Result |
| --- | --- |
| Red close button | Closed a newly opened QA window; the original QA window remained. |
| Yellow minimize/restore | Window collapsed from 1710 x 1006 to a 159 x 179 thumbnail and restored. AXMinimized incorrectly remained false, so that accessibility property is not accepted. |
| Green fullscreen / Fn+F | Physical click entered 1710 x 1073 fullscreen; Fn+F returned to windowed bounds. Earlier 1280 x 800 restore also observed. |
| Option-green | Performed AppKit windowed maximize. A second click did not restore the previous smaller frame; a reversible zoom toggle is not claimed. |
| Edge resize | Physical drag changed dimensions to 1491 x 853 and 1280 x 800. |
| Tabs / split panes | New tabs, split-right, tab reordering by drag, and divider drag changed pane width. Whole-pane relocation was not separately exercised. |
| Shell | Controlled file write read back `QA-shell`; visible terminal output and command execution verified. |
| Chinese IME | Existing Doubao Pinyin displayed `ni'hao` candidates; continuous physical key events plus Space committed `你好`. Separate helper activations disturbed composition and were excluded as harness artifacts. |
| Settings | Added dummy Ollama provider/model, selected default, saved, read config, closed/reopened Settings, and observed retained values and approval mode. |
| Codex Markdown links | Colon range, hash range and OSC 8 file URL each opened the correct local Markdown fixture with Cmd+click. Wrapped paths remained recognizable. |
| Desktop notification delivery | QA OSC 777 notification reached macOS Notification Center with the correct title/body. First permission failure was visibly reported in-app. |
| Notification click | **Passed in follow-up for the originating tab:** the matched current-session notification returned identical window/group/pane data on the main thread and restored the tab with the issuing command. Both AXPress and corrected physical single-click events exercised the existing callback. The earlier click expanded a notification group or used incomplete synthetic mouse events. Cross-window and inactive split-pane navigation were not separately exercised. |
| Markdown line target | **Passed after repair in the final optimized binary:** the anchored fixture opens Raw at line 12, column 3; clipboard readback selects `Li` (two characters). Rendered-to-Raw switching preserves that position. Reopening the same file at line 15, column 4 selects `Lin` and highlights line 15. A plain link still opens Rendered. |
| Code-file target | **Passed in follow-up for VS Code:** Cmd+click on the `.rs:12:3-20:9` fixture and an OSC 8 `file://` hash-range link opened the controlled file. The editor status bar showed line 12, column 3. Synthetic clicks now explicitly set a single-click count and allow asynchronous path scanning to complete. Other editors are not covered by this result. |

The initial permission toast and invalid troubleshooting URL were reproduced
and repaired. In the optimized repair build, two controlled denials produced
one permission toast and one banner. Real accepted authorization removed both,
and actual notification delivery succeeded. A controlled unrelated send error
remained visible after accepted authorization and a later successful send.
Troubleshooting opened Notifications Settings, with native URL result 1;
the Apple documentation URL returned HTTP 200. No global Focus, notification
or input preferences were changed.

The follow-up used an injected diagnostic library only in the QA identity.
Real-mode methods forwarded to the production implementation; the diagnostic
wrapper counted completion calls without supplying one. Explicit QA-only
denial/send-error modes exercised permission recovery and error preservation.

The final optimized binary was rebuilt after the column and LSP producer repairs.
Its actual
notification send succeeded; a physical click returned the identical
window/group/pane context on the main thread, completed once, and restored the
issuing tab. Close, minimize/restore, green-button fullscreen/Fn+F, plain
Markdown preview, both Markdown source anchors and VS Code line 12/column 3
were rechecked. The controlled editor fixture tab was closed, the QA app and
server exited normally, and Codex was restored to the foreground. Installed
app processes 13611 and 13612 remained running.

## Keychain And External Gaps

A unique dummy OS Keychain item was created, read, and overwritten successfully
through the existing Rust backend. The bundled helper's cross-process read
prompted for the login Keychain password. The QA prompt was denied and the
exact dummy item deleted; no existing user key was read and no password was
requested or handled. Cross-process authenticated reads remain unaccepted.

Real provider inference, actual in-app agent command execution, a physical
phone/Tailscale workflow, unexpected browser network reconnect, sound playback,
and an installed-app updater swap remain unverified. Their unit/mock tests
are supporting evidence, not end-to-end acceptance.

## Package

Archived maintenance-only artifact:
`dist/archive/2026-10-07-unpublished/omw-warp-oss-v0.0.13-aarch64-apple-darwin.dmg`.
Size: 284,324,970 bytes (approximately 271 MiB).
SHA256: `1c90e182cd3ea7aa0998eff87539d8420f085f081d5971bc9247935a7b316923`.

The refreshed DMG was mounted read-only and non-browsable. Its bundled CLI
passed the full isolated notification lifecycle and dispatcher-event harness.
Its bundled Agent/Node bytes match the previously exercised resources whose
streaming, cancellation, approval, routing, error and secret-free protocol
checks passed as described above. Strict/deep signature,
DMG internal integrity, companion SHA256, arm64 architecture, plist version,
embedded release tag, and the no-cloud audit passed. Package provenance and
release notes matched the final source; the mounted CLI matched staging.

The first no-cloud invocation used the system tool launcher without the CLT
environment. Its apparent success was rejected as evidence because `strings`
had failed under the Xcode license restriction. A fresh CLT-environment run
had no extraction errors, found the embedded release tag, and confirmed zero
occurrences of all eight prohibited cloud hostnames.

The app was rebuilt from the final repaired source. The bundled Agent and Node
retain the previously tested resource bytes; the CLI retains its final rebuilt
version. Signature-stripped temporary copies match the final production app
after normalizing only the signing-related `__LINKEDIT.vmsize` field. Load-command
inspection and an exact comparison establish that no other program bytes differ.
The bundle targets arm64, embeds
`omw-local-preview-v0.0.13`, and remains ad-hoc signed without Apple
notarization. No installation, push or publication.

## Evidence Files

Local ignored logs are retained under `.tmp/`: `acceptance-workspace.log`,
`acceptance-doc.log`, `acceptance-clippy.log`, `acceptance-cli-final-clippy.log`,
`acceptance-fmt.log`, `acceptance-secret-scan.log`, `acceptance-byorc.log`,
`acceptance-agent.log`, `acceptance-web.log`, `acceptance-web-build.log`,
`acceptance-warp-terminal.log`, `acceptance-updater.log`,
`notify-dispatch-red.log`, `notify-dispatch-green.log`,
`acceptance-cli-build.log`, `acceptance-packaged-agent.log`,
`acceptance-browser.log`, and `acceptance-keychain.log`.
Refreshed package logs: `acceptance-package-refresh.log`,
`acceptance-mounted-notifications.log`, `acceptance-mounted-agent.log`,
and `acceptance-mounted-no-cloud.log`.
Browser screenshots/assertions: `.tmp/acceptance-browser/`.
App/CLI supplementary regressions: `.tmp/app-final-integration-recheck.log`
and `.tmp/cli-full-verification.log`.
Follow-up native trace: `.tmp/diagnostic-qa.log`; semantic assertions:
`.tmp/diagnostic-followup-assertions.log`; retained controlled screenshots:
`.tmp/diagnostic-followup/`. Diagnostic scripts and libraries are ignored
temporary files, not production or package contents.

Repair evidence: `.tmp/repair-integration-serial.log`,
`.tmp/notification-response-red.log`, `.tmp/notification-response-green.log`,
`.tmp/repair-first-production-qa.log`, `.tmp/repair-production-build.log`,
`.tmp/repair-column-readback.log`, `.tmp/repair-column-source-green.log`,
`.tmp/repair-column-source-toggle-green.log`,
`.tmp/repair-column-production-green.log`,
`.tmp/repair-column-production-toggle-green.log`,
`.tmp/repair-column-production-second-green.log`, and `.tmp/repair-qa.log`.
LSP boundary evidence: `.tmp/lsp-column-red.log`, `.tmp/lsp-column-green.log`.
Final-source integration/build/package evidence:
`.tmp/repair-final-integration-green.log`,
`.tmp/repair-final-production-build.log`,
`.tmp/repair-final-package-refresh.log`,
`.tmp/repair-final-package-verification.log`,
`.tmp/repair-lsp-final-column-green.log`,
`.tmp/repair-lsp-final-toggle-green.log`,
`.tmp/repair-lsp-final-second-green.log`, and
`.tmp/repair-final-gui-assertions.log`.
Controlled screenshots are retained in `.tmp/repair-evidence/`.

## Build Environment

Rust 1.92.0, Command Line Tools, Homebrew protoc, and the existing Apple Metal
toolchain asset mounted read-only. A temporary `xcrun --no-cache` wrapper
avoided the cached Xcode compiler stub. Global Xcode configuration and license
acceptance were not modified. Original installed app processes stayed running.
