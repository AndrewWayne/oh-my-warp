# omw-local-preview v0.0.13 (Mac)

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

## Selected Improvements

- Agent settings include a per-provider Test button, with pending, success and
  failure feedback. It uses the current form inputs and keychain credentials
  without saving or sending conversation content. Editing, removing, applying
  or discarding providers invalidates stale results.
- Local Ollama can stream replies without an API key on Mac. The SDK validation
  workaround does not send an Authorization header. Configured keys still work.
- Local terminal panes have a persistent phone icon and overflow-menu action,
  also available through the command palette during alternate-screen programs.
  Sharing uses the existing pane and shell. Stopping one share leaves other
  shares active; closing a shared pane releases it. Startup runs in the background
  and cancellation rolls back an empty daemon. Existing read-only pairing remains;
  a Mac-only protocol repair lets it receive terminal output while rejecting input
  and ignoring remote resize controls. Existing write-enabled clients still work.
- Mac daemon startup, pairing-surface output and status Debug logs omit token-
  bearing pairing URLs. The intentional browser/QR/clipboard flow still works.
  Existing log files are not rewritten.

The connection test checks the provider's `GET /models` endpoint, connection
and authentication. It does not verify inference quality or the selected model.
Servers without that endpoint may support chat while failing this narrow test.
HTTP redirects are refused, and request timeout is 15 seconds. Failure messages
omit provider response bodies, credentials and configured URLs.

## Installation

Apple Silicon only (`aarch64-apple-darwin`), with minimum macOS 11.0 retained.
Windows application behavior remains at 0.0.12; its 0.0.13 packages are built
separately from the recorded Windows source. The Mac bundle is ad-hoc signed,
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

The embedded provenance is the Mac maintenance build-time record:
`windows_artifact_rebuilt=false` refers to that Mac work, and `published=false`
records its state when built. The combined GitHub release's platform manifest
records the separate Windows rebuild and exact source commits for both packages.

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
Detailed maintenance evidence is recorded in `docs/macos27-maintenance-verification.md`.
The four additions and current package checks are recorded in
`docs/macos-four-backports-verification.md`. Final optimized foreground sharing
checks remain pending after the host locked; automated remote protocol tests pass.

Real provider calls, a physical phone/Tailscale connection, an installed-app
update swap, and password-authorized cross-process Keychain reads remain
unverified. Mock-provider and browser checks are documented separately in
the two verification reports in the source tree.
