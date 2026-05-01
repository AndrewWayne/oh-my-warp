# CLAUDE.md

## 1. Think Before Coding

**Don't assume. Don't hide confusion. Surface tradeoffs.**

Before implementing:
- State your assumptions explicitly. If uncertain, ask.
- If multiple interpretations exist, present them - don't pick silently.
- If a simpler approach exists, say so. Push back when warranted.
- If something is unclear, stop. Name what's confusing. Ask.

## 2. Simplicity First

**Minimum code that solves the problem. Nothing speculative.**

- No features beyond what was asked.
- No abstractions for single-use code.
- No "flexibility" or "configurability" that wasn't requested.
- No error handling for impossible scenarios.
- If you write 200 lines and it could be 50, rewrite it.

Ask yourself: "Would a senior engineer say this is overcomplicated?" If yes, simplify.

## 3. Surgical Changes

**Touch only what you must. Clean up only your own mess.**

When editing existing code:
- Don't "improve" adjacent code, comments, or formatting.
- Don't refactor things that aren't broken.
- Match existing style, even if you'd do it differently.
- If you notice unrelated dead code, mention it - don't delete it.

When your changes create orphans:
- Remove imports/variables/functions that YOUR changes made unused.
- Don't remove pre-existing dead code unless asked.

The test: Every changed line should trace directly to the user's request.

## 4. Goal-Driven Execution

**Define success criteria. Loop until verified.**

Transform tasks into verifiable goals:
- "Add validation" → "Write tests for invalid inputs, then make them pass"
- "Fix the bug" → "Write a test that reproduces it, then make it pass"
- "Refactor X" → "Ensure tests pass before and after"

For multi-step tasks, state a brief plan:
```
1. [Step] → verify: [check]
2. [Step] → verify: [check]
3. [Step] → verify: [check]
```

Strong success criteria let you loop independently. Weak criteria ("make it work") require constant clarification.

## 5. Project-specific rules

`omw` is the product brand; `oh-my-warp` is the repo codename (see [PRD §12.1](./PRD.md#121-brand-vs-codename)).

- **Brand.** Never write `Warp` (capitalized) in product-surface code or docs. Allowed in: file paths, `LICENSE`, source-attribution comments (e.g. `// upstream:` blocks), and the literal `oh-my-warp` codename.
- **Vendor.** `vendor/warp-stripped/` is the in-tree Warp fork (AGPL). Direct edits are allowed; preserve the AGPL header on every file you touch. Upstream sync is a manual procedure documented in `specs/fork-strategy.md` §2 — do not run it autonomously.
- **Spec coupling.** Any change to PRD.md §3.1 (v1.0 Committed Scope) MUST include a TODO.md update in the same PR.
- **Test gate.** Any new endpoint in `crates/omw-remote/` requires a contract test (see `specs/test-plan.md` §1.2) AND a fuzz target (`specs/test-plan.md` §3.1) before merge.

If you're unsure whether a change crosses the brand or vendor lines, run `/spec-consistency` and `/check-scope` from the project's slash commands before opening a PR.

### 5.1 Release naming conventions (omw_local previews)

Until v0.3 closes (binary rebrand to `omw`, `omw-server`, agent panel — see [TODO.md](./TODO.md) v0.3), `.dmg` releases of the audit-clean `omw_local` build use the **preview** track, not the v0.x phase tags reserved in TODO.md.

- **Tag.** `omw-local-preview-v<x.y.z>` (e.g. `omw-local-preview-v0.0.1`). The reserved tags `v0.1`, `v0.2`, `v0.3`, `v1.0` follow the [TODO.md](./TODO.md) phases and are NOT used for previews.
- **Distributed app/binary name.** `omw-warp-oss`. The Cargo `[[bin]] warp-oss` inside `vendor/warp-stripped/app/Cargo.toml` is unchanged — the rename happens at packaging time only, to keep fork delta small per [`specs/fork-strategy.md`](./specs/fork-strategy.md) §2.
- **Bundle ID.** `omw.local.warpOss`. The embedded plist baked into the upstream binary still says `dev.warp.WarpOss`; the `.app`'s `Contents/Info.plist` overrides for LaunchServices identity. App data dir: `~/Library/Application Support/omw.local.warpOss/`. Logs still land in `~/Library/Logs/warp-oss.log` (path inherited from the embedded plist; not currently overridden).
- **Architecture.** `aarch64-apple-darwin` only. Universal/x86_64 deferred to a later release track.
- **Signing.** Unsigned. Install instructions ship `xattr -d com.apple.quarantine /Applications/omw-warp-oss.app`. Codesign + notarize is a v1.0 task (see [PRD §13](./PRD.md#13-phased-roadmap)).
- **Package format.** `.dmg` containing `omw-warp-oss.app` + `Applications` symlink + `LICENSE` + the matching `RELEASE_NOTES_v<version>.md`.
- **Brand carryover.** Preview icons may carry transitional "OSS" glyphs derived from upstream until the omw rebrand work in v0.3 lands. The literal `Warp` wordmark stays prohibited per §5 even on previews.
- **Build entry point.** `bash scripts/build-mac-dmg.sh <version>` from the umbrella root. The script does not modify `vendor/warp-stripped/`.