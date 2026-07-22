#!/bin/bash
# agent-notify.sh —— 统一的 agent 钩子分发脚本(Claude Code / Codex 共用一套)。
#
#   用法(在各 agent 的 hooks 配置里):  agent-notify.sh <来源标签>
#   例:  agent-notify.sh Claude    /    agent-notify.sh Codex
#
# 行为一律按 stdin JSON 里的 `hook_event_name` 分流(两家都用 Claude Code 的
# hooks 格式,故可共用):
#   Stop              → 完成(done)。正文取 last_assistant_message,否则读 transcript。
#   Notification      → 等你(input)。正文取 message;过滤"单纯空闲等待"。
#   PermissionRequest → 等你批准(input)。正文 = 批准 <tool_name>: <command/description>。
#   其它事件          → 不弹。
#
# 安全约定(Codex PermissionRequest):本脚本【绝不向 stdout 输出任何内容】、退出 0,
# 故永不构成"决定",codex 会照常走它自己的审批提示,绝不自动批准。整个通知在后台跑、
# 吞掉所有输出与错误,绝不阻塞 agent。

SOURCE="${1:-Agent}"
SELF_DIR="$(cd "$(dirname "$0")" && pwd)"
NOTIFY="$SELF_DIR/ai-notify.sh"

input="$(cat)"

{
  parsed="$(printf '%s' "$input" | /usr/bin/python3 -c '
import sys, os, json, time
try:
    d = json.load(sys.stdin)
except Exception:
    d = {}
ev = d.get("hook_event_name") or ""
proj = os.path.basename((d.get("cwd") or "").rstrip("/")) or "-"
action, event, msg = "notify", "done", ""

if ev == "Stop":
    event = "done"
    lam = (d.get("last_assistant_message") or "").strip()
    if lam:
        msg = lam
    else:
        tp = d.get("transcript_path") or ""
        if tp and os.path.exists(tp):
            time.sleep(0.6)  # 等最后一条 assistant 落盘, 避免 off-by-one
            try:
                with open(tp) as f:
                    for line in f:
                        try:
                            e = json.loads(line)
                        except Exception:
                            continue
                        if e.get("type") == "assistant":
                            content = (e.get("message") or {}).get("content", [])
                            if isinstance(content, list):
                                txt = "".join(c.get("text","") for c in content if isinstance(c, dict) and c.get("type")=="text")
                                if txt.strip():
                                    msg = txt.strip()
            except Exception:
                pass
elif ev == "Notification":
    event = "input"
    msg = (d.get("message") or "").strip()
    low = msg.lower()
    if ("waiting for your input" in low) or ("waiting for input" in low) or (msg == ""):
        action = "skip"
elif ev == "PermissionRequest":
    event = "input"
    tool = d.get("tool_name") or "命令"
    ti = d.get("tool_input") or {}
    detail = ""
    if isinstance(ti, dict):
        detail = ti.get("command") or ti.get("description") or ""
        if isinstance(detail, list):
            detail = " ".join(map(str, detail))
    msg = "批准 %s" % tool
    if str(detail).strip():
        msg += ": " + str(detail)
else:
    action = "skip"

print(action); print(event); print(proj); print(str(msg).replace("\n", " "))
' 2>/dev/null)"

  action="$(printf '%s' "$parsed" | sed -n '1p')"
  event="$(printf '%s' "$parsed" | sed -n '2p')"
  proj="$(printf '%s' "$parsed" | sed -n '3p')"
  msg="$(printf '%s' "$parsed" | sed -n '4,$p')"

  # 突发节流:只压"完成(done)"——同一 "来源:项目" 的完成通知在 THROTTLE 秒内
  # 只弹一次,治 workflow/排队会话里的连弹;间隔较久的"真完成"照弹。**"需要你/等
  # 批准(input)"永不节流**,那是最该立刻弹的。设 AI_NOTIFY_THROTTLE_SEC=0 关。
  if [ "$action" = "notify" ] && [ "$event" = "done" ]; then
    THROTTLE="${AI_NOTIFY_THROTTLE_SEC:-25}"
    if [ "$THROTTLE" -gt 0 ] 2>/dev/null; then
      key="$(printf '%s' "${SOURCE}:${event}:${proj}" | tr -c 'A-Za-z0-9' '_')"
      tdir="$HOME/.local/state/agent-notify-throttle"; mkdir -p "$tdir" 2>/dev/null
      now="$(date +%s)"; last="$(cat "$tdir/$key" 2>/dev/null || echo 0)"
      if [ $(( now - last )) -lt "$THROTTLE" ]; then
        action="throttled"
      else
        echo "$now" > "$tdir/$key"
      fi
    fi
  fi

  # 诊断日志:每次调用(含 skip/throttled)都记一行,便于排查"谁在弹、弹了啥"。
  # 关掉:设 AI_NOTIFY_LOG=0。
  if [ "${AI_NOTIFY_LOG:-1}" != "0" ]; then
    mkdir -p "$HOME/.local/state" 2>/dev/null
    printf '%s\t%s\t%s\t%s\t%s\n' "$(date '+%F %T')" "$SOURCE" "$event" "$action" \
      "$(printf '%s' "${proj}: ${msg}" | cut -c1-80)" >> "$HOME/.local/state/agent-notify.log" 2>/dev/null
  fi

  # 只有 notify 才继续弹(skip=空闲过滤,throttled=突发节流,都不弹)。
  [ "$action" = "notify" ] || exit 0
  AI_NOTIFY_TMUX_PANE="${TMUX_PANE:-}" "$NOTIFY" "$SOURCE" "$event" "$proj" "$msg"
} >/dev/null 2>&1 &

# 关键:主流程 stdout 保持为空、立即退出 0(不表态)。
exit 0
