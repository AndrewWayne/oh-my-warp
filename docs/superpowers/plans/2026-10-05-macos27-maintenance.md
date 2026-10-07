# macOS 27 Maintenance Implementation Plan

**Goal:** Build omw 0.0.13 for macOS 27 while preserving every feature in 0.0.11. Repair native window controls, Codex file references, notification setup detection, and API-key action logging.

**Stable references:** User authorized implementation on 2026-10-05. Base is `omw-local-preview-v0.0.11`, commit `24c06811dee6cb6b22d715c69657af909ade8d4d`. Follow `CLAUDE.md` and `/Users/shuokong/Desktop/cailab/AGENTS.md`. Do not merge the divergent 0.0.12 tree.

**Repository operations:** Inline implementation in `worktrees/macos27-maintenance`, branch `fix/macos27-maintenance`. Local build and test artifacts are authorized. No push, GitHub release, or replacement of the installed running app is required for this task.

**Interaction boundary:** The earlier background-only request was honored. The user later explicitly authorized foreground acceptance with "你现在测试吧". Physical tests were resumed only in the isolated QA identity/configuration. The QA app has now exited; foreground checks and remaining limitations are recorded in the verification report. Do not replace or stop the installed app.

**Explicit preservation boundary (user reaffirmed during the build):** omw remains based on its complete 0.0.11 source and retains its own modifications. Adapt selected upstream fixes in the maintenance areas. The user's follow-up authorizes repairing the diagnosed Markdown location and notification feedback/callback defects. Do not replace the vendor tree, convert the product into current upstream warp, or refactor unrelated omw modules. Test exports, regression cases, provenance packaging, and release documents are the supporting scope.

**Recovery state:** Read this file and `git diff` in this worktree. Existing repo and combo worktree contain user work and must remain untouched. Track verification below. Use Rust 1.92.0 and `DEVELOPER_DIR=/Library/Developer/CommandLineTools` for the current host; the default Xcode installation has an unaccepted license.

## Deliverables

1. Native window events: port upstream commits `4aeeebfc8d36a25f719ee95793325838072a4954`, `ec27d06d7dcc08d78a1447ab0c5974e506e936ff`, and `64e3cd4745ff0148437a0982c7b57179646e69ef` into `vendor/warp-stripped/crates/warpui/src/platform/mac/objc/window.m`. On macOS 27, preserve AppKit traffic-light and resize events; retain terminal tab/pane drag behavior. Verify physical mouse events for close, minimize, fullscreen, Option zoom, and resize in a separate test app.
2. File links: extend `warp_util::path::CleanPathResult` to parse Codex hash and colon line/column ranges, choosing the start location. Preserve legacy path formats and filenames. Propagate locations for OSC 8 file URLs in `app/src/terminal/view/link_detection.rs`. Verify parser cases and real file opening, including wrapped links, URI escaping, and legacy formats.
3. Notification setup: in `crates/omw-cli/src/commands/notify_setup.rs`, honor nonempty `CODEX_HOME` for install/status/uninstall and report the selected path. Quote the dispatch script path consistently and recognize both legacy unquoted and quoted hooks. Keep status JSON fields stable, preserve unrelated hooks, and verify idempotent install/status/uninstall under temporary homes with spaces. Read-only status against the real active home must report installed.
4. Log redaction: port the custom `Debug` implementation from omw commit `8bde12e6ab31f7754768d07556d97c80c7d76c9c` into the existing settings action enum. Verify that API-key action formatting contains `<redacted>` and cannot contain the supplied key; keep reducer behavior intact.
5. Package and provenance: add release notes and an upstream record that distinguishes the original snapshot, selected fixes, and local changes. Build an ad-hoc-signed arm64 DMG with the bundled CLI and agent. Verify checksum, signature, no-cloud audit, feature flags, and runtime smoke in an isolated test identity. Do not claim a complete upstream sync.

## Progress

