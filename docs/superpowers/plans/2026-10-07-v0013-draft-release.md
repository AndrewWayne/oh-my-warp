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

**Recovery state:** Public Pre-release publication is complete. Release ID
`405633716`, tag `omw-local-preview-v0.0.13`, target
`47ed36e813f08da521a428267ce77856da8c9dc8`. Final Mac DMG SHA-256:
`bc6df1c2874c43ef919352761424fba96ef80455f856607e8e0e4d0b0cde6e8f`.
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
the accepted Mac app and its build-time provenance byte-for-byte. The outer
DMG README clarifies the separate Windows rebuild; it does not change the app.
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
- Source snapshots committed and pushed: Mac
  `256a52d3f9de57c32764dc0e9d85e624b11ca764` on `fix/macos27-maintenance`,
  Windows `2866f442fd9564bb78ed425392f6df29754ad3bb` on
  `release/v0.0.13-windows`. Both GitHub SHAs match local. Windows application
  trees (`apps`, `crates`, `vendor`) are byte-unchanged from 0.0.12.
- Staged secret scans, Mac vendor headers (35 files), diff checks and both
  workflow actionlint checks passed. No corresponding runtime changes were
  introduced during this publication task. Windows installer diagnostics now
  record the failing stage and OS error; successful install/uninstall removes
  the owned diagnostic log. Existing installer gates remain intact.
- Temporary GitHub Draft + Pre-release created with target Mac source snapshot;
  title `omw-local-preview v0.0.13 (preview)`. Draft URL currently uses
  `untagged-1d3dce7bb202811c012b`. Mac DMG and sidecar are uploaded, and the
  server DMG digest matches the accepted `dbe880...` checksum. Readback is in
  `.tmp/release-0013-draft-readback.json`.
- Windows workflow dispatched explicitly from its source branch, with
  `tag=omw-local-preview-v0.0.13`, `targets=windows`: run **37606388621**,
  source **2866f442fd9564bb78ed425392f6df29754ad3bb**. Prepare passed, Mac job
  skipped, Windows build is in progress. Watch log:
  `.tmp/windows-0013-watch.log`. Keep monitoring; do not call the trial complete
  or publish before accepted Windows assets are present.
- Stable latest before staging: `omw-local-preview-v0.0.11`, release ID
  `368393805`. Main remote SHA: `da678973e7846f32ee3872c817f8adfbac0f2ff6`.
- At draft creation, remaining work was Windows package/installer acceptance, exact platform source/hash
  manifest and source archives, final public notes, publish with
  `draft=false`/`prerelease=true`/`make_latest=false`, verify public readback.
- Temporary release ID **405633716**. Mac asset sizes/digests confirmed by GitHub;
  public release/tag URL will become `.../releases/tag/omw-local-preview-v0.0.13`
  after publishing. Current release remains private during preparation.
- Windows dependency audit confirms bundled flat paths for Node, Agent,
  Credential Manager helper and ConPTY/OpenConsole/DXC/VC runtime. Optional
  Tailscale has a well-known Windows path and missing-install feedback; browser
  launch/taskkill use OS commands. The real package's probes and manifest/hash
  verification still have to pass on the Windows runner.
- A small real-NSIS installer fixture runs concurrently to discriminate basic
  extraction/activation/upgrade/uninstall problems without waiting for the
  monolithic program compile. Workflow-only commit
  **ef55bb58a21ed3d8a79c6564fc48a0d373e80f9c**, fixture run **37607033080**.
  This fixture is not a substitute for the full-payload installer smoke.
- Fixture run **37607033080 passed** on the real Windows runner, including
  the representative long SDK filename, first install, locked upgrade,
  replacement upgrade, metadata/shortcut readback and uninstall. Evidence:
  `.tmp/windows-0013-installer-fixture.log`. Basic installer lifecycle is
  functional; any reproduced full-payload failure requires its stage/error
  evidence before a fix. The full Windows run **37606388621** is still building.
- Outer Mac README now distinguishes unchanged Windows application behavior
  from separately rebuilt Windows 0.0.13 packages. The embedded Mac provenance
  retains its build-time flags. Repack only the outer DMG documentation and
  verify the accepted app bytes and signatures before replacing the draft asset.
