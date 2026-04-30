# Fork Strategy

Status: Draft v0.2 — tracked-snapshot model
Last updated: 2026-05-01
Owners: TBD

This spec defines how omw maintains its derivation of the open-source Warp client (`warpdotdev/warp`, AGPL-3.0). It covers the tracked-snapshot model (in place since 2026-04-29 with the addition of `vendor/warp-stripped/`), the restrip procedure, how omw-specific edits are tracked, the Route A → Route B escape hatch, and AGPL compliance.

It is referenced by [PRD §8.5](../PRD.md#85-fork-strategy--upstream-tracking), [PRD §12.2](../PRD.md#122-licensing), and [`specs/test-plan.md`](./test-plan.md) §B.5 (snapshot smoke).

> **History note.** An earlier draft (v0.1, 2026-04-29) committed to a submodule + patch-series + nightly-rebase model. That model was retired on 2026-05-01 in favor of the tracked-snapshot model below. The reasoning is captured in §0 ("Why a snapshot, not a patch series") and the v0.1 draft is preserved in git history for reviewers.

---

## 0. Goals & Non-Goals

### Goals

- Keep the AGPL Warp source omw is derived from **legible and reproducible**: anyone can read this repo and see exactly which upstream version we started from and what we changed.
- Make the **license boundary** between the AGPL `vendor/warp-stripped/` tree and the MIT `crates/omw-*` source files unambiguous, both for distributors and for our own code reviews.
- Keep the cost of staying current with upstream **honest about solo-maintainer reality**: deliberate, infrequent restrips with a documented procedure beat aspirational nightly automation.
- Provide a clean **escape hatch** ("Route B") if upstream divergence becomes too costly.

### Non-Goals

- **Continuous upstream tracking.** We do not run nightly rebase CI. Restrips happen on cadence (§2), not on every upstream commit.
- **Cherry-picking individual upstream commits.** When we restrip, we take a whole upstream tag — no buffet.
- **Maintaining feature parity with upstream's hosted features.** We replace hosted, we don't mirror.
- **Mirroring upstream's CI infrastructure.** We run our own CI against `vendor/warp-stripped/` after each restrip; we do not replicate their pipeline.

### Why a snapshot, not a patch series

The submodule + patch-series + nightly-rebase model in v0.1 of this spec optimized for *maximum upstream alignment with minimum drift*. That target is wrong for omw's stage:

- omw is a **solo-maintainer project**. Nightly rebase CI requires either a maintainer reviewing conflicts daily, or those conflicts queue up indefinitely. Neither is sustainable.
- The cloud-feature strip is **a one-time chunk of work**, not a per-commit operation. Patch-series-on-submodule replays that chunk every rebase — wasteful when the strip itself is rarely the source of conflicts.
- The AGPL boundary is **clearer with a tracked tree** than with a submodule + patch-series archive. A reviewer reads `git log vendor/warp-stripped/` and gets the full history; with a submodule they need to clone two repos and synthesize.
- The Route B escape hatch (clean fork of cloud paths) is **easier to invoke** when upstream is a tracked tree we can fork in-place than when it is a submodule whose patches we have to rewrite.

The cost of the snapshot model is real: we lag upstream more than we would with nightly rebase. Section 2 makes the lag policy explicit so users know what to expect.

---

## 1. Repository Layout

One repo carries everything:

```
oh-my-warp/oh-my-warp/      # this repo (umbrella + canonical Warp host)
  LICENSE                   # MIT — covers original omw-* crates and apps/*
  LICENSE-AGPL              # combined-distribution AGPL notice
  Cargo.toml                # outer workspace (MIT crates only)
  crates/                   # MIT — omw-cli, omw-server, omw-remote, omw-pty, ...
  apps/                     # MIT — web-controller, omw-agent (TS)
  vendor/
    warp-stripped/          # AGPL-3.0 — tracked snapshot of upstream + our local-mode strip
      Cargo.toml            # inner workspace (separate from outer)
      LICENSE-AGPL          # upstream AGPL license, verbatim
      LICENSE-MIT           # upstream MIT license for portions originally MIT
      crates/               # upstream Warp crates, with cloud paths stripped or feature-gated
      app/                  # main Warp binary, builds as warp-oss (will rebrand to omw)
      OMW_LOCAL_BUILD.md    # build instructions + strip provenance
    pi-mono/                # submodule — pi-agent monorepo (deferred until v0.2)
    forge-code/             # submodule — reference for ACP work
    warp-fork/              # submodule — DORMANT, retained as Route B fallback only (§6)
```

### 1.1 Two workspaces, one repo

The outer Cargo workspace (`Cargo.toml` at repo root) and the inner workspace (`vendor/warp-stripped/Cargo.toml`) are **separate workspaces**. They do not share `[workspace.dependencies]` blocks; they do not share `target/`. The two workspaces are built independently:

- Outer: `cargo build --workspace` from repo root.
- Inner: `cargo build -p warp --bin warp-oss --features omw_local` from `vendor/warp-stripped/`.

Cross-workspace dependencies are by `path = "../../../crates/<name>"` references. The canonical example is `omw-server`, embedded into `vendor/warp-stripped/app/Cargo.toml` as a path dep so the Warp-derived binary boots an in-process omw-server (PRD §8.3).

### 1.2 Why "vendor/warp-stripped/" not "vendor/warp/"

The directory name announces what's inside: a stripped version of upstream Warp, not a verbatim mirror. Anyone reading the tree knows it has been modified. CLAUDE.md §5 permits the literal `Warp` (capitalized) inside `vendor/warp-stripped/` paths; the brand rule applies to product-surface code only.

### 1.3 Edits to `vendor/warp-stripped/`

Edits are normal source-control operations (regular commits, regular PRs). There is no read-only rule like the v0.1 spec had for the submodule.

However, edits should follow the **omw-specific-edits provenance** convention (§4) so the next restrip can identify and re-apply them.

---

## 2. Snapshot Lifecycle

### 2.1 Cadence

We restrip `vendor/warp-stripped/` from upstream:

- **Every 2 upstream minor releases**, OR
- **On any security advisory affecting Warp's open-source crates**, OR
- **On demand** when a known-broken upstream surface is fixed and we want it.

A "minor release" means upstream's `v0.YYYY.MM.DD.HH.MM.preview_NN` cadence is irrelevant; we treat any version that ships a new public Warp release as a minor for cadence purposes. In practice this means roughly quarterly restrips.

The cadence is a guideline, not a hard rule. Restrips are deliberate work; we trade some upstream lag for predictability.

### 2.2 Pinning

The current upstream snapshot point is recorded in two places:

- `vendor/warp-stripped/OMW_LOCAL_BUILD.md` carries a header line: `Snapshot of: warpdotdev/warp@<sha> (<tag>) restripped <YYYY-MM-DD>`.
- This spec's §8 ("Snapshot Promotion History") logs every restrip with its date, upstream commit, and a one-line summary of what changed.

The `vendor/warp-fork/` submodule (when populated, see §6) is pinned to the **same SHA** as the snapshot point so a Route B migration can diff confidently.

### 2.3 Lag policy

We accept lagging upstream by up to two minor versions. If upstream ships a critical fix, the restrip happens out-of-cadence. If upstream's new minor breaks something we rely on, we pin to the previous minor for the next restrip cycle and document why.

---

## 3. Restrip Procedure

This is the documented procedure. It is run by hand; no automation runs it for us. Allocate a half-day.

### 3.1 Preconditions

- Working tree clean.
- The current `vendor/warp-stripped/` builds and runs (`cargo run -p warp --bin warp-oss --features omw_local` succeeds).
- An "omw-edits-since-last-snapshot" diff is available — generated by `git log vendor/warp-stripped/` since the previous snapshot date.

### 3.2 Step-by-step

1. **Pick the upstream tag.**
   ```
   git ls-remote --tags https://github.com/warpdotdev/warp.git | grep -v '\^{}' | tail -10
   ```
   Choose the tag we are promoting to. Record its SHA.

2. **Fetch the upstream tree at that tag.**
   ```
   mkdir -p .tmp/restrip
   git clone --depth 1 --branch <tag> https://github.com/warpdotdev/warp.git .tmp/restrip/upstream
   ```

3. **Capture the strip diff from the current snapshot.**
   ```
   git -C .tmp/restrip/upstream rev-parse HEAD > .tmp/restrip/upstream-sha
   diff -ruN .tmp/restrip/upstream/ vendor/warp-stripped/ > .tmp/restrip/strip.diff
   ```
   This `strip.diff` represents the cumulative "what omw removed/changed from upstream since this tag." It is the omw-specific-edits set we need to re-apply.

4. **Replace the tree.**
   ```
   git rm -rf vendor/warp-stripped/
   cp -r .tmp/restrip/upstream/ vendor/warp-stripped/
   git add vendor/warp-stripped/
   git commit -m "vendor: snapshot upstream Warp <tag> (pre-restrip)"
   ```
   This commit is *unstripped* — the next commit applies the strip.

5. **Re-apply the strip.**
   The strip is a categorical removal of:
   - `cloud_*` modules
   - `account_*` and `auth_*` modules tied to Warp's hosted login
   - Drive sync paths
   - Oz hosted agent surfaces
   - Hosted workflow catalog (replace with empty local stub)
   - Telemetry exporters pointing to Warp's collectors

   The historical strip applied to the v0.1 draft is documented in `vendor/warp-stripped/OMW_LOCAL_BUILD.md` and in the `strip.diff` from step 3. Use that diff as a guide; it will not apply cleanly (upstream code has moved), but it tells us *what* to look for.

   Re-apply by:
   - Checking out the previous snapshot's tree from git history at `vendor/warp-stripped/`.
   - Diffing against the new upstream's same paths.
   - Re-creating the strip in the new tree, file by file.
   - When upstream has moved a strip target, find its new home and apply the strip there.

6. **Re-apply omw-specific edits.**
   Use the "omw-edits-since-last-snapshot" diff from preconditions. For each edit:
   - If the file still exists at the same path: re-apply.
   - If upstream has moved/renamed: re-apply at the new location, document the move in the commit message.
   - If upstream's behavior change has obviated the edit: drop, document in the commit message.

7. **Update `omw_local` feature gating.**
   The `omw_local` Cargo feature is the canonical opt-in for omw-specific behavior. New cloud surfaces in upstream MUST be feature-gated under `omw_local` rather than removed outright when feasible — this keeps the `strip.diff` smaller next time.

8. **Build verification.**
   ```
   cd vendor/warp-stripped
   cargo check -p warp --bin warp-oss --features omw_local
   cargo build -p warp --bin warp-oss --features omw_local
   cargo run -p warp --bin warp-oss --features omw_local
   ```
   Run the resulting binary, smoke-test the surfaces we know we need (terminal, agent panel placeholder, settings).

9. **Update `OMW_LOCAL_BUILD.md`** snapshot header to the new SHA + tag + date.

10. **Update this spec's §8** ("Snapshot Promotion History") with a new row.

11. **Commit and PR.**
    ```
    git commit -m "vendor: restrip warp-stripped to <tag>"
    ```
    PR carries:
    - The unstripped commit (step 4) and the restripped commit (step 11) as separate, reviewable commits.
    - A summary of which omw-specific edits were re-applied, dropped, or moved.
    - A summary of new upstream cloud surfaces stripped.

12. **Cleanup.**
    ```
    rm -rf .tmp/restrip
    ```
    (Or keep it for the duration of review.)

### 3.3 What the procedure does NOT do

- It does not auto-resolve conflicts. Each conflict is reasoned about and resolved by the maintainer.
- It does not skip a problematic upstream commit silently. If we ever pin to a previous tag, that's documented in §8.
- It does not amend `LICENSE-AGPL` or attribution headers. Those come over verbatim from upstream and stay.

---

## 4. omw-Specific Edits Provenance

Every edit to `vendor/warp-stripped/` carries one of three commit-message tags so the next restrip can categorize it:

- `omw/local-mode:` — adds `omw_local` feature gates, swaps cloud client for omw-server, removes hosted-only paths.
- `omw/branding:` — changes the user-visible product surface (binary name, wordmark, icon, palette). Per CLAUDE.md §5 the literal string `Warp` must not appear in any file touched by this tag.
- `omw/wiring:` — connects omw outer crates (`omw-server`, `omw-remote`, `omw-pty`) into Warp's main app. Path-dep additions to `vendor/warp-stripped/app/Cargo.toml`, callsite changes in `app/src/main.rs`, etc.

The tag goes in the commit subject:

```
omw/branding: rename binary from warp-oss to omw

Series-tag: omw/branding
Upstreamable: no
PRD: §12.1 brand rule
```

Or:

```
omw/wiring: spawn embedded omw-server task on omw_local feature

Series-tag: omw/wiring
Upstreamable: no
PRD: §8.3 component ownership
```

`Series-tag:` and `Upstreamable:` trailers are optional but encouraged — they help the next restrip (§3.2 step 6) classify each edit at a glance.

### 4.1 Why no patch-series branches

The v0.1 spec proposed `omw/local-mode`, `omw/branding`, etc. as separate branches in a sibling repo. The new model achieves the same auditability via commit-message tags on a single tracked tree. This trades branch-per-concern isolation for repo-simplicity; the tradeoff is acceptable because the *legibility* (what changed and why) is preserved.

When a restrip needs to triage edits, `git log --grep '^omw/branding:' vendor/warp-stripped/` reproduces what a `omw/branding` branch would have provided.

---

## 5. Upstream Contribution

When an edit to `vendor/warp-stripped/` is generic enough to benefit upstream — typically a bug fix or a refactor — we owe upstream a PR. The trailer `Upstreamable: yes` marks these.

- For each `Upstreamable: yes` commit, open a PR against `warpdotdev/warp` within 2 weeks of merging it locally.
- If accepted upstream, the next restrip drops the local commit (the change is now in upstream).
- If rejected or stalled, the local commit stays; document the upstream PR URL or rejection rationale in a follow-up commit.

This is the long-term sustainability strategy. The smaller our cumulative `strip.diff`, the cheaper restrips become.

---

## 6. Route A vs Route B

Per [PRD §8.4](../PRD.md#84-implementation-route):

- **Route A** (default v0.1 → v1.0): keep upstream's cloud-shaped code structure, gate it off via `omw_local` feature, and provide replacements (`omw-server`) for the surfaces we need. The `strip.diff` is small and trackable.
- **Route B** (escape hatch): fork the cloud paths cleanly. Upstream's cloud-shaped code structure is replaced with an omw-native layout in `vendor/warp-stripped/`.

### 6.1 Route B trigger conditions

Switch to Route B if **any** of:

- We accumulate a 3rd compat bug attributable to upstream schema/API changes since the last release.
- We hit 5,000 weekly active installs (cost of breakage outweighs cost of divergence).
- Warp upstream removes or breaks the local-server feature flag we depend on.

### 6.2 What invoking Route B means

Under the snapshot model, Route B is implemented as a **fork-in-place** rather than a separate branch:

- The current `vendor/warp-stripped/` tree continues. A new commit (or sequence of commits) tagged `omw/cloud-rewrite:` removes upstream's cloud paths entirely (rather than feature-gating them).
- Restrip procedure §3 still applies, but now `omw/cloud-rewrite:` edits expand on each restrip rather than being feature-gated.
- The dormant `vendor/warp-fork/` submodule (currently a submodule pointer at `warpdotdev/warp` itself, see Git submodule config) becomes load-bearing again only if Route B requires us to publish a separate AGPL fork repo for distribution clarity.

### 6.3 Pre-trigger preparation

Maintain a running list of carried edits with `Series-tag: omw/local-mode` that exist solely as compat shims for upstream's cloud-shape. When the list grows past 5 such shims OR we hit a 3rd compat bug, evaluate Route B.

The `release-checklist` skill (`.claude/skills/release-checklist/`) reviews this list at each release prep.

---

## 7. AGPL Compliance

Per [PRD §12.2](../PRD.md#122-licensing):

- `vendor/warp-stripped/LICENSE-AGPL` carries upstream's AGPL-3.0 license verbatim.
- `vendor/warp-stripped/LICENSE-MIT` carries upstream's MIT license for the portions that were originally MIT-licensed in upstream.
- Every modified file inside `vendor/warp-stripped/` retains its existing license header.
- New files added by us inside `vendor/warp-stripped/` carry an AGPL-3.0 header attributing original authorship to the upstream maintainers and incremental authorship to the omw contributors. (Header generator script is an open question — see §9.)
- The combined distribution (`oh-my-warp/oh-my-warp` repo as a whole) is AGPL because it bundles the AGPL `vendor/warp-stripped/` tree. The umbrella's `LICENSE` (MIT) covers original `omw-*` source files but NOT the combined binary.
- The repo root `LICENSE-AGPL` documents the combined-distribution AGPL terms.

### 7.1 "Source corresponding to the version run by the user"

For binary distributions (Homebrew formula in v1.0):

- The Homebrew formula references a tagged commit in `oh-my-warp/oh-my-warp`.
- That tag, by virtue of containing `vendor/warp-stripped/` in tree, **contains the full corresponding source**. There is no submodule pointer to chase. AGPL "source available" is satisfied by the repo tag alone.
- This is the primary AGPL-compliance advantage of the snapshot model over the v0.1 submodule model.

### 7.2 What this strategy does *not* do

- Provide a perpetual mirror of every upstream Warp commit. AGPL "source corresponding" applies to versions we ship, not to upstream's history.
- Relicense any `vendor/warp-stripped/` code to MIT. AGPL boundaries are absolute.
- Remove upstream's `Warp Team <dev@warp.dev>` author attribution from upstream-authored files.

---

## 8. Snapshot Promotion History

| Date | Upstream tag | Upstream SHA | Restripped by | Summary |
|------|--------------|--------------|----------------|---------|
| 2026-04-29 | `v0.2026.04.29.08.56.preview_00` | `d0f045c0` | Shenhao Miao | Initial promotion. Manual strip of cloud, account, billing, Drive, Oz, hosted workflow catalog. `omw_local` feature added. Binary renamed `warp-oss` (interim; final rebrand to `omw` deferred to v0.3 polish). |

Future restrips append a row.

---

## 9. Open Questions

1. **AGPL header generator** — small script that adds the AGPL preamble to new files inside `vendor/warp-stripped/`. Useful enough to write before the next restrip.
2. **Restrip CI signal** — should we add a CI workflow that, on a fortnightly schedule, polls upstream's tags and opens an issue when we drift past 2 minor versions? Lightweight automation that doesn't try to auto-rebase. Lean: yes, post-v1.0.
3. **`omw_local` feature audit** — periodic review that all cloud paths we depend on are either gated or replaced. Decide cadence (probably per-restrip).
4. **Re-strip log location** — `OMW_LOCAL_BUILD.md` currently mixes build instructions with snapshot metadata. Split into `OMW_LOCAL_BUILD.md` (instructions) and `OMW_SNAPSHOT.md` (metadata) at the next restrip.
5. **Route B evaluation cadence** — evaluate at every release prep via the release-checklist skill, or only when triggers fire? Lean: every release prep.
6. **`vendor/warp-fork/` submodule retention** — currently dormant, retained as Route B fallback. Decide at v1.0: keep or delete the `.gitmodules` entry.

---

*End of fork-strategy v0.2.*
