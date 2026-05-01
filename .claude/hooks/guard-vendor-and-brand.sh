#!/bin/sh
# PreToolUse hook for Write/Edit.
#   Warn on capitalized `Warp` landing in product-surface source.
#
# Exits 0 with stderr text to warn, 0 to pass.
# Honors CLAUDE_HOOKS_DISABLED=1.

set -u

[ "${CLAUDE_HOOKS_DISABLED:-0}" = "1" ] && exit 0
command -v jq >/dev/null 2>&1 || exit 0

INPUT=$(cat)
TOOL=$(printf '%s' "$INPUT" | jq -r '.tool_name // empty' 2>/dev/null || echo "")
TARGET=$(printf '%s' "$INPUT" | jq -r '.tool_input.file_path // empty' 2>/dev/null || echo "")

[ -z "$TARGET" ] && exit 0

# Brand check on product-surface paths only.
PRODUCT_SURFACE=0
case "$TARGET" in
  */crates/omw-*/src/*|crates/omw-*/src/*) PRODUCT_SURFACE=1 ;;
  */apps/web-controller/src/*|apps/web-controller/src/*) PRODUCT_SURFACE=1 ;;
  */apps/native-shim/*|apps/native-shim/*) PRODUCT_SURFACE=1 ;;
esac

[ "$PRODUCT_SURFACE" = "0" ] && exit 0

if [ "$TOOL" = "Write" ]; then
  CONTENT=$(printf '%s' "$INPUT" | jq -r '.tool_input.content // empty' 2>/dev/null || echo "")
elif [ "$TOOL" = "Edit" ]; then
  CONTENT=$(printf '%s' "$INPUT" | jq -r '.tool_input.new_string // empty' 2>/dev/null || echo "")
else
  exit 0
fi

[ -z "$CONTENT" ] && exit 0

# Strip lines with allow-list phrases, then look for capitalized `Warp` as a word.
HITS=$(
  printf '%s\n' "$CONTENT" \
    | grep -v -E '(oh-my-warp|warp-stripped|warpdotdev|upstream:|[Tt]rademark)' \
    | grep -nE '(^|[^A-Za-z])Warp([^a-z]|$)' \
    || true
)

if [ -n "$HITS" ]; then
  cat >&2 <<EOF
warning: 'Warp' (capitalized) found in product-surface file.
  Brand rule (CLAUDE.md §5): use 'omw' on the product surface.
  'oh-my-warp' is the repo codename only.
  Lines:
$(printf '%s\n' "$HITS" | sed 's/^/    /')
  If this is upstream attribution, tag the line with 'upstream:' or move it to LICENSE.
EOF
fi

exit 0
