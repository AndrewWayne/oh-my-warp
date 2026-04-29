#!/bin/sh
# Stop hook. If non-trivial work happened (files changed outside .claude/, .git/)
# but TODO.md is unchanged, remind to update phase status.
# Non-blocking. Honors CLAUDE_HOOKS_DISABLED=1.

set -u

[ "${CLAUDE_HOOKS_DISABLED:-0}" = "1" ] && exit 0
command -v jq >/dev/null 2>&1 || exit 0

INPUT=$(cat)
ACTIVE=$(printf '%s' "$INPUT" | jq -r '.stop_hook_active // false' 2>/dev/null || echo "false")
[ "$ACTIVE" = "true" ] && exit 0

cd "${CLAUDE_PROJECT_DIR:-.}" 2>/dev/null || exit 0
command -v git >/dev/null 2>&1 || exit 0
git rev-parse --git-dir >/dev/null 2>&1 || exit 0

# Combine staged + unstaged + untracked (excluding gitignored).
CHANGED=$(
  {
    git diff --name-only HEAD 2>/dev/null || true
    git ls-files --others --exclude-standard 2>/dev/null || true
  } | sort -u
)
[ -z "$CHANGED" ] && exit 0

# Filter to "interesting" paths: not .claude/, not .git/.
INTERESTING=$(printf '%s\n' "$CHANGED" | grep -v -E '^\.claude/|^\.git/' || true)
[ -z "$INTERESTING" ] && exit 0

# If TODO.md is in the change set, no reminder needed.
if printf '%s\n' "$CHANGED" | grep -q '^TODO\.md$'; then
  exit 0
fi

cat >&2 <<EOF
reminder: non-trivial files changed but TODO.md untouched.
  If a phase item moved (started, completed, blocked), update TODO.md.
  Files changed:
$(printf '%s\n' "$INTERESTING" | head -10 | sed 's/^/    /')
EOF

exit 0
