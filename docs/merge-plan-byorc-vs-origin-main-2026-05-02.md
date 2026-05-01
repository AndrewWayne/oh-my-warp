# Merge plan: `v0.4-thin-byorc-2026-05-02` ↔ `origin/main`

Date: 2026-05-02
Status: scoped, conflict surface mapped, **NOT executed**.

This doc captures the state of a deferred merge so the next session can
resume without re-running the diagnostic steps. Picking up should take
~30 minutes orientation + ~1-1.5 hr conflict resolution + ~30 min test + push.

---

## 1. Branch state at the moment this was written

| Branch | Commit | Notes |
|---|---|---|
| `main` (local) | `2553c09` | 54 commits ahead of `origin/main`, 46 behind. All BYORC + v0.4-thin-polish work lives here. |
| `v0.4-thin-byorc-2026-05-02` (local + pushed) | `2553c09` | Backup of `main` at this commit. Pushed to origin so the BYORC work survives any local rebase/reset. |
| `origin/main` | `3a95b1a` | 46 commits we don't have, all `omw_local`-gated stripping inside `vendor/warp-stripped/` (residual signup, AI assistant rebrand, About page rebuild, settings cleanup) plus a separate fork-strategy direction. |
| `origin/omw/strip-built-in-ai` | (separate branch) | Companion to one of the strips. Probably already merged into origin/main. |

