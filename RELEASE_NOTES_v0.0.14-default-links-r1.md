# oh my warp 0.0.14 default-links-r1

Local Mac preview revision of the public 0.0.14 feature baseline. This is a
separately named package; it does not replace the public release or Windows build.

## Default Codex Launches

Associates each terminal with its current native Codex thread and actual runtime
working directory. Relative file references can then resolve against that
directory, including linked and managed worktrees, resumed threads and multiple
clients. Missing or ambiguous context does not guess a directory.

Requires native Codex 0.162.1, with shared app-server metadata version 0.162 or
later. Tested on macOS 27.0 (26A428). Older macOS runtime acceptance for this
revision has not been performed.

## One-Time Setup

After installing this revision, preview the configuration change with:

```sh
omw codex-links --dry-run
```

To enable the native session identity for future Codex launches:

```sh
omw codex-links
```

The command adds `session-id` and `app-name` to Codex's native terminal title,
preserves existing title items and unrelated settings, and creates a private
configuration backup. It respects `CODEX_HOME` and valid configuration symlinks;
an invalid or dangling configuration link is refused without writing.

oh my warp hides the added prefix. Other terminal apps will display the native
session identity. No prompts, shell aliases, PATH adapters or Codex arguments are
changed. Existing running Codex clients keep running; start a new invocation to
read the updated native title setting. Keep using ordinary `codex` or
`codex --yolo` afterward. Removing the identity through Codex title customization
disables this association for that invocation.

## Acceptance Status

Native shared-daemon startup, --yolo, -C, simultaneous clients, linked and managed
worktree resume, interactive thread switching, directory-choice behavior, and
embedded startup/resume have passed background acceptance. Focused tests cover
identity collisions, missing/silent servers, timeout limits, writable rollout
ownership, symlink aliases, and configuration preservation.

Actual wide GUI Command-click opened the distinct ROOT-A and ROOT-B Chinese
Markdown files from root and linked-worktree sessions. The complete `.md:1`
suffix was highlighted. Narrow wrapped clicks and same-pane switching remain
pending. The user's native title preference is enabled with a private backup;
the production application has not been replaced. This local preview must not
be described as fully mouse-accepted yet. The signed package's bundled record
retains its earlier build-time acceptance snapshot; the repository record tracks
subsequent verification.

The complete custom 0.0.14 Mac source remains the baseline. Windows runtime,
input editing, model settings and notification behavior are outside this change.

Upstream inspected: `openai/codex` tag `rust-v0.162.1`, commit
`092d3acd6bec3e3a14bdc7e7a2810ab628ab759d`. No upstream code was copied.
See the bundled `UPSTREAM_PROVENANCE.json` and repository acceptance record.
