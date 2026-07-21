#!/bin/bash
# codex-notify-dispatch.sh —— Codex `notify` 分发脚本(omw agent 通知)。
#
# Codex 在一轮完成时按 config.toml 里的 `notify` 配置调用本脚本:
#     <本脚本> <配置里的额外参数...> '<JSON payload>'
# 本脚本在 agent-turn-complete 时调用 ai-notify.sh 弹通知——在 omw 的 pane 里,
# ai-notify.sh 会桥接成 omw 原生的"归属该 pane、可点击跳转"通知。
#
# 可选链式转发:若设置了环境变量 CODEX_PREVIOUS_NOTIFY 指向另一个 notify 程序,
# 本脚本会把所有参数原样转发给它(方便与已有的 notify 钩子共存)。默认不转发。

SELF_DIR="$(cd "$(dirname "$0")" && pwd)"
NOTIFY="$SELF_DIR/ai-notify.sh"

# ---- 完成通知(后台, 静默失败, 绝不阻塞 codex)----
{
  json="${@: -1}"          # 最后一个参数是 Codex 传来的 JSON
  proj="$(basename "$PWD")"
  msg="$(printf '%s' "$json" | /usr/bin/python3 -c '
import sys, json
try:
    d = json.load(sys.stdin)
except Exception:
    sys.exit(0)
if d.get("type") == "agent-turn-complete":
    print((d.get("last-assistant-message") or "").strip())
' 2>/dev/null)"
  if printf '%s' "$json" | grep -q '"agent-turn-complete"'; then
    "$NOTIFY" "Codex" "done" "$proj" "$msg"
  fi
} >/dev/null 2>&1 &

# ---- 可选:链式转发给另一个 notify 程序 ----
if [ -n "${CODEX_PREVIOUS_NOTIFY:-}" ] && [ -x "${CODEX_PREVIOUS_NOTIFY}" ]; then
  exec "${CODEX_PREVIOUS_NOTIFY}" "$@"
fi
exit 0
