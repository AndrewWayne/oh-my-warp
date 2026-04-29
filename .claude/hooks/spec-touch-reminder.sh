#!/bin/sh
# PostToolUse hook on Edit/Write. If a planning doc was touched,
# remind to run /spec-consistency. Non-blocking. Honors CLAUDE_HOOKS_DISABLED=1.

set -u

[ "${CLAUDE_HOOKS_DISABLED:-0}" = "1" ] && exit 0
command -v jq >/dev/null 2>&1 || exit 0

INPUT=$(cat)
TARGET=$(printf '%s' "$INPUT" | jq -r '.tool_input.file_path // empty' 2>/dev/null || echo "")

[ -z "$TARGET" ] && exit 0

case "$TARGET" in
  *PRD.md|*TODO.md|specs/*.md|*/specs/*.md)
    echo "reminder: $(basename "$TARGET") touched — run /spec-consistency before commit." >&2
    ;;
esac

exit 0
