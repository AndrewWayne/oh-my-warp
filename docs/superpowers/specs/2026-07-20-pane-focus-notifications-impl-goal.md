# Goal 提示词(完整实施):omw 面向 pane 的可点击通知

> 直接作为编码 agent / 会话在 omw 仓库内的首条指令。执行以三份文档为准,本文件是总纲。

---

## 角色与环境

你是在 oh-my-warp 仓库工作的资深 Rust/GPUI 工程师,目标是把"面向 pane 的可点击通知"实现为可上游的特性,端到端交付(MS0 → P1 → P2)。

- 仓库:`/Users/shuokong/Desktop/oh-my-warp-git`;从 `feat/codex-formula-rendering-v1` 切出工作分支 `feat/pane-focus-notifications`。
- 主源码:`vendor/warp-stripped/app/src`。
- 构建前置已就绪:完整 Xcode(`/Applications/Xcode.app`)、protoc 35.0、rustc(`~/.cargo/bin`,仓库 pin 1.92)。

## 先读(必须,按此为准)

- 需求:`docs/superpowers/specs/2026-07-20-pane-focus-notifications-requirements.md`
- 设计:`docs/superpowers/specs/2026-07-20-pane-focus-notifications-design.md`
- 计划:`docs/superpowers/plans/2026-07-20-pane-focus-notifications-plan.md`

## 最终要达成的效果(验收)

1. 任意 pane 的程序发 OSC 9 / OSC 777 → 弹原生系统通知(带来源信息)。
2. 点击通知 → omw 前置 → 精确切到发出它的**标签 + pane**;多 agent 并行不误跳。
3. 覆盖两类事件:完成/停下等你、需要你回答或批准。
4. omw 设置里可配:总开关(默认开)、按事件开关、"何时弹"(默认始终弹;可选"pane 在前台不弹",默认关)、点击聚焦及行为。
5. 声音可自定义(内置名 + `~/Library/Sounds` 自定义音频,按事件可不同);标题/正文模板变量;扩展点(事件/声音/渠道)显式化。
6. 尊重系统勿扰;同 pane 通知合并、去重节流。
7. 关闭功能即回现状;解析健壮,不影响正常输出与性能(加回归测试锁定)。
8. macOS 优先,Linux 留通知层抽象。

## 已定决策(不要推翻)

- **链路已存在**:`FeatureFlag::PluggableNotifications`、`Event/ModelEvent::PluggableNotification` 已实现但被 flag 双重关闭。本特性是**"解门控 + 补策略"**,复用现有投递/聚焦链路。
- **缺口**:`NotificationContext` 目前只带 `BlockOrigin`;`is_navigated_away` 只做窗口级判定(需升级到 tab/pane 级)。
- **事件分类**:P1 只做**单一"通用通知"**,不动解析/handler 层;"完成 vs 等批准"的区分交给 **P2 的原生识别(CLIAgentEventType)**。
- **默认开**:新增 **`is_escape_sequence_enabled`(默认 true)+ 解门控**;**不改现有 `mode`(Unset 默认),不碰 `terminal/view.rs:14335` 的 banner 逻辑**。
- **自定义声音**:走 **`~/Library/Sounds` + 按名引用**(把用户选的音频拷入该目录再以名引用),纯上层,已实测机制可行。

## 执行顺序(严格按里程碑;每步"编译 + 测试通过 + 真机 smoke"后才进下一步,各自一个 commit)

- **MS0 解门控(阻塞一切)**:把 `PluggableNotifications` 纳入 `OMW_LOCAL_FLAGS` 启用;新增 `is_escape_sequence_enabled`(默认 true)。验收:任意程序 `printf '\e]9;hello\a'` → omw 弹通知 → 点击回到发起 pane 的最小链路在本地 build 跑通。`cargo check -p warp --lib` + smoke。
- **P1**
  - **MS1 精确聚焦**:`NotificationContext` 带 pane/tab locator;`is_navigated_away` 升级到 tab/pane 级;点击聚焦 = 抬窗口 + 选 tab + 选 pane;pane 已在前台则不打断。
  - **MS2 默认始终弹 + 前台抑制**(可选,默认关)。
  - **MS3 事件与声音**:单一通用通知;声音 `~/Library/Sounds` 按名引用;标题/正文模板变量(项目名/pane 标题/来源等)。
  - **MS4 去重/节流/勿扰**:按 pane group 合并;节流 key;尊重系统 DND。
  - **MS5 设置页**:独立"通知"设置区(TOML 键 + 默认 + 校验 + UI),声音选择与模板可配。
- **P2**
  - **MS6 原生 agent turn 识别**:复用 `cli_agent_sessions`,判定 codex/claude 的"turn 完成 / 等待批准",触发通知并按 `CLIAgentEventType` 区分事件,复用 P1 的投递/聚焦/配置地基。

## 工作方式(硬性)

- **TDD 优先**:flag 契约、settings 往返、模板渲染、节流 key 都要有测试。
- 每里程碑:先改 → `cargo check -p warp --lib` + 相关 `cargo test` 通过 → macOS 真机 smoke → 一个 `feat:` commit → 在 `plan.md` 勾选进度。
- **flag 关闭时行为必须与现状完全一致**,加回归测试锁死这个契约。
- 遵循既有模式(参考公式渲染那次的 flag / settings / gate / OMW_LOCAL_FLAGS 与回归测试做法)。
- 不确定的 API:读代码求证,标"待核实",**不要编造**。
- 只做需求内的事;发现设计缺陷就更新设计文档并说明,不擅自扩范围。

## 产出

- 可编译、可演示的 omw 本地 build:OSC 通知 + 点击精确聚焦端到端可用(P1),并含 P2 的 agent turn 原生识别。
- 全套测试通过 + 关键路径回归测试。
- 更新后的 `plan.md` 进度,以及一份**面向其他维护者的 PR 说明草稿**(设计取舍、开放问题、如何开关与配置)。
