#!/usr/bin/env bash
# tech-debt-audit-runner.sh
#
# Runs codex against the /tech-debt-audit skill, then files NEW findings as
# GitHub issues (cap: top-15 per run, ranked by severity).
#
# Idempotent: any finding ID already present in an open OR closed issue with
# the `tech-debt-audit` label is skipped — closing an issue is the human's
# veto. Reopen to un-suppress.
#
# Invoked by ~/Library/LaunchAgents/com.omw.tech-debt-audit.plist on
# Mon/Wed/Fri at 09:30 local. Can also be run manually.
#
# Manual invocations:
#   bash scripts/tech-debt-audit-runner.sh           # full run
#   OMW_AUDIT_DRY_RUN=1 bash scripts/...             # write artifacts, don't file
#   OMW_AUDIT_MODEL=gpt-5.3-codex bash scripts/...   # override model
#   TOP_N=5 bash scripts/...                         # smaller cap this run

set -euo pipefail

REPO_DIR="${REPO_DIR:-/Users/andrewwayne/oh-my-warp}"
GH_REPO="${GH_REPO:-AndrewWayne/oh-my-warp}"
LOG_DIR="${HOME}/Library/Logs"
LOG_FILE="${LOG_DIR}/omw-tech-debt-audit.log"
ISSUE_LABEL="tech-debt-audit"
TOP_N="${TOP_N:-15}"
MODEL="${OMW_AUDIT_MODEL:-gpt-5.5}"
REASONING="${OMW_AUDIT_REASONING:-xhigh}"
LOCK_DIR="${LOG_DIR}/omw-tech-debt-audit.lock.d"

today=$(date +%Y-%m-%d)
mkdir -p "$LOG_DIR"

log() { printf '[%s] %s\n' "$(date '+%Y-%m-%d %H:%M:%S')" "$*" | tee -a "$LOG_FILE" >&2; }

# Concurrency guard: atomic mkdir succeeds for at most one runner.
if ! mkdir "$LOCK_DIR" 2>/dev/null; then
  log "another run holds the lock at $LOCK_DIR; exiting"
  exit 0
fi
cleanup() { rmdir "$LOCK_DIR" 2>/dev/null || true; }
trap cleanup EXIT

log "=== tech-debt-audit run starting (model=$MODEL reasoning=$REASONING) ==="

cd "$REPO_DIR"

# Refuse to audit a dirty tree — would either contaminate the audit with
# in-progress changes or trip git later.
if ! git diff --quiet || ! git diff --cached --quiet; then
  log "ERROR: working tree has uncommitted modifications; refusing to audit"
  exit 2
fi

# Tool checks
for cmd in codex gh jq git; do
  command -v "$cmd" >/dev/null 2>&1 || { log "ERROR: $cmd not on PATH"; exit 2; }
done
gh auth status >/dev/null 2>&1 || { log "ERROR: gh not authenticated"; exit 2; }

audit_md="docs/tech-debt-audit-${today}.md"
findings_json="docs/tech-debt-audit-${today}.findings.json"

prompt_file=$(mktemp)
body_tmp=$(mktemp)
codex_log=$(mktemp)
existing_ids_file=$(mktemp)
filed_ids_file=$(mktemp)
cleanup() {
  rmdir "$LOCK_DIR" 2>/dev/null || true
  rm -f "$prompt_file" "$body_tmp" "$codex_log" "$existing_ids_file" "$filed_ids_file"
}
trap cleanup EXIT

cat > "$prompt_file" <<PROMPT
Run the /tech-debt-audit skill end-to-end on this repo. Follow its protocol verbatim — vendor scope rule (omw delta only inside vendor/warp-stripped/), prior-triage awareness (read the most recent docs/simplify-followups-*.md before Phase 2), and the required "Things that look bad but are actually fine" section.

Output TWO artifacts. Do NOT interact with GitHub.

1. The standard markdown audit at docs/tech-debt-audit-${today}.md.

2. A machine-readable sidecar at docs/tech-debt-audit-${today}.findings.json — a JSON array of objects, one per finding. Each object MUST have these fields:

   {
     "id":             "<12-char hex>",
     "category":       "<one of the 9 dimensions from the skill>",
     "severity":       "Critical" | "High" | "Medium" | "Low",
     "effort":         "S" | "M" | "L",
     "file":           "<repo-relative path>",
     "line":           <integer; 0 if not applicable>,
     "title":          "<one-line finding title, max 120 chars>",
     "description":    "<finding body, plain text, no markdown>",
     "recommendation": "<recommendation body, plain text, no markdown>"
   }

   The "id" MUST be the first 12 hex characters of the SHA-256 of the literal string formed as: category, then a pipe, then file:line, then a pipe, then title. Example: a finding with category="Test debt", file="crates/omw-server/src/lib.rs", line=43, title="serve_agent_loopback has no callers" yields id = first 12 hex of sha256("Test debt|crates/omw-server/src/lib.rs:43|serve_agent_loopback has no callers"). Use a tool to compute the hash; do not invent IDs.

Both files MUST be written before you exit. The runner verifies both exist and that the JSON parses.
PROMPT

log "invoking codex (workspace-write sandbox; will write 2 docs/* files)"
if ! codex exec --skip-git-repo-check \
     -m "$MODEL" \
     --config "model_reasoning_effort=\"$REASONING\"" \
     --sandbox workspace-write \
     --full-auto \
     -C "$REPO_DIR" \
     - <"$prompt_file" >"$codex_log" 2>&1; then
  log "ERROR: codex exec failed (last 40 lines of codex output below)"
  tail -40 "$codex_log" >>"$LOG_FILE"
  exit 3
fi

cat "$codex_log" >>"$LOG_FILE"

