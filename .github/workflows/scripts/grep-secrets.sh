#!/usr/bin/env bash
# I-1 defense-in-depth: refuse to ship a tree containing literal API-key prefixes.
# The primary defense is omw-config's KeyRef type (structural rejection at parse
# time). This grep is a backstop for code paths that bypass KeyRef.
#
# Reference: specs/threat-model.md §4 (I-1).

set -uo pipefail

# Patterns. The {20,} length lets us tolerate short test fixtures like
# "sk-test123" (rejected by KeyRef anyway) without false-positiving on every PR
# that touches the validator's tests.
patterns=(
  'sk-[A-Za-z0-9_-]{20,}'
  'sk-ant-[A-Za-z0-9_-]{20,}'
  'sk-proj-[A-Za-z0-9_-]{20,}'
)

# Self-exclusion: this script names the patterns it's looking for.
exclude=':!.github/workflows/scripts/grep-secrets.sh'

found=0
for pat in "${patterns[@]}"; do
  if matches=$(git grep -EHn "$pat" -- "$exclude" 2>/dev/null); then
    echo "I-1 violation: literal API key prefix found in tracked files:"
    echo "$matches"
    found=1
  fi
done

if [[ $found -ne 0 ]]; then
  echo
  echo "Refusing to proceed. See specs/threat-model.md invariant I-1." >&2
  exit 1
fi

echo "I-1 grep guard: clean."
