# Goal 提示词:omw 面向 pane 的可点击通知

> 供后续 agent / 会话直接作为目标输入。完整验收标准见同目录 `2026-07-20-pane-focus-notifications-requirements.md`。

## GOAL

在 oh-my-warp(仓库:`/Users/shuokong/Desktop/oh-my-warp-git`,当前分支 `feat/codex-formula-rendering-v1`)中,新增"面向 pane 的可点击通知"特性,作为 omw 的上游特色功能。

**做完后要达成:**

1. omw 里任意 pane 的程序发出标准通知转义(OSC 9 / OSC 777)时,弹出原生系统通知(含来源信息)。
2. 点击通知 → omw 前置 → 精确切到"发出该通知的标签",并聚焦其 pane;多 agent 并行不误跳。
3. 覆盖两类事件:"完成/停下等你"、"需要你回答或批准才能继续"。
4. 在 omw 设置里可配:总开关(**默认开**)、按事件开关、"何时弹"(**默认始终弹**;可选"pane 在前台不弹",默认关)、点击聚焦及聚焦行为。
5. 声音可自定义(内置音 + 自定义音频文件,可按事件配不同声音);标题/正文用模板变量;架构为"加事件 / 加声音 / 加通知渠道"预留显式扩展点。
6. 尊重系统勿扰;同 pane 通知合并、去重节流。
7. 关闭功能即回到现状;解析健壮,不影响正常输出与性能。
8. macOS 优先,Linux 留通知层抽象;**本期含 P2**:omw 原生识别 codex/claude 的 turn 完成/等待批准(复用 `cli_agent_sessions`),不依赖程序主动发转义也能触发。

**约束/取向:** 走标准 OSC(对齐 iTerm2/kitty/WezTerm),不用"env 变量+自定义 URL"的外部土办法;配置纳入现有 settings 体系;聚焦复用 `root_view::focus_pane` locator;通知渠道用 trait 便于扩展。

**分阶段:** P1 = OSC 通用通知 + 点击聚焦 + 设置/声音;P2 = 原生 agent turn 识别(共用 P1 地基)。

## 本轮产出(不直接改生产代码)

用 Agent 集群做**源码调研 + 设计提案 + 实施计划**:定位转义解析管线、通知/bell 现状、tab/pane 聚焦机制、通知点击回调路由、settings 体系(TOML+UI)、`cli_agent_sessions` 的 turn 判定信号;把每处**具体接入点(file:line)、要加什么、风险、开放问题**落成设计文档 + 分阶段实施计划。原型编码作为后续步骤。
