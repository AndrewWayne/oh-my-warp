# oh my warp 0.0.14 default-links-r2

Local Mac revision retaining the complete public 0.0.14 feature baseline and
the default Codex session-directory association from default-links-r1.
Public release assets and Windows builds are unchanged.

Fixes wrapped Chinese Codex references whose indentation is emitted by cursor
movement rather than literal spaces. Empty terminal cells now represent blank
columns during Codex path reconstruction, preserving indentation and cell ranges.
The focused regression distinguishes cursor-skipped blanks from explicit spaces.

Native Codex session titles remain required. `omw codex-links` enables them with
a private configuration backup while preserving unrelated settings. No prompt,
PATH adapter or Codex startup argument change is required. The local user's
configuration is already enabled; existing clients need a new invocation to
read it. oh my warp hides the added session-identity prefix.

Tested integration version: Codex 0.162.1. The native shared-daemon metadata
interface requires daemon 0.162 or later. Missing or ambiguous context skips
relative references. This revision does not extend the existing scan bounds.

Narrow-window Command-click passed on both rows of the wrapped Chinese reference,
including its complete `.md:1` suffix. A native interactive resume in the same
terminal opened the distinct worktree file. A continuous foreground switch back
to the root thread cleared the previous hover; clicking its old position did not
open a stale file. Four sparse-grid/soft-wrap regressions and the existing replay
matrix passed against the production library.

Acceptance and package verification are recorded in
`docs/codex-default-session-links-acceptance-2026-10-16.md`. The production
application is not replaced while its terminal sessions remain live.
