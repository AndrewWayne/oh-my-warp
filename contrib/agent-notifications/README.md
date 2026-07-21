# Agent notifications for omw (pane-focus)

让 **Claude Code / Codex** 等交互式 agent 跑完(或等你批准)时,在 omw 里弹一条
**归属到那个 pane 的原生通知,点击精确跳回该对话**。

## 这是什么 / 为什么需要它

omw 本体已内置"面向 pane 的可点击通知":任何程序往终端写 **OSC 9 / OSC 777** 通知
转义,omw 就会弹原生通知,并且点击时聚焦回发出它的那个 pane(见 `feat/pane-focus-notifications`)。

但**交互式 agent(TUI)在你聚焦它时基本不主动吐 OSC 转义**,所以光靠 omw 本体探不到"这一轮完成了"。可靠的完成信号是 agent 自己的**程序钩子**:

- Codex 的 `notify`(每轮完成时回调一个程序)
- Claude Code 的 `Stop` / `Notification` 钩子

本目录的脚本把这两者桥接起来:**用 agent 钩子做可靠触发 → 往该 pane 的 TTY 写一条
OSC 777 → 交给 omw 本体做"归属 pane + 可点击跳转"**。这样"每轮必响 + 精确跳回"两头都占。

## 前置条件

- 一个**带 pane-focus notifications 的 omw build**(`feat/pane-focus-notifications`,
  即 `FeatureFlag::PluggableNotifications` 在 omw_local 下已解门控)。旧 build 收不到 OSC 777。
- `python3`(解析钩子 JSON)。
- 可选:`terminal-notifier`(不在 omw 里时的 macOS 通知兜底;没有则用 `osascript`)。

## 安装

把三个脚本放到 PATH 上任意目录(下例用 `~/.local/bin`)并加执行权限:

```bash
install -m755 ai-notify.sh              ~/.local/bin/ai-notify.sh
install -m755 claude-notify-dispatch.sh ~/.local/bin/claude-notify-dispatch.sh
install -m755 codex-notify-dispatch.sh  ~/.local/bin/codex-notify-dispatch.sh
```

### 配置 Claude Code

在 `~/.claude/settings.json` 加钩子:

```json
{
  "hooks": {
    "Stop": [
      { "matcher": "", "hooks": [
        { "type": "command", "command": "~/.local/bin/claude-notify-dispatch.sh done" } ] }
    ],
    "Notification": [
      { "matcher": "", "hooks": [
        { "type": "command", "command": "~/.local/bin/claude-notify-dispatch.sh input" } ] }
    ]
  }
}
```

- `Stop` = 回答结束(跑完了);`Notification` = 需要你回答/授权(会自动过滤"单纯空闲等待")。
- Claude Code 启动时读该文件,改后需重开 claude。

### 配置 Codex

在 codex 的 `config.toml`(你的 `CODEX_HOME` 那份):

```toml
# 每轮完成 → 走钩子桥接(可靠)
notify = ["/绝对路径/codex-notify-dispatch.sh", "turn-ended"]

# "等你批准" codex 的 notify 钩子收不到, 只能靠 TUI 通知发 OSC 9:
[tui]
notifications = ["approval-requested"]
notification_method = "osc9"
```

- 已有别的 notify 程序想共存?给上面加 `CODEX_PREVIOUS_NOTIFY=/那个程序` 到环境即可链式转发。
- codex 需重开会话才读新配置。
- **局限**:codex 的 OSC 9 不带类型,omw 会把 approval 也当"完成"标签(正文仍是它要你批准的话)。

## 原理(数据流)

```
agent 完成/等待 → agent 钩子(codex notify / claude Stop)
    → *-notify-dispatch.sh(解析出 项目名/消息)
    → ai-notify.sh
        ├ 在 omw 里(TERM_PROGRAM=WarpTerminal): 沿进程树找到该 pane 的 TTY,
        │   写 OSC 777 → omw 本体归属该 pane 弹通知 → 点击精确跳回该 pane
        └ 不在 omw 里: 退回 terminal-notifier / osascript 的 macOS 通知
```

钩子常被 agent 跑成**无控制终端**的子进程,所以 `ai-notify.sh` 会沿进程树向上找
**第一个有真实 TTY 的祖先**(= 在 pane 里跑的那个 agent)写过去。

## 环境开关(`ai-notify.sh`)

| 变量 | 默认 | 作用 |
|---|---|---|
| `AI_NOTIFY_TTY_BRIDGE` | `1` | 设 `0` 关闭 OSC 777 桥接, 一律走 macOS 通知 |
| `AI_NOTIFY_ACTIVATE_BUNDLE` | `omw.local.warpOss` | 非桥接时点击激活的 App bundle id |
| `AI_NOTIFY_DWELL` | `20` | "提醒"样式下多少秒后自动撤掉(横幅样式无效) |

## 注意

- 桥接只在**装了本特性的 omw build** 里给你原生 pane 跳转;在旧 omw 里 OSC 777 会被忽略、
  且桥接会跳过 macOS 兜底 → 那种情况请设 `AI_NOTIFY_TTY_BRIDGE=0` 或升级 omw。
- 这套脚本是 omw 本体特性之外的**可选增强**,用于把 agent 完成信号接进 omw 原生通知。