if [[ ! -f "$audit_md" ]] || [[ ! -f "$findings_json" ]]; then
  log "ERROR: codex completed but expected artifacts missing"
  log "  $audit_md present=$( [[ -f $audit_md ]] && echo yes || echo no )"
  log "  $findings_json present=$( [[ -f $findings_json ]] && echo yes || echo no )"
  exit 4
fi

if ! jq empty "$findings_json" 2>/dev/null; then
  log "ERROR: $findings_json is not valid JSON"
  exit 5
fi

n_total=$(jq 'length' "$findings_json")
log "audit complete: $audit_md (sidecar: $n_total findings)"

if [[ "${OMW_AUDIT_DRY_RUN:-0}" == "1" ]]; then
  log "dry-run mode (OMW_AUDIT_DRY_RUN=1) — skipping issue filing and commit"
  exit 0
fi

# Fetch existing finding IDs from open + closed issues with our label.
if ! gh issue list -R "$GH_REPO" -l "$ISSUE_LABEL" -s all \
     --json body --limit 1000 \
     | jq -r '.[].body | capture("Finding ID: `(?<id>[a-f0-9]+)`") | .id' \
     > "$existing_ids_file" 2>>"$LOG_FILE"; then
  log "ERROR: gh issue list failed"
  exit 6
fi

n_existing=$(wc -l < "$existing_ids_file" | tr -d ' ')
log "existing tech-debt issues (open+closed): $n_existing"

# Severity rank for sort. Lower = more urgent.
sev_rank='{"Critical":0,"High":1,"Medium":2,"Low":3}'

new_top=$(jq \
  --rawfile existing_text "$existing_ids_file" \
  --argjson rank "$sev_rank" \
  --argjson topn "$TOP_N" \
  '
  ($existing_text | split("\n") | map(select(length > 0))) as $existing
  | map(select(.id as $id | ($existing | index($id)) | not))
  | sort_by($rank[.severity] // 99)
  | .[0:$topn]
  ' "$findings_json")

n_new_total=$(jq \
  --rawfile existing_text "$existing_ids_file" \
  '
  ($existing_text | split("\n") | map(select(length > 0))) as $existing
  | map(select(.id as $id | ($existing | index($id)) | not)) | length
  ' "$findings_json")

n_filing=$(jq 'length' <<<"$new_top")
log "NEW findings: $n_new_total. Filing top $n_filing (cap=$TOP_N)."

n_filed=0
n_failed=0

while IFS= read -r finding; do
  id=$(jq -r '.id' <<<"$finding")
  severity=$(jq -r '.severity' <<<"$finding")
  effort=$(jq -r '.effort' <<<"$finding")
  category=$(jq -r '.category' <<<"$finding")
  file=$(jq -r '.file' <<<"$finding")
  line=$(jq -r '.line' <<<"$finding")
  title=$(jq -r '.title' <<<"$finding")
  description=$(jq -r '.description' <<<"$finding")
  recommendation=$(jq -r '.recommendation' <<<"$finding")

  issue_title="[tech-debt][$severity] $title"
  issue_title=$(printf '%s' "$issue_title" | cut -c1-200)

  # Build issue body via printf to avoid heredoc-in-cmd-subst-with-backticks
  # parsing trouble. Pass via gh --body-file.
  {
    printf '**Finding ID:** `%s` (autogenerated; do not change — daemon dedup keys on this)\n' "$id"
    printf '**Category:** %s\n' "$category"
    printf '**Severity:** %s\n' "$severity"
    printf '**Effort:** %s\n' "$effort"
    printf '**File:** `%s:%s`\n' "$file" "$line"
    printf '**Audit run:** %s\n\n' "$today"
    printf '## Description\n\n%s\n\n' "$description"
    printf '## Recommendation\n\n%s\n\n' "$recommendation"
    printf -- '---\n'
    printf '*Filed automatically by `scripts/tech-debt-audit-runner.sh` on %s. ' "$today"
    printf 'Close this issue if intentional or not actionable; the daemon respects closed issues by ID and won'\''t re-file. '
    printf 'Full audit context: [`docs/tech-debt-audit-%s.md`](../blob/main/docs/tech-debt-audit-%s.md).*\n' "$today" "$today"
  } > "$body_tmp"

  sev_label="severity:$(printf '%s' "$severity" | tr '[:upper:]' '[:lower:]')"
  if gh issue create -R "$GH_REPO" \
       -t "$issue_title" \
       -F "$body_tmp" \
       -l "$ISSUE_LABEL" \
       -l "$sev_label" \
       >>"$LOG_FILE" 2>&1; then
    log "filed $id ($severity, $category)"
    printf '%s\n' "$id" >> "$filed_ids_file"
    n_filed=$((n_filed + 1))
  else
    log "WARN: gh issue create FAILED for $id"
    n_failed=$((n_failed + 1))
  fi
done < <(jq -c '.[]' <<<"$new_top")

log "filed=$n_filed failed=$n_failed (of $n_filing attempted; $n_new_total NEW total this run)"

# Auto-commit the audit artifacts so they're tracked. Don't push — that's
# the user's call.
if [[ -n "$(git status --porcelain "$audit_md" "$findings_json" 2>/dev/null)" ]]; then
  git add "$audit_md" "$findings_json"
  if git -c "user.name=tech-debt-audit-daemon" \
         -c "user.email=tech-debt-audit-daemon@local" \
         commit -m "audit: tech-debt-audit-${today} (filed ${n_filed} new issues)

[skip ci]" >>"$LOG_FILE" 2>&1; then
    log "committed audit artifacts (local; not pushed)"
  else
    log "WARN: git commit failed (artifacts left uncommitted)"
  fi
fi

log "=== run complete ==="
