#!/bin/bash
# claude-notify-dispatch.sh —— Claude Code 钩子分发脚本
# 由两个钩子共用, 用第一个参数区分事件:
#   Stop         钩子 →  <本脚本> done   (Claude 回答结束 = 跑完了)
#   Notification 钩子 →  <本脚本> input  (Claude 需要你回答/授权才能继续)
# 钩子的 JSON 从 stdin 传入(含 cwd / transcript_path / message 等)。
#
# 注意:Notification 钩子会为两种情况触发——
#   ① 真·要你决定: "Claude needs your permission to use ..." → 要通知
#   ② 单纯空闲等待: "Claude is waiting for your input"(完成后闲置才弹)→ 不通知
#      (它跟前面的"完成"重复, 用户不想要)
# 因此 input 分支会把 ② 过滤掉。

SELF_DIR="$(cd "$(dirname "$0")" && pwd)"
NOTIFY="$SELF_DIR/ai-notify.sh"
EVENT="${1:-done}"

input="$(cat)"

# Stop 钩子可能在"本轮最后一条 assistant 消息写入 transcript"之前触发,导致读到
# 的是上一轮的回答(off-by-one)。短暂等待让 transcript 落盘。仅 done(Stop, 读
# transcript)需要;input 的文案直接来自钩子 JSON, 无此问题。
[ "$EVENT" = "done" ] && sleep 0.6

# python 解析:第 1 行 = 动作(notify/skip), 第 2 行 = 项目名, 第 3 行起 = 正文
parsed="$(printf '%s' "$input" | EVENT="$EVENT" /usr/bin/python3 -c '
import sys, os, json
try:
    d = json.load(sys.stdin)
except Exception:
    d = {}
event = os.environ.get("EVENT", "done")
proj = os.path.basename(d.get("cwd", "") or "") or "-"
action = "notify"
msg = ""

if event == "input":
    msg = (d.get("message") or "").strip()
    # 过滤掉"单纯空闲等待"这类(完成后闲置触发, 与 done 重复)
    low = msg.lower()
    if ("waiting for your input" in low) or ("waiting for input" in low) or (msg == ""):
        action = "skip"
else:
    # Stop 钩子:从 transcript 取最后一条 assistant 文本
    tp = d.get("transcript_path", "") or ""
    if tp and os.path.exists(tp):
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

print(action)
print(proj)
print(msg.replace("\n"," "))
' 2>/dev/null)"

action="$(printf '%s' "$parsed" | sed -n '1p')"
proj="$(printf '%s' "$parsed" | sed -n '2p')"
msg="$(printf '%s' "$parsed" | sed -n '3,$p')"

# 被判为"空闲等待"等重复情形 → 不弹
[ "$action" = "skip" ] && exit 0

# 把发起会话的 tmux pane 传给通知脚本, 供点击时精确跳转
AI_NOTIFY_TMUX_PANE="${TMUX_PANE:-}" "$NOTIFY" "Claude" "$EVENT" "$proj" "$msg"
exit 0
