# Fork Strategy

Status: Draft v0.1
Last updated: 2026-04-29
Owners: TBD

This spec defines how omw maintains its fork of the open-source Warp client (`warpdotdev/warp`, AGPL-3.0). It covers repository layout, branch model, patch series, the nightly upstream-rebase workflow, conflict triage, the criteria that trigger a switch to "Route B" (clean fork), and AGPL compliance.

It is referenced by [PRD §8.5](../PRD.md#85-fork-strategy--upstream-tracking), [PRD §12.2](../PRD.md#122-licensing), and [`specs/test-plan.md`](./test-plan.md) §B.5 (fork-rebase smoke).

---

## 0. Goals & Non-Goals

### Goals

- Stay current with upstream Warp without losing our patches.
- Make every omw-specific change *legible* — anyone can read the patch series and understand what we changed and why.
- Detect upstream-incompatibility early (nightly rebase) rather than at release time.
- Keep AGPL obligations cleanly satisfied: source available, license headers preserved, attribution intact.
- Provide a clear off-ramp ("Route B") if upstream divergence becomes too costly to track.

### Non-Goals (v1)

- Cherry-picking individual upstream commits. We rebase whole; the upstream branch is the source of truth, not a buffet.
- Automated patch acceptance from upstream. We always review.
- Mirroring upstream's CI infrastructure into our org. We run their tests against our branch in our CI; we don't replicate their pipeline.
- Maintaining feature parity with upstream's hosted features. We replace hosted, we don't mirror.

---

## 1. Repository Layout

Two repositories work together:

- **`oh-my-warp/oh-my-warp`** — this repo (the umbrella). MIT-licensed for the original `omw-*` crates. Carries the PRD, specs, CLI/server/remote crates, web controller, packaging.
- **`oh-my-warp/warp-fork`** — the actual Warp fork. AGPL-3.0. Created in v0.3.

The fork is referenced from this repo as a git submodule:

```
oh-my-warp/
  vendor/
    warp-fork/        # submodule → git@github.com:oh-my-warp/warp-fork.git
```

Rationale:

- Submodule keeps the AGPL fork distinguishable from the MIT umbrella, preserving the licensing boundary in PRD §12.2.
- Different release cadence — the fork tracks upstream nightly; the umbrella ships on its own schedule.
- The fork repo can be cloned independently by anyone exercising AGPL rights without pulling our entire toolchain.

The `vendor/warp-fork/` path is treated as **read-only** from this repo per [CLAUDE.md §5](../CLAUDE.md#5-project-specific-rules):

> **Vendor.** `vendor/warp-fork/` is a submodule pointing at the `oh-my-warp/warp-fork` sibling repo. Never edit it from this repo. Patches go upstream; rebase strategy lives in `specs/fork-strategy.md`.

Edits to fork code happen in the `warp-fork` repo, on the appropriate patch-series branch (§3).

---

## 2. Branches in `warp-fork`

| Branch | Role | Cadence | Pushable by |
|---|---|---|---|
| `upstream/master` | Read-only mirror of `warpdotdev/warp` `master`. Updated nightly. | Daily | CI bot only |
| `omw/main` | Integration branch. The HEAD that we ship. Equals upstream + all patch series applied in order. | Continuous (rebased nightly) | Maintainers, fast-forward only |
| `omw/local-mode` | Patch series for the local-server backend (`BackendMode::OmwLocal`, removal of cloud paths the local mode doesn't need). | Per-patch commits | Maintainers |
| `omw/branding` | Patch series for branding (binary rename, icon, color palette, removal of "Warp" wordmark from product surfaces). | Per-patch commits | Maintainers |
| `omw/<future>` | Future patch series, one per concern. Examples: `omw/audit-ui`, `omw/agent-panel-rewire`. | Per-patch commits | Maintainers |

`omw/main` is *not* a long-lived feature branch. It is a thin synthesis: `upstream/master` + each `omw/*` series cleanly applied. Any new local change goes onto a series branch first, then is folded into `omw/main` by the rebase pipeline.

### 2.1 Why patch series instead of one long-running branch

- **Reviewable diffs.** Each patch series is a focused, narrow change with a single concern. Reviewers can read `omw/branding` independently of `omw/local-mode`.
- **Upstream-friendly.** When a patch is suitable for upstream (a bug fix, a generic improvement), we can `git format-patch` and submit it. Generic improvements should land upstream first; we only carry truly omw-specific things.
- **Conflict isolation.** A nightly conflict is attributable to a specific series, which is owned by a specific maintainer. We don't drown in a single 5000-line "our patches" branch.

### 2.2 Patch-series naming

`omw/<concern>` where `<concern>` is a single hyphen-cased noun. Don't put version numbers in series names — series are continuous, not snapshots.

---

## 3. Patch Series Discipline

Each `omw/<concern>` branch is a sequence of commits authored by us, on top of `upstream/master`. The series is the source of truth; `omw/main` is regenerated from these series.

### 3.1 Commit conventions

- One commit per logical change. No "squash everything before merge" — we want the granularity for upstreaming.
- Commit message format:
  ```
  <concern>: <imperative summary>

  <body explaining motivation, alternatives considered, link to PRD section>

  Series: omw/<concern>
  Upstreamable: yes|no|partial
  ```
- `Upstreamable: yes` means we should periodically open a PR upstream with this commit. `partial` means a refactored version could be upstreamed; we owe a follow-up.
- Sign-off: every patch carries a Developer Certificate of Origin sign-off (`Signed-off-by:`). AGPL distribution requires clear authorship.

### 3.2 Patch series boundaries

A patch belongs to **one** series. If a change spans series, it's actually two changes — split it.

Suggested boundaries for v1:

- `omw/local-mode` — anything that adds `omw_local` Cargo feature gates, swaps the cloud client for `omw-server`, removes hosted-only code paths the local build doesn't need.
- `omw/branding` — anything that changes the user-visible product surface: binary name (`warp` → `omw`), wordmark, icon, color palette, splash screen, app menu. Per [CLAUDE.md §5](../CLAUDE.md#5-project-specific-rules), the literal string `Warp` must not appear in any product-surface code or docs touched by this series.
- `omw/agent-panel-rewire` — patches to the agent panel UI to call `omw-server` instead of the hosted agent.
- `omw/audit-ui` — patches that add the Activity view (PRD §7.4) backed by `omw-server` audit reads.

When a v0.4+ change crosses, e.g. agent panel + branding, the branding part goes to `omw/branding` and the wiring part goes to `omw/agent-panel-rewire`.

### 3.3 Generated artifacts: `git format-patch` archive

Every release tag emits a `patches/` archive in the umbrella repo:

```
oh-my-warp/
  patches/
    v0.3/
      omw-local-mode/
        0001-introduce-OmwLocal-backend-mode.patch
        0002-route-agent-panel-to-omw-server.patch
        ...
      omw-branding/
        0001-rename-binary-to-omw.patch
        ...
```

Generated by:

```
git -C vendor/warp-fork format-patch upstream/master..omw/<concern> -o ../patches/v0.3/omw-<concern>/
```

The archive is checked into `oh-my-warp/oh-my-warp` so the umbrella repo carries a full history of what we changed, version by version, even if the fork repo's git history were lost. This also helps satisfy AGPL "source available" obligations for the combined distribution (PRD §12.2).

---

## 4. Nightly Upstream Rebase

Implemented in `.github/workflows/upstream-rebase.yml` (a Phase 0 deliverable). Runs in the `warp-fork` context once that repo exists (v0.3+); until then the workflow is a documented placeholder that no-ops.

### 4.1 What the workflow does

1. Check out `oh-my-warp/warp-fork` with full history.
2. `git fetch upstream` (where `upstream` points at `warpdotdev/warp`).
3. Fast-forward `upstream/master` from `upstream/master` (rename if needed).
4. For each `omw/<concern>` branch:
   a. `git rebase upstream/master`.
   b. On conflict, abort that branch's rebase and capture (a) the conflicting upstream commit's SHA + author + message, (b) the conflicting omw commit, (c) the file list, (d) `git rerere` cache hits if any.
5. If all series rebased cleanly:
   a. Reset `omw/main` to `upstream/master`, then merge each `omw/*` series in canonical order. Force-push `omw/main` (it's a synthesized branch; force-push is normal for it).
   b. Run upstream's `cargo test --workspace` against `omw/main`.
   c. Run our integration tests (where applicable) against `omw/main`.
   d. Push `omw/main` to the fork repo if green.
6. If any series failed to rebase, OR upstream tests fail:
   a. Open or update a tracking issue on `oh-my-warp/oh-my-warp` with label `upstream-conflict` (see §5).
   b. Do **not** push `omw/main`. Yesterday's good `omw/main` continues to be the shipping head.

### 4.2 What the workflow does *not* do

- It does not auto-resolve conflicts. `git rerere` is enabled to remember past resolutions, but a fresh conflict requires a human.
- It does not skip a problematic upstream commit. If we ever decide to skip one, that decision is human, captured in the relevant patch series' commit message, and rebased explicitly.
- It does not amend commit messages or sign-offs.

### 4.3 Permissions

- The workflow runs with a fine-scoped GitHub token: `contents:write` on the `warp-fork` repo, `issues:write` on the umbrella repo. Nothing else.
- It does not have write access to `upstream/master` directly — that would be incorrect (we can only fast-forward from upstream).

### 4.4 Cadence

Nightly at 03:00 UTC (off-peak for both US and EU maintainers). On-demand via `workflow_dispatch`.

Do **not** run it on every push to `omw/<concern>` — fork-tracking is asynchronous from feature work. Local rebases happen when a maintainer chooses.

---

## 5. Conflict Triage

When the nightly rebase fails, the workflow files an `upstream-conflict` issue on `oh-my-warp/oh-my-warp`. Issues follow this template:

```
## Upstream conflict — <YYYY-MM-DD>

**Upstream commit at conflict:** `<sha>` — `<author>` — `<message>`
**omw branch failing:** `omw/<concern>`
**omw commit failing:** `<sha>` — `<message>`

**Files in conflict:**
- `path/to/file1.rs`
- `path/to/file2.rs`

**Type (best guess from CI):**
- [ ] Cosmetic (whitespace, import order)
- [ ] Refactor in upstream that touched our patch's location
- [ ] Behavioral change in upstream that overlaps with our patch
- [ ] API removal upstream that breaks our patch

**Triage owner:** @<assigned by series ownership>
**Resolution due:** within 1 week, or skip-with-rationale on the patch.
```

### 5.1 Triage classification

The reviewer classifies the conflict and resolves accordingly:

- **Cosmetic** → reapply the patch with the new whitespace/import ordering. Update `git rerere` cache.
- **Refactor in our patch's location** → rewrite the patch against the new upstream code. Document in the commit body what changed and why.
- **Behavioral change overlapping our patch** → assess whether upstream's new behavior obviates our patch (drop it!), modifies what we want (rewrite), or conflicts with our intent (carry, with explicit rationale + a tracking entry in `specs/fork-strategy.md` open questions).
- **API removal** → significant. Triggers a Route B trigger evaluation (§7).

### 5.2 Skip-with-rationale

If a single upstream commit is genuinely incompatible with our direction (rare), we may skip it with an explicit commit on `omw/<concern>` that says:

```
omw/local-mode: skip upstream commit <sha>

Upstream introduced <feature> which assumes Warp's hosted backend.
We don't have that backend; the feature is moot in local mode.

Skipping until upstream allows opt-out, or we drop the feature here.

Series: omw/local-mode
Upstreamable: no
Tracking: oh-my-warp/oh-my-warp#NNN
```

Skip commits are visible in the patch series — they don't hide. Aim for zero skips per release.

### 5.3 Slash command

The `/triage-rebase` slash command (defined in `.claude/skills/triage-rebase/`) walks the maintainer through the classification and proposes resolutions. See its skill definition for the routine.

---

## 6. Upstream Contribution

Whenever a patch in our series carries `Upstreamable: yes`, we owe upstream a PR.

- Small bugfixes — open immediately.
- Generic improvements (e.g., a refactor that's better for everyone) — open after our own patch has been stable in `omw/main` for 2 weeks (so we know it works).
- Cherry-picked into upstream → we drop the patch from our series next rebase.

This is the long-term sustainability strategy: keep our diff small. The fewer carried patches, the cheaper rebase becomes.

---

## 7. Route A vs Route B

Per [PRD §8.4](../PRD.md#84-implementation-route):

- **Route A** (default v0.1 → v1.0): keep `omw-server` as a local-mode shim; the fork talks to it via `with_local_server`. Replace cloud paths only as needed.
- **Route B** (escape hatch): clean fork of cloud paths — we stop using upstream's cloud-shaped code structure entirely; the fork carries an omw-native cloud-replacement layout instead of a shim.

### 7.1 Route B trigger conditions

Per PRD §8.4, switch to Route B if **any** of:

- We accumulate a 3rd compat bug attributable to upstream schema/API changes since the last release.
- We hit 5,000 weekly active installs.
- Warp upstream removes or breaks the local-server feature.

### 7.2 What invoking Route B means

- A new top-level patch series `omw/cloud-rewrite` is opened.
- The cloud paths in upstream are removed in this series rather than shimmed.
- `omw-server`'s GraphQL surface stops trying to be compat with upstream's schema and becomes whatever local mode actually needs.
- Maintenance cost shifts: rebases become smaller (no cloud-shaped surfaces to carry compat for); divergence becomes larger (less upstream alignment overall).

### 7.3 Pre-trigger preparation

Maintain a running list of carried patches related to cloud-shape compat. When the list hits 3, the trigger is one bug away. The release-checklist skill (`.claude/skills/release-checklist/`) checks this list at each release prep.

---

## 8. AGPL Compliance

Per [PRD §12.2](../PRD.md#122-licensing):

- The fork carries upstream's AGPL-3.0 license verbatim.
- Every modified file retains its existing AGPL header. New files added by us in the fork carry an AGPL-3.0 header attributing original authorship to the upstream maintainers and incremental authorship to the omw contributors.
- The combined distribution (umbrella + fork) is effectively AGPL. The umbrella ships [`LICENSE-AGPL`](../LICENSE-AGPL) at its root, and the fork ships its own `LICENSE`.
- Source for the combined distribution is available via:
  - The umbrella repo (this one), with submodule pointer.
  - The `warp-fork` repo directly.
  - The `patches/` archive (§3.3) reproducing every patch from upstream master, version by version.

### 8.1 AGPL "source corresponding to the version run by the user"

For binary distributions (Homebrew formula in v1.0):

- The Homebrew formula references a tagged commit in `oh-my-warp/oh-my-warp` AND a tagged commit (or submodule pointer) in `oh-my-warp/warp-fork`.
- Re-builds use the exact submodule SHA — never `master`.
- The tag carries a `patches/v<version>/` archive (§3.3) reflecting the exact patches applied to that build.

### 8.2 What this strategy does *not* do

- Provide a perpetual mirror of every upstream Warp commit. AGPL "source corresponding" applies to versions we ship, not to upstream's history.
- Relicense any fork code to MIT. AGPL boundaries are the entire reason `omw-*` lives outside the fork.

---

## 9. First Fork (v0.3 entry criteria)

Before v0.3 begins:

- [ ] `oh-my-warp/warp-fork` GitHub repo exists, AGPL-licensed, with `upstream/master` branch tracking `warpdotdev/master`.
- [ ] `omw/main` initialized as empty fast-forward from `upstream/master`.
- [ ] `omw/local-mode` and `omw/branding` series branches created (empty initially).
- [ ] `vendor/warp-fork/` submodule wired into the umbrella repo.
- [ ] `.github/workflows/upstream-rebase.yml` activated (no-op guard removed).
- [ ] First-pass branding patch landed on `omw/branding` (binary rename, splash text).
- [ ] First-pass local-mode patch landed on `omw/local-mode` (`omw_local` feature gate added; cloud paths still functional but no longer the default).

---

## 10. Open Questions

1. **Where does `git rerere` cache live** — committed alongside the workflow, or kept ephemeral and re-warmed? Probably committed (in the fork repo, `.git/rr-cache` content captured into a sibling file). Decide before v0.3.
2. **Maintainer review SLA on `upstream-conflict` issues** — 1 week is the proposed default. Review at v0.3 retro.
3. **Upstream-PR-friendly patches** — should we maintain a separate "would-be-upstream" branch that lives alongside `omw/main` to make ongoing PR opening cheaper? Possibly Beyond v1.
4. **DCO sign-off enforcement** — pre-commit hook in the fork repo, or CI check, or both? Decide when fork is created.
5. **AGPL header generator** — small script that adds the right license preamble to new files. Useful enough to write in v0.3.
6. **Submodule vs git subtree** — submodule chosen for clearer license boundary. Subtree's "no extra clone step" UX win is real; revisit if v1 onboarding feedback shows submodules are a friction point.
7. **What to do if `warpdotdev/warp` rebrands its default branch from `master`** — currently we name the mirror branch `upstream/master` regardless. The workflow handles either origin name. Documented for clarity.

---

*End of fork-strategy v0.1.*