`origin/main`'s direction differs in policy: in-tree fork model with manual
upstream sync (vs HEAD's tracked-snapshot model), AGPL terms in `LICENSE`
(vs HEAD's separate `LICENSE-AGPL` file).

## 2. Merge surface

`git merge --no-ff origin/main` (attempted, aborted) produces these
conflicts:

| File | Conflict blocks | Disposition recommendation |
|---|---|---|
| `LICENSE-AGPL` | modify-vs-delete | **Take origin/main's delete.** They consolidated AGPL terms into `LICENSE`; resolving as `git rm LICENSE-AGPL`. |
| `CLAUDE.md` | 1 block (§5 Project-specific rules) | **Take origin/main.** Their fork-strategy phrasing (in-tree fork, `LICENSE` not `LICENSE-AGPL`) is the canonical direction. Re-apply our `omw/wiring:` commit-tag conventions if they're not already there — check before clobbering. |
| `vendor/warp-stripped/app/Cargo.toml` | 1 block (omw_local feature definition) | **Union the features.** Theirs: `omw_local = ["omw_default", "warp_core/omw_local"]`. Ours: `omw_local = ["skip_firebase_anonymous_user", "warp_core/omw_local", "dep:omw_server", "dep:omw_remote", "dep:omw_pty", "dep:qrcode"]`. Merged: `["omw_default", "skip_firebase_anonymous_user", "warp_core/omw_local", "dep:omw_server", "dep:omw_remote", "dep:omw_pty", "dep:qrcode"]`, plus keep their new `cloud = [...]` feature line. Also reconcile the `default-features = false` removal we made in commit `66a6bfd` — that's still needed for the embedded-web-controller path. |
| `TODO.md` | 3 blocks | **Manual merge.** Theirs has the v0.0.1 preview release entries + the residual-signup-strip done items; ours has the v0.4-thin-polish progress + Gap 1 A/B/C status. Both belong. Walk each block: keep both versions where they describe non-overlapping work; pick one when they describe the same item differently. |
| `PRD.md` | 9 blocks | **Most work is here.** Both branches updated §13 phased roadmap and §8.5 fork strategy. Origin/main's version is the canonical fork-strategy direction; our v0.4-thin-polish + v0.4-thin-byorc-* commits are the canonical roadmap-progress entries. Reconcile §13 carefully — preserve origin/main's v0.0.1 preview milestone and our v0.4-thin polish status. |
| `specs/fork-strategy.md` | 6 blocks | **Take origin/main as base, then re-apply our restrip-procedure callouts.** Their version replaces "tracked snapshot" with "in-tree fork" — that's the policy decision. Our restrip procedure (§3) and omw-edits provenance (§4) may still apply with terminology updated. Read both side-by-side. |
| `specs/test-plan.md` | 3 blocks | **Manual merge.** Both updated §1.2 and §3.1 (contract test + fuzz target). |

Plus a residual: `vendor/warp-fork` directory not empty — git couldn't
`rmdir` it because we still have files there. Our HEAD removed the warp-fork
submodule reference but left the directory; origin/main also tracks this
removal. Likely resolved by `rm -rf vendor/warp-fork` after the merge
completes (review what's left in there first).

## 3. Recommended workflow when resuming

```bash
# Start clean.
cd C:/Users/andre/oh-my-warp/oh-my-warp
git checkout main
git status                             # should be clean
git fetch origin                       # already fetched in this session — no-op-ish

# Create a fresh merge branch from main (don't use the deleted attempt;
# its parent commit is the same).
git checkout -b v0.4-thin-byorc-merged-2026-05-XX main

# Attempt merge.
git merge --no-ff origin/main

# Resolve in this order — easy first, work upward:
#  1. git rm LICENSE-AGPL                  (already documented above)
#  2. CLAUDE.md                            (1 block, take origin/main)
#  3. vendor/warp-stripped/app/Cargo.toml  (1 block, union features per §2)
#  4. specs/test-plan.md                   (3 blocks)
#  5. TODO.md                              (3 blocks)
#  6. specs/fork-strategy.md               (6 blocks, careful — policy)
#  7. PRD.md                               (9 blocks, most work)

# After each: git add <file> + spot-check the resolution by running
#   /spec-consistency  (the project's slash command — checks doc cross-refs)

# When all conflicts resolved:
git status                             # should show no UU/UD/AA entries
git diff --cached | head -100          # sanity-spot-check
git commit                             # default merge commit message is fine

# Then test the tree.
/c/Users/andre/.cargo/bin/cargo test --workspace 2>&1 | tail -20
cd vendor/warp-stripped
/c/Users/andre/.cargo/bin/cargo test -p warp --features omw_local --lib omw 2>&1 | tail -10
/c/Users/andre/.cargo/bin/cargo build -p warp --bin warp-oss --features omw_local 2>&1 | tail -5
cd ../..

# All green? Push the merged branch (NOT main):
git push -u origin v0.4-thin-byorc-merged-2026-05-XX

# Decide later: open a PR onto origin/main, OR rebase main forward
# onto origin/main with the merge result, OR keep both branches
# parallel until the v0.4-thin demo is shipping cleanly.
```

## 4. Things to watch for during resolution

- **Their AGPL-headers-on-every-touched-file rule** (CLAUDE.md §5 in
  origin/main). If we adopt that, we may need to retroactively add headers
  to files we touched in our 54 commits inside `vendor/warp-stripped/`.
  Do this as a separate follow-up commit AFTER the merge lands, not as
  part of conflict resolution.
- **`omw_default` feature**. Origin/main's `omw_local` depends on
  `omw_default`. We never used that feature. After merging, build with
  `--no-default-features --features omw_local` to confirm we don't pick
  up cloud paths via `omw_default`.
- **`vendor/warp-fork/`**. Our HEAD has `~~Fork Warp into oh-my-warp/warp-fork~~ — superseded`.
  Origin/main's direction may have re-introduced or never had it. If git
  surfaces an unmerged `.gitmodules` entry, reconcile.
- **`specs/cloud-strip-plan.md`** (in their tree, not ours). Origin/main
  has this; we don't. After the merge it should appear automatically.
- **Their `assets/omw-warp-oss-icon.png`** is a binary they added. Should
  drop in cleanly.
- **Their `RELEASE_NOTES_v0.0.1.md`** is new. Drops in.
- **Their `omw_local`-gated AI/auth/billing/Drive view rewrites**. Our
  recent v0.4-thin-polish doesn't touch these files; the merge should
  auto-resolve them. If git flags a conflict on any of these, treat it
  as suspect.

## 5. Why not resolved tonight

The conflicts are policy-load-bearing (fork-strategy, AGPL location,
brand-rule wording). Resolving them while exhausted risks introducing
contradictions between PRD/TODO/CLAUDE/spec docs that take longer to
debug than to merge in the first place. The mechanical conflicts (Cargo.toml,
LICENSE-AGPL) take 5 minutes; the policy ones need ~1 hr of careful side-by-side
reading.

The backup branch `v0.4-thin-byorc-2026-05-02` is **already pushed to origin**
— no work is at risk if local main is rewound or rebased.
