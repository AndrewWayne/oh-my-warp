#!/bin/sh
# PreToolUse hook for Bash. Blocks `bash scripts/build-mac-dmg.sh ...` and
# the Windows equivalent unless the runtime's bundle probes are in parity
# with the build script's bundling steps.
#
# What this catches:
# - omw_inproc_server.rs probes for `<exe_dir>/../Resources/<path>` at
#   runtime; the build script must bundle <path>. Without parity, the
#   .app launches but ENOENTs on the first agent use (the v0.0.3 ship
#   bug: Resources/bin/node was probed but never bundled).
#
# Exits 2 to block, 0 to pass. Honors CLAUDE_HOOKS_DISABLED=1.

set -u

[ "${CLAUDE_HOOKS_DISABLED:-0}" = "1" ] && exit 0
command -v jq >/dev/null 2>&1 || exit 0

INPUT=$(cat)
CMD=$(printf '%s' "$INPUT" | jq -r '.tool_input.command // empty' 2>/dev/null || echo "")

[ -z "$CMD" ] && exit 0

# Match release build entry points only.
case "$CMD" in
  *"scripts/build-mac-dmg.sh"*) ;;
  *) exit 0 ;;
esac

PROJECT="${CLAUDE_PROJECT_DIR:-$(pwd)}"
INPROC="${PROJECT}/vendor/warp-stripped/app/src/ai_assistant/omw_inproc_server.rs"
SCRIPT="${PROJECT}/scripts/build-mac-dmg.sh"

[ -f "$INPROC" ] || exit 0
[ -f "$SCRIPT" ] || exit 0

# Extract every `<exe_dir>/../Resources/<path>` and `<exe_dir>/<path>`
# probed in inproc_server.rs (these are the .app-bundle and flat layout
# candidates).  Strip the prefix to get the relative path under Resources/.
PROBED=$(grep -oE 'exe_dir\.join\("[^"]+"\)' "$INPROC" \
  | sed -E 's@exe_dir\.join\("@@; s@"\)@@' \
  | grep -E '^\.\./Resources/|^bin/|^[A-Za-z]' \
  | sed -E 's@^\.\./Resources/@@' \
  | sort -u)

[ -z "$PROBED" ] && exit 0

MISSING=""
for path in $PROBED; do
    # The build script bundles into ${KERNEL_RESOURCES}, ${APP_DIR}/Contents/Resources,
    # or via `ditto ... Resources/<dir>`.  We look for the relative path or its
    # basename appearing on the right-hand side of cp/ditto/mkdir.
    base=$(basename "$path")
    # Match either the literal relative path or the basename in a bundling op.
    if ! grep -qE "(Resources/${path}|KERNEL_RESOURCES.*${base}|/Resources/${base})" "$SCRIPT"; then
        MISSING="${MISSING}  - ${path} (probed by omw_inproc_server.rs, no matching cp/ditto in scripts/build-mac-dmg.sh)
"
    fi
done

if [ -n "$MISSING" ]; then
    cat >&2 <<EOF
blocked: release-build self-containment audit failed.

The .app runtime probes these paths under Resources/ but the build script
does not bundle them.  Launching the .app from Finder will ENOENT on the
first agent use (this is the v0.0.3 ship bug).

Missing bundle steps:
${MISSING}
Fix one of:
  1. Add a cp/ditto step in scripts/build-mac-dmg.sh that places the
     dependency at the probed path.
  2. Remove the runtime probe in omw_inproc_server.rs if it is no longer
     needed.

To bypass for a local-only test build:
  CLAUDE_HOOKS_DISABLED=1 bash scripts/build-mac-dmg.sh ...

For a full audit (env vars, well-known-path resolvers, audit-no-cloud),
use the 'release-build-audit' skill before re-running the build.

Command: $CMD
EOF
    exit 2
fi

exit 0
