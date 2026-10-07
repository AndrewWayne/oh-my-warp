# 0.0.13 Draft Release Implementation Plan

**Goal:** Publish one GitHub Pre-release for 0.0.13, with complete Mac
and Windows downloads, preserving the Mac 0.0.11 feature baseline/maintenance
repairs/four backports and the Windows 0.0.12 application behavior.

**Stable references:** User explicitly requested the GitHub trial release as a
preparation version rather than a final release. The user clarified that this
means a publicly downloadable Pre-release, not a private Draft. Earlier Windows preservation
and background-work requirements remain. `CLAUDE.md`, `specs/fork-strategy.md`,
`docs/upstream/v0.0.13.json`, and both Mac verification reports apply.
Windows base: `299b55e1c53b685fc2ee665b6beb3b8f28e21eb8` (0.0.12).
Mac base: `24c06811dee6cb6b22d715c69657af909ade8d4d` (0.0.11).

**Repository operations:** Source commits and new branch pushes are necessary
to provide the source for the requested GitHub artifacts and are authorized by
this release request. Keep `main` and unrelated dirty worktrees untouched. Use
`fix/macos27-maintenance` for the exact Mac source and an isolated
`release/v0.0.13-windows` worktree for the unchanged Windows application source.
No force push, merge, installed-app replacement, final release,
or latest-release promotion. Prepare in Draft, then publish as Pre-release only
after assets and checks are complete. Do not push a release tag that could rebuild these
platforms from the wrong baseline. Continue the established inline execution.

**Recovery state:** Mac 0.0.13 has been rebuilt and checked. DMG SHA-256:
`dbe880f4ec2b0733cc20e054b6f2edefac9f4f9c02d7c1c10c4c68cb691d633a`.
Fresh evidence: `.tmp/renumber-0013-{package-build,package-check,agent-check}.log`.
Keep the two final foreground acceptance gaps explicit. GitHub 0.0.12 has only
Mac assets. Windows run `33082661977` compiled successfully, but installer smoke
failed with exit 13 and its 14-day audit artifact has expired. The full log is
retained at `.tmp/windows-0012-release-job.log`. Do not present old missing
Windows assets as verified or rename an old binary to claim a new version.

## Task 1: Preserve And Publish Corresponding Source

**Project meaning:** Every downloadable binary has inspectable source and a
clear platform baseline; local custom behavior remains intact.
**Architecture relationship:** Mac maintenance is based on 0.0.11 with selected
backports, while Windows 0.0.12 contains additional Windows-only work. Distinct
source branches prevent cross-platform source replacement.
**Technical contract:** Commit only authorized maintenance/backport source,
tests, provenance and release records. Exclude `.tmp/`, generated `dist/`, the
untracked `target` symlink and unrelated work. Add a narrowly scoped 0.0.13
workflow guard to preserve the manually curated platform artifacts.
**Dependencies:** Already verified runtime source; no runtime changes needed.
**Semantic acceptance:** Inspect staged diff and secret/license checks; push
new source branches; verify GitHub commit SHA matches local SHA and both bases.
**Recovery checkpoint:** Record exact source commits and pushed refs below.

## Task 2: Complete Windows Artifacts

**Project meaning:** The draft includes an actually usable Windows distribution.
**Architecture relationship:** The 0.0.12 build script bundles the Windows app,
Node, Agent, Credential Manager helper, ConPTY/OpenConsole and runtime DLLs.
The NSIS installer wraps the same verified portable payload.
**Technical contract:** Build with version 0.0.13 from the 0.0.12 application
source. Windows application code remains unchanged. Changes to build workflow,
packaging diagnostics or a demonstrated packaging defect must be documented
separately. Upload only after the existing integrity and installer gates pass.
Require the destination to remain Draft + Pre-release before upload. Skip Mac
CI to retain the locally accepted DMG. Do not weaken failed smoke gates.
**Dependencies:** Source branch and an existing draft destination.
**Semantic acceptance:** Real Windows runner verifies all ZIP paths/hashes,
bundled Node/Agent/helper probes, forbidden-cloud audit and installer first
install, locked/successful replacement upgrade, metadata/shortcuts and uninstall.
**Recovery checkpoint:** Record CI run ID, outcome, artifact digests and any
packaging diagnosis. Do not declare completeness while assets are missing.

## Task 3: Assemble And Read Back The GitHub Draft

**Project meaning:** Users review a single 0.0.13 trial with both platforms.
**Architecture relationship:** GitHub release body and an attached platform
manifest identify exact source commits and download checksums, while preserving
the existing Mac package's release notes/provenance byte-for-byte.
**Technical contract:** Tag name `omw-local-preview-v0.0.13`; prepare with
`draft=true`, then publish with `draft=false`, `prerelease=true`,
`make_latest=false`. Attach Mac DMG/sidecar, Windows
ZIP/sidecar and setup EXE/sidecar, plus platform source/checksum manifest.
Document current acceptance limits, not inherited claims of full testing.
**Dependencies:** Source snapshots and accepted package outputs.
**Semantic acceptance:** Authenticated release readback proves both flags and
target commit; server asset sizes/digests match verified local/CI artifacts.
The previous stable release remains latest and the installed Mac app untouched.
**Recovery checkpoint:** Record release ID/URL, final assets and flags below.

## Progress

- Repository `AndrewWayne/oh-my-warp`; logged-in account has WRITE access.
- Existing 0.0.13 GitHub Release is absent.
- Isolated Windows worktree created from the exact 0.0.12 tag.
- No runtime source edits have been made for this publication task.
- User clarified public Pre-release delivery; Draft is only a staging state.