- Documentation-only DMG repack passed strict/deep signatures, mounted integrity,
  exact source-program comparison, no-cloud and notification lifecycle checks.
  Final Mac size **281455711**, SHA-256
  **bc6df1c2874c43ef919352761424fba96ef80455f856607e8e0e4d0b0cde6e8f**.
  The signed app SHA remains `3f44db0b...`; only the outer README changed.
  Prior DMG/sidecar retained under `before-dual-platform-notes/` in the archive.
- Full Windows run **37606388621** passed the combined build/package/audit step,
  complete-payload installer smoke and release upload. CI is finishing cache
  cleanup. The six binary/checksum assets are now present in the draft.
  Windows ZIP size **190805188**, SHA-256
  **755479b41528b52e7fe0998affa357896bc74ed6acff052ffda12b4be8ebbb9f**;
  setup size **131893778**, SHA-256
  **d3990c31598043df30b01d3b6ab25dc5abb1b7a54f417e98d9257e8ad8017a28**.
  The earlier 0.0.12 installer exit 13 did not reproduce; no underlying installer
  root-cause fix is asserted. Windows `apps`, `crates`, `vendor` remain unchanged
  from the exact 0.0.12 base (fresh `git diff --exit-code` passed).

## Final Publication Record

- Published **2026-10-07 11:33:30 UTC** as a publicly downloadable Pre-release:
  https://github.com/AndrewWayne/oh-my-warp/releases/tag/omw-local-preview-v0.0.13
- Authenticated and anonymous API readbacks confirm `draft=false`,
  `prerelease=true`, the expected release body, nine assets and all sizes/digests.
  Latest stable remains **0.0.11**, release ID **368393805**. Main remains
  `da678973e7846f32ee3872c817f8adfbac0f2ff6`.
- Windows run **37606388621** completed with **success**, exact source
  `2866f442fd9564bb78ed425392f6df29754ad3bb`. Its full ZIP validation verified
  **16979 files, 70 directories, 489372883 payload bytes**. Bundled helper/Agent,
  cloud audit and full installer lifecycle passed. Full logs are retained at
  `.tmp/windows-0013-full-job.log`; source/gate readback is at
  `.tmp/windows-0013-full-run.json`.
- Tag target and Mac source archive are the documentation commit
  `47ed36e813f08da521a428267ce77856da8c9dc8`. Mac program sources, dependencies
  and build scripts are unchanged from the accepted build source `256a52d...`;
  the fresh commit comparison passed. The Windows source archive is generated
  from its exact successful build source `2866f44...`.
- Mac source archive: **165229381 bytes**, SHA-256
  `110b11cabf502307bb0ff40eb712c3c56190271d0952cb28d1c2b0bc4051372f`.
  Windows source archive: **165233716 bytes**, SHA-256
  `092a8fd7e5eff5705580e62a5b919a97e75a14f4bbd827e7208cf370bc19072d`.
- Attached platform manifest SHA-256:
  `0d1727a7e2e5ef7ac1755bea23c1fab408d2647d4aa5bbd9e4d3750a0ebed91f`.
  It records eight other assets, exact platform sources, submodule gitlinks,
  verification evidence and acceptance limits. Automatic GitHub source archives
  point to the Mac tag; the separate Windows archive is corresponding source.
- Tag-triggered Release run **37614886873** succeeded with both platform jobs
  **skipped** as intended, preserving the separately accepted artifacts.
- All nine anonymous download HEAD requests returned **HTTP 200**, and the three
  actual downloaded checksum files matched CI/local records. Direct local
  requests timed out without the existing Git proxy; reusing that proxy for
  these requests passed. No network settings were changed. Evidence:
  `.tmp/release-0013-public-download-verification.json` and
  `.tmp/release-0013-public-downloads/`.
- No installed application or terminal session was replaced or stopped by this
  release preparation. The existing untracked Mac `target` symlink remains.
  The two final Mac GUI gaps, older-system execution and real-provider/phone
  limitations remain explicit in the public notes and manifest.
