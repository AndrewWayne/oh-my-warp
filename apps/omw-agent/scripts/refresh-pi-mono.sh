#!/usr/bin/env bash
# Refresh the vendored pi-agent-core kernel from the vendor/pi-mono submodule.
# Run from the umbrella repo root.
#
# Manual ritual — see apps/omw-agent/vendor/README.md for the bump procedure.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
SRC="$REPO_ROOT/vendor/pi-mono/packages/agent/src"
DST="$REPO_ROOT/apps/omw-agent/vendor/pi-agent-core"

if [[ ! -d "$SRC" ]]; then
    echo "error: $SRC missing — run 'git submodule update --init vendor/pi-mono' first" >&2
    exit 1
fi

# Refuse if pin commit doesn't match what README documents. The README is
# the source of truth; the script would silently land any commit otherwise.
EXPECTED_PIN="fe1381389de87d2620af5d7e46d00f76f4e65274"
ACTUAL_PIN="$(cd "$REPO_ROOT/vendor/pi-mono" && git rev-parse HEAD)"
if [[ "$ACTUAL_PIN" != "$EXPECTED_PIN" ]]; then
    echo "error: vendor/pi-mono is at $ACTUAL_PIN but README pins $EXPECTED_PIN" >&2
    echo "  update apps/omw-agent/vendor/README.md before refreshing." >&2
    exit 1
fi

mkdir -p "$DST"

# Copy only the five kernel files — agent-loop, agent, index, proxy, types.
# Anything else in pi-mono is out of scope (see vendor/README.md).
for f in agent-loop.ts agent.ts index.ts proxy.ts types.ts; do
    cp "$SRC/$f" "$DST/$f"
done

# Refresh the LICENSE alongside the source in case upstream renews/edits.
cp "$REPO_ROOT/vendor/pi-mono/LICENSE" "$DST/LICENSE"

echo "refreshed pi-agent-core into $DST"
echo "next:"
echo "  - bump @mariozechner/pi-ai in apps/omw-agent/package.json to match upstream agent-core's pi-ai dep"
echo "  - cd apps/omw-agent && npm install && npm run typecheck && npm test"
