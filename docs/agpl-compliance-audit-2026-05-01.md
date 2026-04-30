# AGPL Compliance Audit — 2026-05-01

**Scope.** Verify that the recent move to the tracked-snapshot fork model (`specs/fork-strategy.md` v0.2) does not introduce an AGPL-3.0 compliance regression for the umbrella repo `oh-my-warp/oh-my-warp`.

**Auditor.** Claude Code (autonomous, on behalf of project owner). This is an engineering-side review, not a legal-side review. Per PRD §12 disclaimer, a lawyer must also review before public binary distribution.

**Result.** Pass with two corrections applied during the audit (see §4).

---

## 1. License files present

| File | Purpose | Status |
|------|---------|--------|
| `/LICENSE` | MIT — covers original `crates/omw-*` and `apps/*` source files | ✓ present, contains MIT terms with copyright "(c) 2026 Shenhao Miao" |
| `/LICENSE-AGPL` | Combined-distribution AGPL notice + AGPL-3.0 verbatim text | ✓ present, 718 lines (preamble + AGPL verbatim) |
| `vendor/warp-stripped/LICENSE-AGPL` | Upstream's AGPL-3.0 license, verbatim | ✓ present, opens with `Copyright (C) 2020-2026 Denver Technologies, Inc.` |
| `vendor/warp-stripped/LICENSE-MIT` | Upstream's MIT license for portions originally MIT in upstream | ✓ present |

All four required files are present and load without truncation.

## 2. Combined-distribution language

The root `LICENSE-AGPL` preamble (lines 1–55) explains:

- The repo ships in two parts: MIT'd `omw-*` crates and the AGPL-3.0 `vendor/warp-stripped/` tree.
- Combined-distribution binaries (e.g., the v1.0 Homebrew formula) are governed by AGPL-3.0.
- Per AGPL §13 (Remote Network Interaction), users interacting with such a build remotely are entitled to the Corresponding Source.
- The MIT'd `omw-*` crates remain independently usable under MIT.
- Source for the combined distribution is available by cloning this repo at any release tag (no submodule chase under the tracked-snapshot model).

**Verdict.** Combined-distribution language is present and accurate after the corrections in §4.

## 3. Per-file source headers

AGPL-3.0 §5 ("Conveying Modified Source Versions") requires that modified source versions carry "prominent notices stating that you modified it, and giving a relevant date" plus "prominent notices stating that it is released under this License." Per-file headers are one common way to satisfy this; license files at the root of the source tree are another.

Sample inspection of `vendor/warp-stripped/`:

- `vendor/warp-stripped/app/src/ai/active_agent_views_model.rs` — no per-file header (sample-checked first 20 lines).
- `vendor/warp-stripped/crates/command/src/lib.rs` — no per-file header (sample-checked first 20 lines).
- `vendor/warp-stripped/crates/ai/src/api_keys.rs` — no per-file header.

This matches upstream `warpdotdev/warp`'s convention: upstream does not carry per-file headers either. The AGPL-3.0 obligation is satisfied at the tree level by `vendor/warp-stripped/LICENSE-AGPL` and at the umbrella level by `/LICENSE-AGPL`.

**Note.** When omw adds *new files* inside `vendor/warp-stripped/` (e.g., the `omw-server` path-dep wiring patch in v0.3), those files SHOULD carry an AGPL-3.0 header explicitly stating that they are derived/added work, with omw author attribution and a date. This is more conservative than upstream's convention and reduces ambiguity for downstream redistributors.

**Action item.** Write a small AGPL-header generator (open question Q1 in `specs/fork-strategy.md` §9) before the next restrip OR before adding the first new file in `vendor/warp-stripped/`.

## 4. Corrections applied during this audit

### 4.1 Root `LICENSE-AGPL` referenced the old submodule model

Found: lines 15–19 and 36–43 of `LICENSE-AGPL` referenced `vendor/warp-fork/` as a submodule and a "sibling repository `oh-my-warp/warp-fork`," and pointed source-availability at a `patches/v<version>/` archive that no longer exists under the snapshot model.

Fixed in this PR:
- Replaced the `vendor/warp-fork/` paragraph with a `vendor/warp-stripped/` paragraph that cites the new model and notes the dormant submodule's Route B-only status.
- Replaced the source-availability list with a single-clone-of-this-repo statement, citing the snapshot promotion history in `specs/fork-strategy.md` §8.

### 4.2 No regression vs. the v0.1 spec's intent

The v0.1 fork-strategy spec used a submodule + patch-series + nightly-rebase model. AGPL "source corresponding to the version run by the user" required a clone of *both* the umbrella repo and the `oh-my-warp/warp-fork` submodule, plus consultation of the `patches/v<version>/` archive.

The v0.2 tracked-snapshot model satisfies the same obligation more directly: a clone of *this repo* at any release tag yields the full corresponding source. This is, if anything, a strengthening of source availability, not a weakening.

## 5. Trademark / brand check

PRD §12.3 and CLAUDE.md §5 forbid the literal `Warp` (capitalized) on omw product surfaces, with explicit exceptions for:
- File paths inside `vendor/warp-stripped/` (permitted).
- `LICENSE-AGPL` (permitted; required for accurate attribution).
- Source-attribution comments and the `oh-my-warp` codename (permitted).

Spot check of files modified in Phase A:
- `specs/fork-strategy.md` v0.2 — uses `Warp` capitalized only in factual upstream references (`warpdotdev/warp`, "upstream Warp") and license context. No product-surface usage.
- `PRD.md` — uses `Warp` capitalized in factual references (Warp upstream, Warp Drive, Warp's hosted features). No product-surface usage.
- `LICENSE-AGPL` — uses `Warp` for trademark notice and factual upstream attribution (permitted).
- `CLAUDE.md` — the §5 brand rule itself names `Warp` (permitted; it's the rule).

**Verdict.** No new product-surface `Warp` usage introduced.

## 6. Open items for future audits

1. **AGPL header generator** (Q1 in `specs/fork-strategy.md` §9). Should be written before the next restrip or before adding the first new file in `vendor/warp-stripped/`.
2. **Lawyer-side review** of combined-distribution language pre-launch (PRD §12 disclaimer).
3. **Restrip-cadence policy** — formalize what "every 2 minor versions" means once we observe upstream's release cadence over 6 months.
4. **`vendor/warp-fork/` submodule lifecycle** — decide at v1.0 whether to retain or delete the dormant submodule entry in `.gitmodules`. Currently retained per `specs/fork-strategy.md` §6.3 as a Route B fallback.

## 7. Conclusion

The umbrella repo's AGPL compliance posture under the tracked-snapshot model is materially equivalent to or stronger than the previous submodule model. Two stale references in `LICENSE-AGPL` were corrected in this PR. No new compliance gaps were identified.

The repo is safe to continue feature work (v0.4-thin BYORC implementation) on its current AGPL footing, subject to the open items in §6 and the standard PRD §12 lawyer-review obligation pre-launch.

---

*End of audit, 2026-05-01.*
