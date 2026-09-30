#!/bin/bash
# ai-notify.sh —— 把一条通知作为 OSC 777 发到「发起它的那个 pane」,交给新 omw
# 原生处理(归属该 pane、点击精确跳回)。供 agent-notify.sh 等分发脚本调用。
#
# 用法: ai-notify.sh <source> <event> <project> <message>
#   source : 工具名, 如 Codex / Claude
#   event  : done(跑完了) | input(等我回答/批准)
#   project: 项目名/工作区, 可空
#   message: 通知正文, 可空
#
# 只走 omw(OSC 777):不再有 macOS 兜底,也不看 AI_NOTIFY_TTY_BRIDGE/TERM_PROGRAM。
# 前提:你在带 pane-focus 的新 omw 里跑(旧 omw 会忽略 OSC 777 → 收不到通知)。

# 手动总开关:存在此文件时静音、立即退出(测试用)。删掉即恢复。
[ -f "$HOME/.local/state/ai-notify-off" ] && exit 0

src="${1:-AI}"
event="${2:-done}"
project="${3:-}"
message="${4:-}"

case "$event" in
  input) icon="⌛️"; word="等你" ;;
  *)     icon="✅"; word="完成"; event="done" ;;
esac

# 标题(OSC 777 的 title 是单个参数、不能含分号 → 分号换逗号)
if [ -n "$project" ]; then
  title="$icon $src $word · $project"
else
  title="$icon $src $word"
fi
title=$(printf '%s' "$title" | tr ';' ',')

# 正文兜底 + 压平换行 + 截断
[ -z "$message" ] && { [ "$event" = "input" ] && message="需要你回答/批准" || message="任务已结束"; }
message=$(printf '%s' "$message" | tr '\n' ' ' | cut -c1-200)

# 找到发起 pane 的 TTY:首选本进程控制终端 /dev/tty;agent 钩子常是无控制终端的
# 子进程,则沿进程树向上找第一个有真实可写 TTY 的祖先(= 在 omw pane 里跑的 agent)。
target_tty=""
# 最可靠:agent 启动时通过 AI_PANE_TTY 传进来的 pane tty(env 被子进程继承,
# 不受钩子被 claude 跑成脱离进程的影响)。
if [ -n "$AI_PANE_TTY" ] && [ -w "$AI_PANE_TTY" ]; then
  target_tty="$AI_PANE_TTY"
elif { : >/dev/tty; } 2>/dev/null; then
  target_tty="/dev/tty"
else
  _pid=$$
  _i=0
  while [ "$_i" -lt 12 ]; do
    _i=$((_i + 1))
    _pid=$(ps -o ppid= -p "$_pid" 2>/dev/null | tr -d ' ')
    { [ -z "$_pid" ] || [ "$_pid" -le 1 ]; } && break
    _t=$(ps -o tty= -p "$_pid" 2>/dev/null | tr -d ' ')
    if [ -n "$_t" ] && [ "$_t" != "??" ] && [ "$_t" != "-" ] && [ -w "/dev/$_t" ]; then
      target_tty="/dev/$_t"
      break
    fi
  done
fi

# 发 OSC 777 给该 pane,交给 omw 原生管线;找不到 TTY 就安静退出(无 macOS 兜底)。
[ -n "$target_tty" ] && printf '\033]777;notify;%s;%s\007' "$title" "$message" >"$target_tty" 2>/dev/null
exit 0