- [x] Isolated checkout created at the exact 0.0.11 release commit.
- [x] Regression evidence: old path probe fails complete Codex ranges; custom CODEX_HOME and mixed-hook uninstall tests fail before their fixes.
- [x] Four repairs implemented, with focused notification and path tests passing.
- [x] Rebuilt final app integration: 73 passed, 2 existing stub-server cases ignored. All 4 file-link cases now pass against the complete app library, including the final existing-filename guard. Full CLI regression: 69 passed.
- [x] Release build, DMG, signature, checksum, and source audit.
- [x] Runtime smoke and final review; unavailable GUI checks recorded in `docs/macos27-maintenance-verification.md`.
- [x] Authorized foreground checks: close, minimize/restore, fullscreen, Option-green zoom, edge resize, tab/divider dragging, shell input, Chinese IME, settings save/reopen, and Markdown links.
- [x] Expanded workspace, browser, packaged-Agent, notification dispatcher, and updater checks; evidence and skips recorded in the verification report.
- [x] Notification dispatch licensing failure reproduced, fixed in both script copies, and covered by an installed-script regression.
- [x] Corrected diagnostic acceptance: native notification click restored its originating session; VS Code opened the controlled source file at line 12, column 3. Initial failures were caused by the QA mouse/notification-group handling.
- [x] Repair diagnosed defects: anchored Markdown opens Raw at the source location; Raw/rendered switching and editor launch retain that location. Ordinary Markdown preview preference remains available. Source entry points populate link metadata; repeat links update the retained location.
- [x] Permission feedback deduplicates and clears on authorization recovery without hiding unrelated errors; notification help/settings links have valid destinations. Optimized QA runtime verified denial/recovery and preserved a controlled Other error.
- [x] Native notification response calls completion exactly once, including non-default actions and absent Rust wrapper. Six compiled Objective-C cases pass; an actual optimized-app click completed once and restored the issuing tab.
- [x] Column precision: GUI readback exposed one-based/zero-based mismatch; the file-jump boundary now converts columns. Source QA selects `Li` at column 3 before and after mode switching, compared with pre-fix `Lin`.
- [x] Shared-consumer review: editor and code-review LSP definition targets normalize zero-based columns to the file-opening contract. Extracted production-expression red/green probe preserves five columns through both routes. Real language-server GUI navigation remains unverified.
- [x] Rebuild and test the repaired production binary/package in the isolated QA identity; update release evidence and checksum. Final-source integration: 89 passed, 2 existing ignores. Final optimized Raw anchors, mode switching and actual notification response passed. Mounted DMG passed signature, arm64 identity, embedded tag, provenance/notes, no-cloud, program-byte comparison and bundled notification lifecycle checks. SHA256: `1c90e182cd3ea7aa0998eff87539d8420f085f081d5971bc9247935a7b316923`.
- [ ] External acceptance gaps: real provider, phone/Tailscale, installed-app update swap, cross-process Keychain authorization, other editors, and inactive-pane/cross-window notification navigation require separate acceptance.

## Verification So Far

- `warp_util` release test executable: 59 passed, including legacy and Codex path formats.
- `omw-cli` notification integration: 7 passed, including custom homes, quoting, idempotence, and mixed-group preservation.
- Real configuration read-only probe with `CODEX_HOME=/Users/shuokong/.codex-cli`: Claude, Codex, and scripts all detected.
- Agent kernel: TypeScript build passed; 89 tests passed, 3 real-keychain integration tests skipped by existing configuration.
- Original inline-agent prompt integration file: 7 passed against the compiled app test library, linked directly without generating additional dSYM bundles.
- Vendor AGPL header check, provenance JSON/SHA validation, shell syntax, and `git diff --check` passed.
- Source-boundary audit against the exact 0.0.11 tag passed: every tracked delta is in the four maintenance areas or test/packaging support. Agent, provider config, remote/server/PTY, omw feature flags, and updater implementations are byte-for-byte unchanged in Git.
- The initial app test build preceded the final filename guard. The supplementary complete-app rebuild closes that gap: all 4 file-link cases passed, along with the other 69 app integration cases. Only the app package's optimization/debug information was disabled for the supplementary test build; production features were retained with test exports added. Actual GUI file opening remains unverified.
- The expanded run found a stale baseline settings test that rejected allowed non-default drafts and assumed no bootstrapped config file. Its fixture now uses an incomplete default provider and asserts unchanged config bytes. A companion test verifies allowed draft saving. Production settings logic is unchanged.
- Final default-feature release build and DMG creation succeeded. App, CLI, and bundled Node are arm64; the binary embeds `omw-local-preview-v0.0.13`. The final binary passed the no-cloud audit.
- SHA256 companion check and DMG internal integrity check passed. A read-only, non-browsable mount verified the actual DMG app's strict/deep signature, binary, release notes, and provenance against the staged release.
- The actual DMG's bundled Node and Agent passed JSON-RPC session create, cancel, and unknown-method checks without sending a prompt or accessing a key. Bundled CLI read-only detection with the active custom Codex home reported all three status fields true.
- On this macOS 27 host, a physical green-button click entered fullscreen (1280 x 800 to 1710 x 1073); Fn+F restored the original dimensions. Other physical checks were stopped when the user requested background-only operation. The QA app exited; original installed app processes remained running.
- Subsequent foreground authorization enabled the additional physical checks above. Current results supersede the earlier background-only GUI limitation; see the verification report for precise observations and unresolved navigation cases.
