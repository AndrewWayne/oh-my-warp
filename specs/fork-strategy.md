# Fork Strategy

Status: Draft v0.2 (2026-05-01 — in-tree rewrite)
Last updated: 2026-05-01
Owners: TBD

This spec defines how omw maintains its in-tree fork of the open-source Warp client (`warpdotdev/warp`, AGPL-3.0). It covers the location of the fork, the manual upstream-sync procedure, AGPL compliance posture, and the criteria that trigger a switch to "Route B" (clean fork).

It is referenced by [PRD §8.5](../PRD.md#85-fork-strategy--upstream-tracking), [PRD §12.2](../PRD.md#122-licensing), and [`specs/test-plan.md`](./test-plan.md) §B.5.

> **History.** v0.1 of this spec proposed a sibling repo (`oh-my-warp/warp-fork`) mounted via submodule at `vendor/warp-fork/`, with patch-series branches and a nightly upstream-rebase CI. That architecture was retired on 2026-05-01 in favor of the simpler in-tree model documented below. The umbrella repo absorbed the AGPL fork, and the umbrella relicensed from MIT to AGPL-3.0 as a consequence (see PRD §12.2).

---

## 0. Goals & Non-Goals

### Goals

- Stay current with upstream Warp at maintainer-chosen cadence without losing our patches.
- Make every omw-specific change *legible* — reading `git log -- vendor/warp-stripped/` shows what we changed and why.
- Keep AGPL obligations cleanly satisfied: source available at the umbrella repo, license headers preserved, attribution intact.
- Provide a clear off-ramp ("Route B") if upstream divergence becomes too costly.

### Non-Goals (v1)

- Cherry-picking individual upstream commits. We sync whole-tree from upstream, then re-apply our diff on top — `warpdotdev/master` is the input, not a buffet.
- Automated upstream-tracking CI. We choose when to sync.
- Patch-series management. There are no `omw/local-mode`, `omw/branding`, etc. branches — just regular commits to `vendor/warp-stripped/` on regular feature branches.
- A separate AGPL repository.

---

## 1. Repository Layout

The fork lives at `vendor/warp-stripped/` inside this umbrella repo. It is **not** a submodule and **not** a git tree of its own — it is regular files committed into the umbrella, with upstream Warp's AGPL-3.0 headers preserved per file.

```
oh-my-warp/                 (this repo, AGPL-3.0)
└── vendor/
    ├── warp-stripped/      ← in-tree Warp fork
    ├── pi-mono/            (submodule — pi-agent kernel)
    └── forge-code/         (submodule)
```

Edits to fork code happen directly in `vendor/warp-stripped/`. Per [CLAUDE.md §5](../CLAUDE.md#5-project-specific-rules), preserve the AGPL header on every file you touch.

---

## 2. Upstream Sync Procedure

Manual, at maintainer discretion. Recommended cadence: monthly during active v0.3 work; less frequent later.

### 2.1 The procedure

1. Clone (or update) a pristine `warpdotdev/warp` checkout in a scratch directory **outside** this repo, e.g. `~/scratch/warp-upstream/`. Note the upstream commit SHA.
2. From the umbrella repo root, run a directory-level sync:
   ```sh
   rsync -av --delete \
     --exclude='.git' \
     --exclude='target/' \
     ~/scratch/warp-upstream/ \
     vendor/warp-stripped/
   ```
3. Inspect `git diff -- vendor/warp-stripped/`. Three things should be visible:
   - Upstream changes since the previous sync (new files, modified lines).
   - Our omw modifications **un-applied** (because rsync overwrote them with upstream).
4. Re-apply our omw modifications. The simplest way: revert the parts of the diff that correspond to our prior work, keeping only the genuinely new upstream content. Use `git diff` and `git restore -p` to navigate.
5. Build with `--features omw_local` to confirm the strip still compiles.
6. Run `vendor/warp-stripped/scripts/audit-no-cloud.sh` to confirm no cloud strings leaked back in.
7. Commit with a message of the form:
   ```
   vendor: sync warp-stripped to upstream <short-sha>

   Synced vendor/warp-stripped/ to warpdotdev/warp@<sha>
   (<commit message subject of upstream HEAD>).

   omw modifications re-applied:
     - omw_local Cargo feature gates (X files)
     - audit-no-cloud script
     - <other in-tree omw changes>

   Build verified: cargo build -p warp --bin warp-oss --features omw_local.
   Audit verified: scripts/audit-no-cloud.sh OK.
   ```

### 2.2 What this procedure does *not* do

- It does not preserve upstream's per-commit history inside this repo. The umbrella's `git log` shows our sync commits as squashes of upstream-since-last-sync. Upstream's full history remains available at `warpdotdev/warp` for anyone who needs it.
- It does not auto-resolve conflicts between upstream and our omw modifications. Conflicts surface in step 4 as places where the rsync overwrote our edits; the maintainer decides per location.
- It does not run on a schedule. There is no nightly CI for this.

### 2.3 Why rsync rather than `git subtree`/`git read-tree`

`git subtree` preserves upstream history at the cost of significant complexity (history splits, custom merge strategies, fragile when upstream rewrites). `rsync` loses upstream's per-commit history but the umbrella repo is small, the diff is auditable, and the procedure is a few lines of shell. For omw's small-team in-tree fork, rsync wins on simplicity.

---

## 3. AGPL Compliance

The umbrella repo is AGPL-3.0 (see [PRD §12.2](../PRD.md#122-licensing)). Compliance posture:

- The umbrella `LICENSE` file contains the verbatim GNU AGPL-3.0 text.
- Files in `vendor/warp-stripped/` retain their upstream AGPL headers verbatim. New files added by omw inside `vendor/warp-stripped/` get an AGPL-3.0 header attributing original authorship to upstream Warp maintainers and incremental authorship to omw contributors.
- Original `omw-*` files (in `crates/`, `apps/`) carry an AGPL-3.0 header attributing authorship to omw contributors.
- Source for any released binary is the umbrella repo at the corresponding tag. Anyone exercising AGPL "Corresponding Source" rights gets the entire umbrella, including `vendor/warp-stripped/` at that tag.

### 3.1 What this strategy does *not* do

- Provide a perpetual mirror of every upstream Warp commit. AGPL "source corresponding" applies to versions we ship, not to upstream's history.
- Maintain MIT-licensed code anywhere in the umbrella. The earlier MIT umbrella + AGPL submodule split was retired on 2026-05-01.

---

## 4. Route A vs Route B

Per [PRD §8.4](../PRD.md#84-implementation-route):

- **Route A** (default v0.1 → v1.0): keep `omw-server` as a local-mode shim; the in-tree fork talks to it via `with_local_server`. Replace cloud paths only as needed.
- **Route B** (escape hatch): clean rewrite of the cloud-shaped surfaces of the fork — we stop carrying upstream's cloud-shaped code structure and rewrite those surfaces to omw-native shapes inside `vendor/warp-stripped/`.

### 4.1 Route B trigger conditions

Per PRD §8.4, switch to Route B if **any** of:

- We accumulate a 3rd compat bug attributable to upstream schema/API changes since the last release.
- We hit 5,000 weekly active installs.
- Warp upstream removes or breaks the local-server feature.

### 4.2 Pre-trigger preparation

Maintain a running tally of carried compat patches in this spec's revisions log (or a sidecar file) when the count starts to bite. The release checklist (`.claude/skills/release-checklist/`) checks this list at each release prep.

---

## 5. v0.3 Entry Criteria

Before v0.3 closes:

- [x] `vendor/warp-stripped/` exists and builds with `--features omw_local` (cloud feature still default-on).
- [x] `omw_local` feature gates AI/cloud UI surfaces at the dispatcher level; cloud-only crates are marked `optional = true` and grouped under a `cloud` Cargo feature.
- [ ] **Cloud-strip cascade complete:** `cargo build -p warp --bin warp-oss --no-default-features --features omw_local` compiles cleanly; `scripts/audit-no-cloud.sh` reports zero forbidden hostnames in the resulting binary. Plan: [`specs/cloud-strip-plan.md`](./cloud-strip-plan.md).
- [ ] First branding pass landed (binary rename, splash, color palette).
- [ ] `omw-server` minimal surface boots the client.
- [ ] Agent panel routed through `omw-server` → `omw-agent`.

The first three items are partially complete as of branch `omw/strip-built-in-ai` (2026-05-01).

---

## 6. Open Questions

1. **Sync cadence policy.** Monthly during v0.3, then quarterly? Decided ad-hoc by maintainer for now.
2. **AGPL header generator** — small script that adds the right license preamble to new omw files inside `vendor/warp-stripped/`. Useful enough to write in v0.3.
3. **Upstream-PR-friendly bug fixes.** When we discover an upstream bug while working in `vendor/warp-stripped/`, we should fix it upstream first and pull the fix down on the next sync. No formal pipeline; just discipline.

---

*End of fork-strategy v0.2.*
