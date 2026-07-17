#!/usr/bin/env bash
# Enforce specs/fork-strategy.md §3: every omw-authored file inside the in-tree
# Warp fork carries the AGPL-3.0 incremental-authorship header. omw's fork files
# live under an `omw/` module directory (e.g. vendor/warp-stripped/app/src/omw/)
# or are named `omw_*.rs`; upstream Warp files keep their own provenance and are
# out of scope. A few omw-authored files predate that convention or aren't Rust
# and are listed by path below.
#
# Backstop for the High-severity finding in issue #6: new omw fork files must not
# ship without the header.

set -uo pipefail

spdx="SPDX-License-Identifier: AGPL-3.0-only"
expected="// $spdx"
missing=0
count=0

# omw-authored .rs files that predate the naming convention above and so match
# neither find pattern. Keep this list short: new fork files should live under
# an `omw/` directory or be named `omw_*.rs` instead of being added here.
extra_paths=(
  vendor/warp-stripped/app/src/autoupdate/oss.rs
)

# .rs files carry the header block as the very first thing, so line 1 is the
# SPDX identifier.
while IFS= read -r -d '' f; do
  count=$((count + 1))
  if [ "$(head -1 "$f")" != "$expected" ]; then
    echo "missing AGPL-3.0 header: $f"
    missing=1
  fi
done < <(
  {
    find vendor/warp-stripped -path '*/omw/*' -name '*.rs' -print0
    find vendor/warp-stripped -name 'omw_*.rs' -print0
    printf '%s\0' "${extra_paths[@]}"
  } | sort -z -u
)

# omw-authored non-.rs files: the header uses the file's native comment syntax
# (# for shell/PowerShell, <!-- --> for Markdown) and sits below a required first
# line (shebang, #requires, or the document title), so match the SPDX identifier
# near the top rather than on line 1.
nonrs_paths=(
  vendor/warp-stripped/scripts/audit-no-cloud.sh
  vendor/warp-stripped/run-omw-local.ps1
  vendor/warp-stripped/OMW_LOCAL_BUILD.md
)

for f in "${nonrs_paths[@]}"; do
  count=$((count + 1))
  if ! grep -qF "$spdx" <(head -n 15 "$f"); then
    echo "missing AGPL-3.0 header: $f"
    missing=1
  fi
done

if [ "$missing" -ne 0 ]; then
  echo
  echo "omw-authored files under vendor/warp-stripped/ must carry the AGPL-3.0"
  echo "incremental-authorship header near the top (specs/fork-strategy.md §3),"
  echo "below any shebang, #requires, or document title. The SPDX identifier:"
  echo "  $spdx"
  echo "Comment it with // (Rust), # (shell/PowerShell), or <!-- --> (Markdown)."
  exit 1
fi

echo "vendor omw AGPL headers: OK ($count files)"
