# Agent notifications for omw (pane-focus)

让 **Claude Code / Codex** 等交互式 agent 在「跑完 / 等你批准」时,弹一条**归属到发起它
那个 pane 的 omw 原生通知,点击精确跳回该 pane**。

## 这是什么 / 为什么需要它

omw 本体已内置「面向 pane 的可点击通知」:任何程序往终端写 **OSC 9 / OSC 777** 通知转义,
omw 就弹原生通知,并在点击时聚焦回发出它的那个 pane(见 `feat/pane-focus-notifications`)。

但**交互式 agent(TUI)聚焦时基本不主动吐 OSC**,所以光靠 omw 本体探不到「这一轮完成 /
正在等你批准」。可靠信号是 agent 自己的**钩子(hooks)**。本目录的脚本把它桥接起来:

```
agent 钩子(Claude Stop/Notification、Codex Stop/PermissionRequest)
  → agent-notify.sh(按 hook_event_name 分流 + 突发节流)
  → ai-notify.sh(把通知作为 OSC 777 写回「发起它的那个 pane」)
  → omw 本体归属该 pane 弹原生通知 → 点击精确跳回
```

**只走 omw(OSC),不再有 macOS 兜底**。前提是你用**带 pane-focus 的新 omw**(旧 omw 会忽略
OSC 777 → 收不到)。

## 前置条件

- 一个**带 pane-focus notifications 的 omw build**(`feat/pane-focus-notifications`)。
- `python3`(解析钩子 JSON)。

## 安装

### 1) 放脚本

把两个脚本放到 PATH 上任意固定目录(下例用 `~/.local/bin`)并加执行权限。两者需**同目录**
(`agent-notify.sh` 会调同目录的 `ai-notify.sh`):

```bash
install -m755 agent-notify.sh ~/.local/bin/agent-notify.sh
install -m755 ai-notify.sh    ~/.local/bin/ai-notify.sh
```

### 2) 加 shell wrapper(关键:把 pane 的 tty 传下去)

agent 常把钩子跑成**脱离控制终端的子进程**,脚本沿进程树找不到发起它的 pane。解法:启动
agent 时用环境变量 `AI_PANE_TTY` 把当前 pane 的 tty 传进去(会被脱离的钩子继承)。在你的
shell rc(如 `~/.zshrc`)加:

```zsh
claude() { AI_PANE_TTY="$(tty 2>/dev/null)" command claude "$@"; }
codex()  { AI_PANE_TTY="$(tty 2>/dev/null)" command codex  "$@"; }
```

这样直接敲 `claude` / `codex` 即可,**不用手打**。新开的 shell 才生效。

### 3) 配 Claude Code(`~/.claude/settings.json`)

```json
{
  "hooks": {
    "Stop": [
      { "matcher": "", "hooks": [
        { "type": "command", "command": "~/.local/bin/agent-notify.sh Claude" } ] }
    ],
    "Notification": [
      { "matcher": "", "hooks": [
        { "type": "command", "command": "~/.local/bin/agent-notify.sh Claude" } ] }
    ]
  }
}
```

- 事件由脚本按 stdin 的 `hook_event_name` 自动分流:`Stop`=完成、`Notification`=需要你(会
  过滤「单纯空闲等待」)。改后需重开 Claude Code。

### 4) 配 Codex(`~/.codex/hooks.json`)

```json
{
  "hooks": {
    "Stop": [
      { "matcher": "", "hooks": [
        { "type": "command", "command": "~/.local/bin/agent-notify.sh Codex" } ] }
    ],
    "PermissionRequest": [
      { "matcher": "", "hooks": [
        { "type": "command", "command": "~/.local/bin/agent-notify.sh Codex" } ] }
    ]
  }
}
```

- `Stop`=一轮完成、`PermissionRequest`=等你批准命令/改文件。
- **首次要信任**:codex 里 `/hooks` 或启动时「Trust all and continue」信任一次(按 hash,
  脚本改了要重信)。多个 CODEX_HOME 各配一份(如 `~/.codex-crs/hooks.json`)。

## 环境开关

| 变量 / 文件 | 默认 | 作用 |
|---|---|---|
| `AI_NOTIFY_THROTTLE_SEC` | `25` | 同「来源:项目」的**完成**通知在这么多秒内只弹一次(治 workflow/排队刷屏);**「等你/批准」永不节流**。设 0 关。 |
| `AI_NOTIFY_LOG` | `1` | 记一行到 `~/.local/state/agent-notify.log`(时间/来源/事件/notify·skip·throttled/内容)。设 0 关。 |
| `~/.local/state/ai-notify-off`(文件) | 不存在 | 存在时**静音所有通知**、立即退出(测试/临时关用)。删掉即恢复。 |

## 安全

- Codex `PermissionRequest` 的处理**只弹通知、绝不代你决定**:脚本 stdout 恒为空、退出 0,故
  codex 照常走它自己的审批提示,**永不自动批准**(只有显式输出 `decision.behavior="allow"`
  才会自动批准,脚本刻意不输出)。

## 已知局限

- **Codex「问你问题 / 让你选(elicitation / requestUserInput)」抓不到**:codex 的钩子事件里
  没有「向用户提问」这一类(只有 完成 / 批准 / 工具调用 / 会话生命周期),这类交互只存在于
  codex 的 **app-server 协议**里。想覆盖它需要走 app-server 客户端(更重),不在本脚本范围。
- 交互式 codex 也不自吐 OSC;`codex exec` 等非交互场景本就发 OSC 9,不需要本桥接。
