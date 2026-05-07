---
name: release-build-audit
description: Walk every Command::new() spawn site in omw_local code and cross-reference against the bundling steps in scripts/build-mac-dmg.sh (and the Windows equivalent). Use BEFORE running a release build script; this is the audit that would have caught the v0.0.3 keychain-helper and Node-on-PATH ship bugs. Argument is the target version.
tools: Bash, Read, Grep
---

# Release Build Audit

Verifies a release build is self-contained: every external binary the
runtime spawns is either bundled inside the `.app`, resolved via an
explicit well-known-paths candidate list, or has a graceful runtime
fallback that doesn't break the user's golden path.

The companion hook `.claude/hooks/guard-release-build.sh` runs a fast
structural check on every `bash scripts/build-mac-dmg.sh ...` invocation;
this skill is the deeper, comprehensive audit you run by hand before
publishing a release.

## When to invoke

- Before running `bash scripts/build-mac-dmg.sh <version>` for any
  release intended for download (preview tracks included).
- After adding a new `Command::new()` / `tokio::process::Command::new()`
  call in any omw_local-specific code.
- After adding a new env var read in omw_local-specific code.

Do NOT invoke for local debug builds; use `CLAUDE_HOOKS_DISABLED=1` to
bypass the structural hook there.

## Procedure

### 1. Enumerate spawn sites

In omw_local-specific code only, list every `Command::new(...)` and
`tokio::process::Command::new(...)`:

- `vendor/warp-stripped/app/src/omw/`
- `vendor/warp-stripped/app/src/ai_assistant/omw_*.rs`
- `crates/omw-server/src/agent/`
- `crates/omw-pty/src/`
- `crates/omw-remote/src/`
- `apps/omw-agent/src/`

Upstream warp spawns (`git`, `gh`, `brew`, `osascript`, `ssh`, `wsl`,
PowerShell) are out of scope — those drive opt-in UX surfaces (CLI
install prompt, Homebrew detection, etc.) that aren't on the golden
path for v0.0.x previews.

### 2. Classify each spawn site by resolution strategy

For each site, determine how the binary is found at runtime:

| Strategy | Verdict | Examples |
|---|---|---|
| **Bundled at `Resources/<path>`** with explicit `<exe_dir>/../Resources/...` probe | ✓ self-contained | `omw-agent.mjs`, `omw-keychain-helper`, `node` |
| **Well-known absolute paths** with explicit candidate list | ✓ external-but-resolved | `tailscale.rs:find_tailscale_binary` (Homebrew, /usr/local, /Applications) |
| **PATH lookup** (`Command::new("foo")` with no path resolution) | ✗ FAILS on Finder launch — minimal `PATH=/usr/bin:/bin:/usr/sbin:/sbin` excludes Homebrew | (must be fixed) |
| **Static link** (no separate binary) | ✓ N/A | `omw-remote` daemon (linked into warp-oss) |

A `.app` launched from Finder/LaunchServices inherits the system minimal
PATH. Bare `Command::new("node")` was the v0.0.3 bug.

### 3. Cross-reference the build script

For every spawn site classified as "Bundled at Resources/...", grep
`scripts/build-mac-dmg.sh` for an explicit `cp` or `ditto` to that path.
Flag any path the runtime probes for that the build script does not
place. This is the fast structural check the hook also runs.

### 4. Audit env vars

Find every `std::env::var*("OMW_*")` and `process.env.OMW_*` read in
omw_local code. For each:

- If the code falls back gracefully when unset → OK (override-only).
- If unset crashes the runtime or breaks a user-visible feature → must
  be documented in `RELEASE_NOTES_v<version>.md`, OR the build must
  bake in a default that doesn't require the env var.

The .app from Finder does NOT inherit the user's shell env vars, so any
required `OMW_*` env var is effectively unset on the golden path.

### 5. Run audit-no-cloud against the staged binary

If `vendor/warp-stripped/target/release/warp-oss` exists, run
`bash vendor/warp-stripped/scripts/audit-no-cloud.sh
vendor/warp-stripped/target/release/warp-oss`. Must pass (zero hits on
the eight forbidden hostnames).

