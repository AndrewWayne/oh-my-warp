#!/bin/bash
# ai-notify.sh —— 统一的 macOS 通知核心(供 Codex / Claude 分发脚本调用)
# 用法: ai-notify.sh <source> <event> <project> <message>
#   source : 工具名, 如 Codex / Claude
#   event  : done(跑完了) | input(等我回答/批准)
#   project: 项目名/工作区(用于多会话区分与分组), 可空
#   message: 通知正文(最后一句话等), 可空
#
# 有 terminal-notifier 就用它(支持分组 -group、点击跳回 -activate);
# 没有则回退到系统自带 osascript(仍能区分项目和声音, 但点击无动作)。

src="${1:-AI}"
event="${2:-done}"
project="${3:-}"
message="${4:-}"

# 点击通知时要激活的 App(默认 omw;可用环境变量覆盖)
ACTIVATE_BUNDLE="${AI_NOTIFY_ACTIVATE_BUNDLE:-omw.local.warpOss}"

# 事件 → 图标 / 提示音
case "$event" in
  input)
    icon="⌛️"; word="等你"; sound="Basso" ;;
  *)
    icon="✅"; word="完成"; sound="Glass"; event="done" ;;
esac

# 标题:图标 + 工具 + 状态 (+ 项目)
if [ -n "$project" ]; then
  title="$icon $src $word · $project"
else
  title="$icon $src $word"
fi

# 正文兜底 + 截断(通知太长会被系统截,主动收到 ~200 字符)
[ -z "$message" ] && { [ "$event" = "input" ] && message="需要你回答/批准" || message="任务已结束"; }
# 压平换行,便于单行展示
message=$(printf '%s' "$message" | tr '\n' ' ' | cut -c1-200)

# 分组键:同一工具+项目的通知互相覆盖,避免刷屏
group="ai-notify:${src}:${project:-_}"

# ── omw 原生桥接(agent 钩子 → 精确 pane 聚焦)──────────────────────────
# 当本脚本由 agent 钩子(codex notify / claude Stop)在 omw 的 pane 里调用时
# (TERM_PROGRAM=WarpTerminal 且能写到该 pane 的 /dev/tty),就往该 pane 的 TTY
# 发一条 OSC 777 桌面通知转义,交给 omw 原生管线处理:omw 会把通知归到"这个
# pane",点击时精确聚焦回该对话——而不是像外部 macOS 通知那样只把 App 拉到前台。
# 桥接成功即退出, 避免与 macOS 通知重复。设 AI_NOTIFY_TTY_BRIDGE=0 可关闭。
if [ "${AI_NOTIFY_TTY_BRIDGE:-1}" != "0" ] && [ "${TERM_PROGRAM:-}" = "WarpTerminal" ]; then
  # OSC 777 的 title 是单个参数(不能含分号);body 可含分号(omw 会重新拼接)。
  osc_title=$(printf '%s' "$title" | tr ';' ',')

  # 找到目标 pane 的 TTY。
  # 首选:本进程自己的控制终端 /dev/tty。
  # 但 agent(如 Claude Code)常把钩子跑成"无控制终端"的子进程,这时 /dev/tty
  # 打不开——于是沿进程树向上找第一个带真实 TTY 的祖先(= 在 omw pane 里跑的
  # 那个 agent),把 OSC 777 写到它的 TTY 上,omw 就会把通知归到那个 pane。
  target_tty=""
  if { : >/dev/tty; } 2>/dev/null; then
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

  # 成功写入即视为已交给 omw 原生处理, 退出以避免重复弹 macOS 通知。
  if [ -n "$target_tty" ] \
     && { printf '\033]777;notify;%s;%s\007' "$osc_title" "$message" >"$target_tty"; } 2>/dev/null; then
    exit 0
  fi
fi

# 停留秒数:在"提醒(Alert)"样式下, 脚本会在这么多秒后自动撤掉通知
# (横幅样式下系统固定 ~5 秒消失, 该值无效)。设 0 = 不自动撤(需手动点掉)。
DWELL="${AI_NOTIFY_DWELL:-20}"

if command -v terminal-notifier >/dev/null 2>&1; then
  tn_args=( -title "$title" -message "$message" -sound "$sound" -group "$group" )
  # 点击动作:
  #   在 tmux 里(拿到了发起会话的 pane)→ 精确切到该 pane 的 window/pane, 再激活 omw
  #   不在 tmux 里 → 退回"整体激活 omw"(omw 无法从外部定位具体标签)
  pane="${AI_NOTIFY_TMUX_PANE:-}"
  tmux_bin="$(command -v tmux)"
  if [ -n "$pane" ] && [ -n "$tmux_bin" ]; then
    # 点击命令由 terminal-notifier 以极简 PATH 执行, 故 tmux 用绝对路径
    tn_args+=( -execute "'$tmux_bin' select-window -t '$pane' 2>/dev/null; '$tmux_bin' select-pane -t '$pane' 2>/dev/null; /usr/bin/open -b '$ACTIVATE_BUNDLE' 2>/dev/null" )
  else
    tn_args+=( -activate "$ACTIVATE_BUNDLE" )
  fi
  terminal-notifier "${tn_args[@]}" >/dev/null 2>&1
  # 停 DWELL 秒后自动移除(仅"提醒"样式下有实际停留效果)
  if [ "$DWELL" -gt 0 ] 2>/dev/null; then
    ( sleep "$DWELL"; terminal-notifier -remove "$group" >/dev/null 2>&1 ) </dev/null >/dev/null 2>&1 &
  fi
else
  # osascript:用 AppleScript argv 传参,避开引号/特殊字符转义问题
  /usr/bin/osascript \
    -e 'on run {t, m, s}' \
    -e 'display notification m with title t sound name s' \
    -e 'end run' \
    "$title" "$message" "$sound" >/dev/null 2>&1
fi
exit 0
