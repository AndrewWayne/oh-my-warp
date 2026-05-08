#!/usr/bin/env bash
# install-tech-debt-daemon.sh
#
# Installs ~/Library/LaunchAgents/com.omw.tech-debt-audit.plist that runs
# tech-debt-audit-runner.sh on Mon/Wed/Fri at 09:30 local time.
#
# Idempotent: bootouts an existing daemon before bootstrapping the fresh one.

set -euo pipefail

LABEL="com.omw.tech-debt-audit"
PLIST="${HOME}/Library/LaunchAgents/${LABEL}.plist"
SCRIPTS_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_DIR="$(cd "${SCRIPTS_DIR}/.." && pwd)"
RUNNER="${SCRIPTS_DIR}/tech-debt-audit-runner.sh"
LOG_DIR="${HOME}/Library/Logs"
DOMAIN="gui/$(id -u)"

# Preflight checks — fail loud if the daemon would be a silent dud.
fatal() { printf 'ERROR: %s\n' "$1" >&2; exit 1; }
[[ -x "$RUNNER" ]] || fatal "runner not found or not executable: $RUNNER"
command -v codex >/dev/null 2>&1 || fatal "codex CLI not on PATH (brew install codex)"
command -v gh    >/dev/null 2>&1 || fatal "gh CLI not on PATH (brew install gh)"
command -v jq    >/dev/null 2>&1 || fatal "jq not on PATH (brew install jq)"
gh auth status >/dev/null 2>&1   || fatal "gh not authenticated. Run: gh auth login"

# Confirm codex is logged in by running a trivial probe.
if ! echo "ping" | codex exec --skip-git-repo-check -m gpt-5.5 \
       --sandbox read-only >/dev/null 2>&1; then
  printf 'WARN: codex probe with gpt-5.5 failed. The daemon may need `codex login` first.\n' >&2
fi

mkdir -p "$LOG_DIR" "$(dirname "$PLIST")"

cat > "$PLIST" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>${LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>/bin/bash</string>
        <string>${RUNNER}</string>
    </array>
    <key>StartCalendarInterval</key>
    <array>
        <dict>
            <key>Weekday</key><integer>1</integer>
            <key>Hour</key><integer>9</integer>
            <key>Minute</key><integer>30</integer>
        </dict>
        <dict>
            <key>Weekday</key><integer>3</integer>
            <key>Hour</key><integer>9</integer>
            <key>Minute</key><integer>30</integer>
        </dict>
        <dict>
            <key>Weekday</key><integer>5</integer>
            <key>Hour</key><integer>9</integer>
            <key>Minute</key><integer>30</integer>
        </dict>
    </array>
    <key>StandardOutPath</key>
    <string>${LOG_DIR}/omw-tech-debt-audit.out.log</string>
    <key>StandardErrorPath</key>
    <string>${LOG_DIR}/omw-tech-debt-audit.err.log</string>
    <key>WorkingDirectory</key>
    <string>${REPO_DIR}</string>
    <key>EnvironmentVariables</key>
    <dict>
        <key>PATH</key>
        <string>/opt/homebrew/bin:/opt/homebrew/sbin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin</string>
        <key>HOME</key>
        <string>${HOME}</string>
    </dict>
    <key>RunAtLoad</key>
    <false/>
</dict>
</plist>
EOF

# Bootstrap (idempotent — bootout first if already loaded)
if launchctl print "${DOMAIN}/${LABEL}" >/dev/null 2>&1; then
  printf 'Reloading existing daemon...\n'
  launchctl bootout "${DOMAIN}/${LABEL}" 2>/dev/null || true
fi

launchctl bootstrap "${DOMAIN}" "${PLIST}"
launchctl enable    "${DOMAIN}/${LABEL}"

cat <<MSG

Installed: ${LABEL}
  Schedule:   Mon/Wed/Fri at 09:30 local
  Runner:     ${RUNNER}
  Plist:      ${PLIST}
  Logs:       ${LOG_DIR}/omw-tech-debt-audit.{log,out.log,err.log}
  Repo:       ${REPO_DIR}
  GitHub:     auto-files top-15 NEW findings per run, label=tech-debt-audit
  Dedup:      open OR closed issues block re-filing (close to suppress)

Run now (out-of-schedule):
  launchctl kickstart -k ${DOMAIN}/${LABEL}

Dry-run (write artifacts, don't file or commit):
  OMW_AUDIT_DRY_RUN=1 bash ${RUNNER}

Tail live log:
  tail -f ${LOG_DIR}/omw-tech-debt-audit.log

Uninstall:
  bash ${SCRIPTS_DIR}/uninstall-tech-debt-daemon.sh

Cost note: each run invokes codex with model gpt-5.5 + reasoning xhigh.
Expect 5-20 min and a non-trivial token bill per run. Adjust by setting
OMW_AUDIT_MODEL / OMW_AUDIT_REASONING in the plist EnvironmentVariables.
MSG
