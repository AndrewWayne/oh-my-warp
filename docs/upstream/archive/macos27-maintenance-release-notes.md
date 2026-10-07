# omw-local-preview v0.0.13

macOS 27 maintenance build based on the complete 0.0.11 release source
(`24c06811dee6cb6b22d715c69657af909ade8d4d`). The divergent 0.0.12 preview
tree is not merged. Existing local agent, notification, OSC 8, wrapped-path,
tab/pane, provider, and update features remain in the source baseline.

## Repairs

- Native close, minimize, fullscreen, Option-green zoom, and resize events
  use AppKit handling on macOS 27, incorporating upstream fixes #12557,
  #12770, and #14716 while retaining terminal drag handling.
- Codex file references support complete line/column ranges, including
  `src/main.rs:12:3-20:9` and `src/main.rs#L12C3-L20C9`. File opening uses
  the start position. OSC 8 file URLs preserve positions and escaped
  characters in filenames.
- Markdown links with a source location open in Raw mode at that position.
  Links without a location retain the Markdown-preview preference. Switching
  Raw/rendered mode and opening from the preview retain the link's start location.
  Internal-editor jumps now convert source column numbers correctly instead of
  placing the caret one character to the right. LSP definition jumps in the
  editor and code-review pane normalize their zero-based positions before
  entering the same file-opening interface.
- Notification setup uses nonempty `CODEX_HOME` (otherwise `~/.codex`),
  recognizes quoted commands, repairs legacy unquoted script paths, and
  preserves unrelated commands during uninstall. The dispatcher uses the
  installed Command Line Tools Python when available, avoiding silent
  notification loss from a system Python launcher blocked by Xcode licensing.
- Repeated notification permission errors share one toast. Authorization
  recovery removes permission-related feedback. Troubleshooting opens macOS
  notification settings, and notification responses finish their native callback.
- Settings API-key actions print `<redacted>` in Debug logs. This prevents
  new key-input action logs from exposing keys; existing logs are not rewritten.

## Installation

Apple Silicon only (`aarch64-apple-darwin`). The bundle is ad-hoc signed,
without Apple notarization. Open the DMG and install `omw-warp-oss.app`
when current terminal sessions can be closed. This local build does not
replace the installed application automatically.

For a custom Codex home, run notification setup with the same environment
used to launch Codex, for example:

```sh
CODEX_HOME="$HOME/.codex-cli" omw notify-setup status --json
CODEX_HOME="$HOME/.codex-cli" omw notify-setup install
```

The app's setup banner inherits the app process environment. A Codex
wrapper that exports `CODEX_HOME` only for its child does not export that
setting back to the app; use the explicit CLI command in that case.

## Source Provenance

`UPSTREAM_PROVENANCE.json` in the package records the exact omw baseline
and adopted upstream commits. This is a selective maintenance backport,
not a whole-tree synchronization with upstream master.

## Acceptance Limits

This is a maintenance candidate, not a declaration of complete acceptance.
Physical window controls, resize, tab/divider dragging, Chinese input,
settings save/reopen, and local Markdown file opening were exercised on
macOS 27. Desktop notification delivery reached Notification Center.

Follow-up checks established that current-session notification clicks return
to the originating tab and VS Code opens the code fixture at line 12, column 3.
Other editors and cross-window/inactive-pane notification navigation are not
covered by those checks. The final optimized app also passed Markdown source
line/column jumps, repeated anchors, Raw/rendered switching, and ordinary
preview checks. Permission denial deduplication, recovery, unrelated-error
preservation, and native callback completion passed the repair verification.
Detailed evidence is recorded in the source verification report.

Real provider calls, a physical phone/Tailscale connection, an installed-app
update swap, and password-authorized cross-process Keychain reads remain
unverified. Mock-provider and browser checks are documented separately in
`docs/macos27-maintenance-verification.md` in the source tree.
