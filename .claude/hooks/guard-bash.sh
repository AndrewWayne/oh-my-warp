#!/bin/sh
# PreToolUse hook for Bash. Blocks destructive ops we can't undo.
# Exits 2 to block, 0 to pass. Honors CLAUDE_HOOKS_DISABLED=1.

set -u

[ "${CLAUDE_HOOKS_DISABLED:-0}" = "1" ] && exit 0
command -v jq >/dev/null 2>&1 || exit 0

INPUT=$(cat)
CMD=$(printf '%s' "$INPUT" | jq -r '.tool_input.command // empty' 2>/dev/null || echo "")

[ -z "$CMD" ] && exit 0

# Block rm -rf against vendor/ (submodule destruction).
# Match flexibly: `rm -rf vendor`, `rm -rf ./vendor`, `rm -rf /abs/vendor`, with optional /warp-fork suffix.
case "$CMD" in
  *"rm -rf vendor"*|*"rm -rf ./vendor"*|*"rm -rf /"*"/vendor"*|*"rm -rf  vendor"*)
    cat >&2 <<EOF
blocked: 'rm -rf vendor/' would destroy the warp-fork submodule.
  Use 'git submodule deinit' or operate inside the sibling repo instead.
  Command: $CMD
EOF
    exit 2
    ;;
esac

# Block rm -rf against .git (repo history destruction).
case "$CMD" in
  *"rm -rf .git"*|*"rm -rf ./.git"*|*"rm -rf /"*"/.git"*)
    cat >&2 <<EOF
blocked: 'rm -rf .git' would destroy repository history.
  If you really want a clean checkout, clone fresh into a new directory.
  Command: $CMD
EOF
    exit 2
    ;;
esac

exit 0
