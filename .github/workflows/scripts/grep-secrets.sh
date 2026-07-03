#!/usr/bin/env bash
# I-1 defense-in-depth: refuse to ship a tree containing literal API-key prefixes.
# The primary defense is omw-config's KeyRef type (structural rejection at parse
# time). This grep is a backstop for code paths that bypass KeyRef.
#
# Reference: specs/threat-model.md §4 (I-1).

set -uo pipefail

# Patterns. The {20,} length lets us tolerate short test fixtures like
# "sk-test123" (rejected by KeyRef anyway) without false-positiving on every PR
# that touches the validator's tests. The (^|[^A-Za-z0-9_-]) left boundary
# stops "sk-" matching inside ordinary words/URLs (e.g. "...warp-ai-ask-from-
# block-keybinding..." contains "sk-from-block-keybinding...").
patterns=(
  '(^|[^A-Za-z0-9_-])sk-[A-Za-z0-9_-]{20,}'
  '(^|[^A-Za-z0-9_-])sk-ant-[A-Za-z0-9_-]{20,}'
  '(^|[^A-Za-z0-9_-])sk-proj-[A-Za-z0-9_-]{20,}'
)

excludes=(
  # Self-exclusion: this script names the patterns it's looking for.
  ':!.github/workflows/scripts/grep-secrets.sh'
  # Upstream test fixtures inside the vendored fork carry deliberately fake
  # sk-ant-* keys (structurally valid, obviously synthetic). They predate the
  # fork and are exercised by upstream's own tests; keep them excluded rather
  # than diverge from upstream. Any NEW file that trips the scan still fails
  # CI and needs a human decision here.
  ':!vendor/warp-stripped/app/src/ai/agent_sdk/driver/harness/claude_code_tests.rs'
  ':!vendor/warp-stripped/app/src/terminal/model/secrets_test.rs'
)

found=0
for pat in "${patterns[@]}"; do
  # -I: never scan binary blobs (the vendored fasttext model false-positives).
  if matches=$(git grep -EHnI "$pat" -- "${excludes[@]}" 2>/dev/null); then
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
