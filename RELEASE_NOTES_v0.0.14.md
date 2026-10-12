# oh my warp 0.0.14 (Mac Pre-release)

This build retains the complete 0.0.13 Mac feature set, including the 0.0.11
custom baseline, macOS 27 window repairs, notifications, local Agent, provider
connection tests, keyless Ollama, per-pane phone sharing, and log redaction.

## Codex File Links

- Local file references remain clickable when Codex enables mouse reporting.
- Chinese filenames highlight through the complete `.md` suffix. Soft wraps,
  Codex hard wraps, spaces, and narrow consecutive path lists retain one target.
- Relative references, `./` and `../` resolve against the associated Codex
  session directory. Explicit `-C` / `--cd` takes precedence; verified worktree
  and resumed-session contexts use their own directories.
- Line/column ranges and existing OSC 8 links retain the existing file-opening
  preference. No Codex prompt or configuration change is required.

Unknown or ambiguous session context skips relative links rather than opening a
same-named file from another directory. Default daemon sessions without explicit
`-C` are outside verified relative-path support. Reconstruction remains bounded
to eight physical rows and 8192 bytes. Older Codex state schemas and directory
changes inside the TUI are not exhaustively verified.

## Installation And Source

Apple Silicon only; minimum macOS 11.0 metadata is retained, but older macOS
execution is unverified. Open the DMG and drag `omw-warp-oss.app` to Applications
when your terminal sessions can be closed. This download does not replace a
running application automatically. The bundle is ad-hoc signed, not notarized.

If macOS quarantines the downloaded application, the preview installation
command is:

```sh
xattr -d com.apple.quarantine /Applications/omw-warp-oss.app
```

`UPSTREAM_PROVENANCE.json` records the inherited maintenance baseline and the
new local adaptation. The release platform manifest identifies exact sources,
build evidence and SHA-256 values. Windows is built separately with the same
version label and unchanged 0.0.13 application behavior.

## Acceptance Limits

The file-link QA passed focused regressions and captured-output replay. The
user reported successful root/worktree mouse acceptance. The distributed app
uses an optimized release build; package validation is recorded separately.

Codex input editing retains current behavior: Command+Delete and true
Command+Z/redo integration are not added. Physical phone/Tailscale, real provider
credentials, older operating systems, final settings restart/readback and
two-pane foreground sharing cleanup remain outside completed acceptance.

This is a publicly downloadable Pre-release, not a stable release.
