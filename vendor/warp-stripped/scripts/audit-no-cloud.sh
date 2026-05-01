#!/usr/bin/env bash
# audit-no-cloud.sh — verify a warp-oss build has no Warp-cloud or firebase strings.
#
# Usage: audit-no-cloud.sh [path/to/warp-oss]
# Defaults to vendor/warp-stripped/target/debug/warp-oss relative to repo root.
# Exits 0 if all forbidden hostnames have zero hits; exits 1 otherwise.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DEFAULT_BIN="${SCRIPT_DIR}/../target/debug/warp-oss"
BIN="${1:-$DEFAULT_BIN}"

if [[ ! -x "$BIN" ]]; then
  echo "audit-no-cloud: binary not found or not executable: $BIN" >&2
  exit 2
fi

# Hostnames the omw_local build must NOT contain.
PATTERNS=(
  "app.warp.dev"
  "api.warp.dev"
  "cloud.warp.dev"
  "oz.warp.dev"
  "firebase.googleapis.com"
  "firebaseio.com"
  "identitytoolkit.googleapis.com"
  "securetoken.googleapis.com"
)

fail=0
for pat in "${PATTERNS[@]}"; do
  count=$(strings "$BIN" | grep -c -F "$pat" || true)
  printf "%-40s %d\n" "$pat" "$count"
  if [[ "$count" -gt 0 ]]; then
    fail=1
  fi
done

if [[ "$fail" -ne 0 ]]; then
  echo "audit-no-cloud: FAIL — forbidden hostnames present in $BIN" >&2
  exit 1
fi

echo "audit-no-cloud: OK"
