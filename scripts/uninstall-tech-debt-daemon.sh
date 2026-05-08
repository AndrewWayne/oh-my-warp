#!/usr/bin/env bash
# uninstall-tech-debt-daemon.sh — bootout + remove the LaunchAgent plist.

set -euo pipefail

LABEL="com.omw.tech-debt-audit"
PLIST="${HOME}/Library/LaunchAgents/${LABEL}.plist"
DOMAIN="gui/$(id -u)"

if launchctl print "${DOMAIN}/${LABEL}" >/dev/null 2>&1; then
  launchctl bootout "${DOMAIN}/${LABEL}" 2>/dev/null || true
  printf 'Booted out %s.\n' "${LABEL}"
else
  printf 'Daemon %s was not loaded.\n' "${LABEL}"
fi

if [[ -f "${PLIST}" ]]; then
  rm -f "${PLIST}"
  printf 'Removed %s.\n' "${PLIST}"
fi

printf 'Done. Logs at ~/Library/Logs/omw-tech-debt-audit.* are kept.\n'
