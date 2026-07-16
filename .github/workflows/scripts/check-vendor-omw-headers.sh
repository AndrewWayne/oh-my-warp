#!/usr/bin/env bash
# Enforce specs/fork-strategy.md §3: every omw-authored file inside the in-tree
# Warp fork carries the AGPL-3.0 incremental-authorship header. omw's fork files
# live under an `omw/` module directory (e.g. vendor/warp-stripped/app/src/omw/)
# or are named `omw_*.rs`; upstream Warp files keep their own provenance and are
# out of scope.
#
# Backstop for the High-severity finding in issue #6: new omw fork files must not
# ship without the header.

set -uo pipefail

expected="// SPDX-License-Identifier: AGPL-3.0-only"
missing=0
count=0

# omw-authored files that predate the naming convention above and so match
# neither find pattern. Keep this list short: new fork files should live under
# an `omw/` directory or be named `omw_*.rs` instead of being added here.
extra_paths=(
  vendor/warp-stripped/app/src/autoupdate/oss.rs
)

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

if [ "$missing" -ne 0 ]; then
  echo
  echo "omw-authored files under vendor/warp-stripped/ must begin with the"
  echo "AGPL-3.0 incremental-authorship header (specs/fork-strategy.md §3)."
  echo "Prepend this as the first line, then the full header block:"
  echo "  $expected"
  exit 1
fi

echo "vendor omw AGPL headers: OK ($count files)"