### 6. Verify bundle inventory after a build

If `dist/staging-v<version>/omw-warp-oss.app/` exists, inventory
`Contents/Resources/`. Cross-check against the spawn-site classification
from step 2.

For `Resources/node_modules/`, presence of the directory is not enough.
Read the runtime dependencies declared in `apps/omw-agent/package.json`
(`Object.keys(dependencies)`) and verify each one resolves to a
non-empty subdirectory under `Resources/node_modules/`. Walk one level
deeper for scoped packages (e.g. `@mariozechner/pi-ai`).

This catches the v0.0.3-rev2 ship bug: the repo is an npm workspace
(`workspaces: ["apps/*"]`) so deps hoist to the repo root. A fresh
`apps/omw-agent/node_modules/` is empty save for `.vite/`, and the
build script's prior `[[ ! -d node_modules ]]` guard was satisfied by
that stray dotfile-dir, silently shipping a bundle with no runtime
deps. The kernel ENOENTs on `@mariozechner/pi-ai` at first
`/agent/sessions` POST and the GUI sees a 503 with
"agent process exited before request completed". The build script now
installs deps in an isolated tmp dir to bypass workspace hoisting and
hard-fails if any declared dep is missing from the bundle, but this
audit step is the human-judgment backstop in case that ever regresses
(e.g. a new dep is added in package.json but its name is misspelled in
either place, or the isolated install is replaced with something that
re-introduces hoisting).

### 7. Sentinel (optional)

On full PASS, write `.claude/cache/release-audit-passed.<HEAD-SHA>` so
future tooling can confirm an audit was performed on this commit. The
current hook does not require this sentinel; it does its own structural
check on every invocation.

## Output format

```
Release build audit — HEAD <sha> — target v<version>

Spawn sites (omw_local code only):
  ✓ vendor/.../omw_inproc_server.rs:179  Command::new(node)            [bundled@Resources/bin/node via locate_node()]
  ✓ vendor/.../omw_inproc_server.rs:177  cfg.command (helper child)    [bundled@Resources/omw-keychain-helper]
  ✓ vendor/.../tailscale.rs:41           Command::new(<find_tailscale>) [well-known paths: /opt/homebrew/bin, /usr/local/bin, /Applications/Tailscale.app/...]
  ✓ crates/omw-pty/src/lib.rs:239        spawn_blocking(pty)            [static link via portable-pty]

Bundle parity (Resources/ probes vs build-mac-dmg.sh cp/ditto steps):
  ✓ Resources/bin/omw-agent.mjs
  ✓ Resources/bin/node
  ✓ Resources/omw-keychain-helper
  ✓ Resources/dist/ Resources/vendor/ Resources/package.json (kernel layout)
  ✓ Resources/node_modules/ — every declared runtime dep present (3/3: @mariozechner/pi-ai, @iarna/toml, typebox)

Env vars audited:
  ✓ OMW_AGENT_BIN          (override-only; default: locate_kernel_script)
  ✓ OMW_AGENT_NODE         (override-only; default: locate_node)
  ✓ OMW_KEYCHAIN_HELPER    (override-only; default: locate_keychain_helper)
  ✓ OMW_SERVER_URL         (override-only; default: 127.0.0.1:8788)

Forbidden-hostname audit:  ✓ (audit-no-cloud.sh: 0 hits across 8 hosts)

Verdict: PASS
```

If any item is `✗`, the verdict is FAIL with a per-issue remediation
note. Do not proceed to the build until FAIL items are resolved.

## Notes

- Read-only by default. The only write is the sentinel file on PASS.
- The structural hook is fast but coarse; this skill is the human
  judgment layer (e.g. "is this PATH-fallback acceptable because the
  feature is opt-in?").
- For the Windows track (deferred past v0.0.3), parity check
  `scripts/build-windows-zip.ps1` against the same spawn sites, with
  `.exe` suffixes and Windows-native paths.
