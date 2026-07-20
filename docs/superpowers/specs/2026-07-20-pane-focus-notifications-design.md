# 设计提案:面向 pane 的可点击通知(Pane-Focus Notifications)

- 日期:2026-07-20
- 状态:设计草案(基于 6 份子系统调研,待维护者对齐)
- 关联需求:`docs/superpowers/specs/2026-07-20-pane-focus-notifications-requirements.md`
- 关联目标:`docs/superpowers/specs/2026-07-20-pane-focus-notifications-goal.md`
- 代码根:`vendor/warp-stripped`(以下所有 file:line 相对此根,除非另注)

---

## 0. 核心结论(先说最重要的)

**这不是从零起步,而是"解门控 + 补策略"任务。** 调研的 6 个子系统一致证明:需求里被当作"要从头做"的三大块——OSC 9/777 解析、桌面通知投递、点击→聚焦发出通知的那个 pane——在本 fork 里**已经端到端打通并可复用**。

已验证的三个事实(本文档已核对源码):

1. `NotificationContext`(`app/src/notification.rs:11`)当前**只有** `BlockOrigin{window_id, pane_group_id, pane_id}` 一个变体,已经是"精确定位发通知的那个 pane"的现成载体,serde 往返 + macOS 点击回调 + `focus_pane` 全链路可用。
2. `is_navigated_away_from_window`(`app/src/terminal/view.rs:19638`)确实**只做窗口级判定**(`Some(ctx.window_id()) != active_window`),无法支撑"该 pane 在前台就不弹"的 pane 级选项。
3. `FeatureFlag::PluggableNotifications` 在 omw 本地 build 里被**双重关闭**:既列在 `OMW_LOCAL_DISABLED_FLAGS`(`app/src/lib.rs:2568`),又受 `#[cfg(feature = "pluggable_notifications")]` 门控(`app/src/lib.rs:2988`)。**这是整个特性在默认 omw 构建里根本不触发的根因,必须首先解决。**

因此本设计的主轴是:**复用现有链路,只补齐 5 类增量** —— (A) 解门控让特性在 omw 默认开;(B) 把"仅离开窗口才弹"改成"默认始终弹"并加 pane 级前台抑制;(C) 事件分类(完成 vs 等待批准)+ 自定义声音;(D) 去重/节流/勿扰;(E) 设置 UI。P2 的 agent turn 识别**复用同一投递/聚焦/配置地基**,只是触发源从"程序发转义"换成"omw 自己判定"。

---

## 1. 架构总览(数据流)

```
                         ┌──────────────── P1: 转义源 ────────────────┐
  程序写 OSC 9 / OSC 777 字节流
      │
      ▼
  [解析] terminal/model/ansi/mod.rs:811 osc_dispatch
      ├ b"9"   :884  →  handler.pluggable_notification(None, body)
      └ b"777" :1012 →  handler.pluggable_notification(title, body)   ← 已就绪, 基本免改
      │
      ▼
  grid/ansi_handler.rs:522 pluggable_notification → event_proxy.send_terminal_event
      │  (per-session ChannelEventListener 天然携带来源身份)
      ▼
  terminal_model.rs:3562  Event::PluggableNotification   ← gated FeatureFlag::PluggableNotifications
      ▼
  model_events.rs:306  → ModelEvent::PluggableNotification
      ▼
 ┌────────────────────────── 统一决策层 ──────────────────────────┐
 │ terminal/view.rs:11295  (self.view_id = pane 身份)             │
 │   ★ P1 改造主战场:                                             │
 │   - 读设置决定"是否弹 / 何时弹"(默认始终弹, 不再只看 away)   │
 │   - pane 前台抑制(新 is_pane_in_foreground, 默认关)           │
 │   - 事件分类 kind(完成/等待批准) → 选声音/文案模板            │
 │   - 去重/节流/勿扰 gate                                          │
 └────────────────────────────────────────────────────────────────┘
      │  emit Event::SendNotification(BlockNotification)
      ▼
  pane_group/pane/terminal_pane.rs:678,684  给通知补 pane_id
      ▼
  workspace/view.rs:13023  ★唯一 OS 投递点★
      - 组装 NotificationContext::BlockOrigin{window_id, pane_group_id, pane_id}
      - UserNotification::new_with_sound(...)  ← 声音/模板/去重挂载点
      - ctx.send_desktop_notification
      ▼
  [平台投递] warpui_core platform trait send_desktop_notification
      ├ macOS: warpui/.../mac/objc/notifications/notifications.m  (UNUserNotification, 完整)
      └ Linux: warpui/.../winit/notifications/linux.rs  (notify_rust, 半残: 无 data/无声音/无点击)
      │
      ▼ (用户点击)
  [点击回调] macOS objc app.m:440 → mac/app.rs:461 → callbacks.on_notification_clicked
      ▼
  app/src/lib.rs:2105  on_notification_clicked
      - 反序列化 NotificationContext → 校验 window_id 仍存在
      - dispatch "root_view:handle_notification_click" + PaneViewLocator
      ▼
  root_view.rs:2547 handle_notification_click → root_view.rs:2520 focus_pane
      - show_window_and_focus_app(抬窗口) + Quake 特判
      - workspace.focus_pane(locator)
      ▼
  workspace/view.rs:5315 focus_pane
      - 按 pane_group_id 选 tab
      - PaneGroup::focus_pane_by_id(pane_id) 选 pane (顺序敏感: 先 pane 后 activate tab)


  ┌──────────────── P2: 原生 agent turn 识别(复用上面下半段)────────────────┐
  agent 插件 OSC777 sentinel "warp://cli-agent" / Codex OSC9 纯文本
      ▼
  cli_agent_sessions/listener/mod.rs:172  订阅 ModelEvent::PluggableNotification
      ▼  try_parse + handle_event (event/mod.rs:71, v1.rs)
  cli_agent_sessions/mod.rs:382 update_from_event → :153 apply_event
      - Stop            → Success (turn 完成)
      - PermissionRequest/QuestionAsked → Blocked (等待你)
      ▼  emit CLIAgentSessionsModelEvent::StatusChanged{terminal_view_id, agent, status, session_context}
  ai/agent_management/agent_management_model.rs:147 handle_cli_agent_session_event
      ★ P2 接入点: 在此(或 add_notification :394)扇出到 OS 通知渠道
      - 已归一化 Success/Blocked → title/message/category + terminal_view_id
      - 已有 is_terminal_view_visible(:410) → 复用做前台抑制
      ▼  最终仍走 workspace SendNotification 带 pane_id(与 P1 同一地基), 而非新造一条链
```

**关键设计原则:任何新触发源(OSC / agent turn)只要最终 emit `pane_group::Event::SendNotification{notification, pane_id}`,就自动获得"投递 + 可点击定位 + 聚焦"能力,不必碰投递层与聚焦层。**

---

## 2. 模块划分与接入点汇总(具体到 file:line)

按"改动性质"分四组。标注 `[免改]` 的是已就绪只需复用,`[改]` 是本期要动,`[新]` 是新增,`[待核实]` 是调研未定死需实现时核对。

### 2.1 解析层(子系统 A)—— 基本免改

| 位置 | 性质 | 说明 |
|---|---|---|
| `terminal/model/ansi/mod.rs:811` osc_dispatch | [免改] | OSC 分发总入口,`match params[0]` 按数字字符串分派。 |
| `terminal/model/ansi/mod.rs:884` (OSC 9) | [免改/可选改] | 已能区分 ConEmu 子命令、freeform 文本。若要 P1 携带 kind,可扩子参数(见 §7 开放问题 1)。 |
| `terminal/model/ansi/mod.rs:1012` (OSC 777) | [免改] | 要求 `params[1]==b"notify"`,提取 title/body 已就绪。 |
| `terminal/model/ansi/mod.rs:1000` `PromptMarker::try_from(&params[1..])` | [复用范式] | 给 OSC 加 typed 子命令的可照抄模式,若 P1 决定让 OSC 9 携带 kind 用此范式。 |

### 2.2 Handler / 事件层(子系统 A)

| 位置 | 性质 | 说明 |
|---|---|---|
| `terminal/model/ansi/handler.rs:401` `pluggable_notification` 默认空实现 | [可选改] | 若给通知加 `kind` 需扩签名 `(title, body, kind)`;**注意 alt_screen/block/blocks/early_output 各 impl 都要同步**,漏改编译失败。**P1 建议先不改签名**,kind 由 P2 的 `CLIAgentEventType` 提供,P1 OSC 走"通用通知"单一类别(见 §6 P1/P2 边界)。 |
| `grid/ansi_handler.rs:522` `pluggable_notification` 实现 | [条件改] | 仅当上面改签名时同步。 |
| `grid/ansi_handler.rs:548` `bell` | [免改] | Bell 走 C0 路径,`Event::Bell`→`request_user_attention`(view.rs:10080),本期不动。 |
| `terminal/event.rs:58,155` `Event::Bell` / `Event::PluggableNotification` | [条件改] | 仅当加 kind 字段时改。 |

### 2.3 决策层(子系统 A/C/F)—— **P1 主战场**

| 位置 | 性质 | 说明 |
|---|---|---|
| `terminal/view.rs:11295` `ModelEvent::PluggableNotification` 处理块 | **[改]** | 现状:仅 `is_navigated_away_from_window` 为真才发桌面通知。需改为:读新设置 → 默认始终弹;`suppress_when_pane_foreground` 为真时用新的 pane 级判定抑制;接去重/节流/勿扰;按 kind 选声音。`self.view_id` 是 pane 归属源头。 |
| `terminal/view.rs:11304-11313` Codex-listener 抑制 | **[改/待核实]** | 现状会吞掉 title=None 的裸 OSC 9(若该 pane 有 Codex listener),可能违反"任意工具可用"。建议改为**仅抑制 P2 sentinel 通知,放行裸 OSC 9**。 |
| `terminal/view.rs:19638` `is_navigated_away_from_window` | [免改,旁边加新方法] | 保留;新增 `is_pane_in_foreground`(见 2.7)。 |
| `terminal/view.rs:796-802` `NotificationsTrigger` 枚举 | **[改]** | 新增变体 `EscapeSequence`(P1)/`AgentTurnComplete`/`AgentNeedsApproval`(P2)。 |
| `terminal/view.rs:836+` `create_notification_content` | **[改]** | 加文案分支 + 模板变量渲染(项目名/pane 标题/耗时/来源工具)。 |

### 2.4 投递层(子系统 B/F)

| 位置 | 性质 | 说明 |
|---|---|---|
| `pane_group/pane/terminal_pane.rs:678-695` | [免改/条件改] | pane_id 已附加到 SendNotification 与 ShowToast;若上层加 kind/urgency 字段需在此透传。 |
| `workspace/view.rs:13023-13077` `SendNotification` 处理 | **[改]** | 唯一 OS 投递点。DND 检测、去重/节流、pane-前台-抑制的最终 gate、按 kind 选声音都插在 `send_desktop_notification` 之前。`UserNotification::new_with_sound`(:13044)是唯一挂声音点。 |
| `notification.rs:11` `NotificationContext` | [免改/可选加变体] | `BlockOrigin` 已够 P1/P2 用(pane 三元组)。仅当 P2 需携带 session/turn 元信息时加变体;`lib.rs:2109` match 加分支即可,focus 路径 100% 复用。 |
| `crates/warpui_core/src/notification.rs:11-72` `UserNotification` | **[改]** | `play_sound: bool` → 升级为 `sound: SoundSpec` 枚举(见 §4.3)。这是投递层唯一实质扩展。 |
| `crates/warpui/.../mac/objc/notifications/notifications.m:46-101` | **[改]** | `content.sound` 从写死 `defaultSound` 改为 `UNNotificationSound soundNamed:`;`identifier` 从写死 `@"CUSTOMIZED_NOTIFICATION"`(:77)改为 **per-pane**(含 pane_id),实现"同 pane 覆盖合并"而非"跨 pane 串台"。**这是投递层必须动的一处。** |
| `crates/warpui/.../mac/delegate.rs:329-353` send FFI | **[改]** | 增加 soundName / identifier 参数透传。 |

### 2.5 点击→聚焦层(子系统 B/C)—— 几乎免改

| 位置 | 性质 | 说明 |
|---|---|---|
| `app/src/lib.rs:2105-2133` `on_notification_clicked` | [免改/可选改] | 反序列化 + 校验 window_id + dispatch。仅当"点击聚焦可配/聚焦行为分级"时在此按设置分派。 |
| `root_view.rs:2547` `handle_notification_click` → `root_view.rs:2520` `focus_pane` | [免改] | 抬窗口 + Quake 特判 + `workspace.focus_pane`。**新增聚焦入口必须经此,勿绕过(否则漏 Quake 状态)。** |
| `workspace/view.rs:5315` `focus_pane` | [免改] | 选 tab + 选 pane;**顺序敏感**(先 focus pane 再 activate tab,见 :5322 注释)。 |
| `pane_group/mod.rs:5256` `focus_pane_by_id` / `mod.rs:6321` `focus_pane` | [免改] | 最内层,对失效 id 有 no-op 保护(不崩)。 |
| `workspace/util.rs:13` `PaneViewLocator` | [免改] | 稳定标识(pane_group_id + pane_id 都是实体 id,拖动/重排不失效)。 |

### 2.6 特性开关(子系统 F)—— **必须首先处理的解门控**

| 位置 | 性质 | 说明 |
|---|---|---|
| `crates/warp_features/src/lib.rs:865` `OMW_LOCAL_FLAGS` | **[改]** | 加入 `FeatureFlag::PluggableNotifications`(让 omw 本地 build 默认开)。 |
| `app/src/lib.rs:2568` `OMW_LOCAL_DISABLED_FLAGS` | **[改]** | **删除** `FeatureFlag::PluggableNotifications` 行——否则 `enabled_features()` 末尾(:3079-3083)会在 omw_local_mode 下无条件 remove 它。**这两处必须成对处理**(已核实两处均存在)。 |
| `app/src/lib.rs:2988` `#[cfg(feature = "pluggable_notifications")]` | **[改/待核实]** | 该 cargo feature 是否在 omw build profile 里启用需核实;若否需在 Cargo.toml 打开或去掉 cfg 门控。 |
| `crates/warp_features/src/features_test.rs:33` | **[新]** | 照 `omw_local_flags_enable_codex_formula_rendering` 样板加回归测试,锁死 `OMW_LOCAL_FLAGS` 含它 + `OMW_LOCAL_DISABLED_FLAGS` 不含它。 |

### 2.7 设置层(子系统 D)—— 扩展现有结构

| 位置 | 性质 | 说明 |
|---|---|---|
| `terminal/session_settings.rs:69` `NotificationsSettings` 结构 | **[改]** | 加字段(见 §4.1)。**依赖已有 `#[serde(default)]`(已核实在 :69)+ 更新 Default impl(:99)保证向后兼容**——历史真出过反序列化事故(:62 注释引 PR)。 |
| `terminal/session_settings.rs:40` `NotificationsMode` 附近 | **[新]** | 新增 `NotificationSound` / `NotificationEvent` 枚举(见 §4.3)。 |
| `settings_view/features_page.rs:4957-5033` 通知区 | **[改]** | 每个新开关加 `FeaturesPageAction` 变体 + dispatch handler(照抄 :1510)+ 渲染行(:3740 `render_notification_toggle`);下拉用 :2768 范式;路径 text_input 用 :3706。 |
| `settings_view/mod.rs:191/1281/1318` + 新建 `notifications_page.rs` | **[新/可选]** | 需求第5条要"独立通知区"。方案 B(推荐):照 `omw_agent_page.rs` 建独立页,`#[cfg(feature="omw_local")]`。方案 A(小改):Features 页内 `render_group` 分组。 |
| `app/src/settings/init.rs:61` `SessionSettings::register` | [免改] | 复用 `SessionSettings.notifications` 单一数据源,不新建 group(避免配置分裂)。 |

### 2.8 新增 pane 级前台判定(子系统 C)

**[新]** 在 `terminal/view.rs` 新增 `is_pane_in_foreground(&self, ctx) -> bool`:
`active_window == window_id` && 该 pane 所在 tab 是 workspace 活动 tab && pane 是该 pane_group 的 `focused_pane_id`。
- 取数路径 **[待核实]**:`TerminalView` 的 `ViewContext` 里能否直接拿到 Workspace 活动 tab / `PaneGroup::focused_pane_id`,可能需经 pane_group 上溯或从 workspace 查。子系统 E 已有现成 `is_terminal_view_visible`(`agent_management_model.rs:410`,:466 判定 active pane 或同 tab)——P2 直接复用它,P1 可参考其取数逻辑。

### 2.9 P2 信号源(子系统 E)—— 复用现成 turn 状态机

| 位置 | 性质 | 说明 |
|---|---|---|
| `cli_agent_sessions/mod.rs:18` `CLIAgentSessionStatus` | [免改] | InProgress / Success(=Stop=turn 完成)/ Blocked(=PermissionRequest/QuestionAsked=等待你)。 |
| `cli_agent_sessions/mod.rs:220` `StatusChanged` 事件 | [免改] | 携带 terminal_view_id + session_context(project/cwd/query/summary/display_title,做模板变量)。 |
| `ai/agent_management/agent_management_model.rs:147` `handle_cli_agent_session_event` | **[改]** | **P2 首选接入点**:已归一化 Success/Blocked → title/message/category + terminal_view_id。在此扇出到 OS 通知渠道。 |
| `ai/agent_management/agent_management_model.rs:394` `add_notification` | **[改/可选]** | 更干净:所有 CLI+Warp 通知收敛于此,已有 `is_terminal_view_visible`(前台抑制)+ `NotificationCategory`(→声音/文案)。 |
| `cli_agent_sessions/listener/mod.rs:87` `CodexSessionHandler` | [免改/可选增强] | Codex OSC9 无类型,一律当 Stop/Success,`supports_rich_status=false`。**Codex 只有"完成"粒度**;要"等待批准"需脆弱文本匹配,建议保守方案不做。 |

---

## 3. 通知点击 → 聚焦 pane 的完整回路设计

**结论:全链路已存在,本期复用,不新造。** 回路:

1. **发送时就地捕获来源 pane**:`workspace/view.rs:13029` 用当前 `pane_group.id()` + `window_id` + 事件里的 `pane_id` 组成 `NotificationContext::BlockOrigin`,`serde_json` 序列化进 `UserNotification.data` 随通知发出。pane_id 来源:`terminal_pane.rs:678` 用 `terminal_pane_id.into()`。
2. **macOS 投递**:`data` 塞进 `content.userInfo[DATA]`(notifications.m)。
3. **点击回调**:objc `didReceiveNotificationResponse`(app.m:440)→ `warp_app_notification_clicked`(mac/app.rs:461)→ `response_from_native` 还原 data → `callbacks.on_notification_clicked`。
4. **app 分派**:`lib.rs:2105` 反序列化为 `BlockOrigin`,校验 window 存在,`dispatch_action("root_view:handle_notification_click", PaneViewLocator{pane_group_id, pane_id})`。
5. **聚焦三级**:`handle_notification_click`(root_view.rs:2547)→ `focus_pane`(root_view.rs:2520):(a) `show_window_and_focus_app` 抬窗口;(b) Quake 状态处理;(c) Terminal 态下 `workspace.focus_pane(locator)`(view.rs:5315)选 tab + 选 pane。**正好对应需求的"抬窗口 + 选 tab + 选 pane"。**

**多 agent 不误跳**:每条通知带自己的 pane 三元组,`terminal_view_id`/pane_id 一一对应,天然隔离。

**P2 点击聚焦**:子系统 E 现有 toast 用 `WorkspaceAction::FocusTerminalViewInWorkspace{terminal_view_id}`(toast_stack.rs:257)。**[待与子系统 B/C 交叉确认]**:P2 走原生 OS 通知时应统一走 `NotificationContext::BlockOrigin` + `root_view:handle_notification_click`(与 P1 同一路径),而非 `FocusTerminalViewInWorkspace`——需把 `terminal_view_id` 映射到 `PaneViewLocator{pane_group_id, pane_id}`。若映射不便,则退回复用 `FocusTerminalViewInWorkspace` 但需确认它也走 `RootView::focus_pane`(经 Quake 特判)。**这是 P2 必须钉死的一致性问题。**

**健壮性(可接受的边缘 no-op)**:
- window 已关:`lib.rs:2117` 只在 window 存在时 dispatch,否则静默失败。可选 fallback:按 pane_id 全局查找(照 `workspace/view.rs:5363` `focus_terminal_view_in_other_window` 模式)。**建议本期不做,文档说明。**
- pane 已关:`PaneGroup::focus_pane`(mod.rs:6331)+ `Workspace::focus_pane`(find 不到静默返回)对失效 id no-op,不崩,用户感觉"点了没反应"。可接受。

---

## 4. 设置项设计

**单一数据源:全部并入现有 `SessionSettings.notifications`(`toml_path = notifications.preferences`,`max_table_depth: 1`,`sync_to_cloud = Globally`)。** 不新建 group,避免配置散两处。

### 4.1 `NotificationsSettings` 新增字段(TOML 键 / 默认值)

现有字段保留。新增(全部依赖 `#[serde(default)]` + 更新 Default impl):

| 字段名 | 类型 | 默认 | TOML 键(`[notifications]`) | 语义 |
|---|---|---|---|---|
| `is_escape_sequence_enabled` | `bool` | `true` | `is_escape_sequence_enabled` | P1:OSC 9/777 通用通知总开关(按事件类型) |
| `always_notify` | `bool` | `true` | `always_notify` | "何时弹"默认始终弹(事件驱动,不看在不在场) |
| `suppress_when_pane_foreground` | `bool` | `false` | `suppress_when_pane_foreground` | 可选:发通知的 pane 已在前台时不弹 |
| `focus_on_click` | `bool` | `true` | `focus_on_click` | 是否点击聚焦 |
| `focus_behavior` | `FocusBehavior` 枚举 | `RaiseSelectTabAndPane` | `focus_behavior` | 聚焦行为分级(抬窗/选tab/选pane),下拉呈现 |
| `respect_system_dnd` | `bool` | `true` | `respect_system_dnd` | 尊重系统勿扰/专注 |
| `throttle_window_secs` | `u64`(Duration) | `5` **[待产品定]** | `throttle_window_secs` | 同 pane 合并/节流窗口 |
| `event_sounds` | `HashMap<NotificationEvent, NotificationSound>` | 见 §4.3 | `event_sounds` | 按事件类型配声音 |
| `title_template` / `body_template` | `String`(可选) | 空=用内置文案 | `title_template` / `body_template` | 模板变量文案 |

- **总开关**:复用现有 `mode`(Enabled/Disabled/...)作整体总开关。**[待定]** 需求要"默认开",但现有 `mode` 默认是 `Unset`(非 Enabled)——改默认会牵连 `terminal/view.rs:14335` 的"未配置 banner"逻辑。**建议**:保留 `mode=Unset` 语义不动,新特性用 `is_escape_sequence_enabled`(默认 true)独立控制 P1;整体"默认开"通过 feature flag 解门控 + 各 `is_*_enabled` 默认 true 达成,不动 `mode` 默认。**这是设置层最需要拍板的一处(见 §7 开放问题 2)。**
- **cloud sync**:`event_sounds` 里的 `File(path)` 是 per-machine 项,同步到别机器会失效。**[待定]** 该字段可能需标 `Never`/`private` 或用 `current_value_is_syncable()` 过滤(参考 `github_pr_chip_default_validation`)。

### 4.2 新增枚举

```rust
// session_settings.rs:40 附近, 各自 derive Serialize/Deserialize/PartialEq/JsonSchema/SettingsValue
enum NotificationEvent { TaskCompleted, NeedsAttention, Generic }  // per-event 声音 map 的 key

enum FocusBehavior {          // 下拉, 需 as_dropdown_label()
    None,                     // 不聚焦(与 focus_on_click=false 等价, 二选一)
    RaiseWindowOnly,          // 仅抬窗口
    RaiseAndSelectTab,        // 抬窗口 + 选 tab
    RaiseSelectTabAndPane,    // 全量(默认)
}
```

### 4.3 声音与自定义音频(§6.6 需求)

```rust
enum NotificationSound {
    None,                 // 静音
    Default,              // 系统默认音
    Named(String),        // 内置音名(macOS 系统音 / bundle 内置)
    File(PathBuf),        // 用户自定义音频文件
}
```

- 存储:`event_sounds: HashMap<NotificationEvent, NotificationSound>`(HashMap 已被 SettingsValue 支持,参考 `QuakeModeSettings` 的 `pin_position` map)。
- **投递层改造**:`UserNotification.play_sound: bool` → `sound: SoundSpec`;macOS `notifications.m` 的 `content.sound` 从写死 `defaultSound` 改 `UNNotificationSound soundNamed:`。
- **自定义文件的 macOS 限制 [待核实]**:`UNNotificationSound soundNamed:` 要求音频文件在 app bundle 或 `~/Library/Sounds`,**不支持任意路径**。可能需在发通知前把用户文件拷入 `~/Library/Sounds`,或降级为"仅内置名 + Default"。这决定"自定义音"是纯上层还是要改打包脚本。
- **无原生文件选择器**:全仓 grep 无 rfd/NSOpenPanel,"自定义声音文件"UI 只能 **text_input 输入路径**(复用 :3706 范式)+ `validate` 校验存在/可读/扩展名,非法回落内置默认音。

### 4.4 模板变量

- 设置体系只存模板字符串;**变量解析在发通知层做**(`create_notification_content` view.rs:836 / workspace/view.rs:13023 前)。
- 变量清单 **[待对齐]**:`{project}`(session_context.project)/ `{pane_title}` / `{tool}`(来源工具)/ `{duration}`。**耗时目前 session_context 无字段**,需另算(turn 开始时间戳)或新增,或本期先不支持 `{duration}`。

### 4.5 UI 落点

- **推荐方案 B(独立通知页)**:照 `omw_agent_page.rs` 全套(View + SettingsPageMeta + SettingsWidget + TypedActionView),`#[cfg(feature="omw_local")]`,与 OmwAgent 并列注入 nav(mod.rs:191/1281/1318)。符合需求"独立通知区"+ omw 特色。
- 组件复用:总开关 `render_body_item`+switch;子项 `render_notification_toggle`(:3740);枚举 `Dropdown`+`update_*_dropdown`(:2768);阈值/声音路径 `text_input EditorView`(:3706);分组 `render_group`;平台门控 `add_setting`(:1168);非同步项标 `LocalOnlyIconState`。

---

## 5. 勿扰、合并/节流、平台抽象

### 5.1 勿扰(DND)

- **macOS 免费部分**:`UNUserNotificationCenter` 会自动尊重系统 Focus/DND(静默压入通知中心)。**"尊重勿扰"的最低要求 mac 天然满足。**
- **主动检测的坑 [风险]**:macOS **无稳定公开 DND 查询 API**(旧法读 `com.apple.notificationcenterui doNotDisturb` 在新版失效/需私有 API)。
- **决策(建议 MVP)**:走 **best-effort**——`respect_system_dnd` 设置先接到"是否 playSound"层面(DND 时静音但仍投递,由系统压制),不做"检测到 DND 完全不发"。Linux(notify_rust/dbus)同样无可靠 DND 查询,同策略。

### 5.2 合并 / 去重 / 节流

- **两条机制并用**:
  1. **macOS identifier per-pane**:把 `notifications.m:77` 写死的 `identifier=@"CUSTOMIZED_NOTIFICATION"` 改为含 pane_id → **同 pane 后一条自动覆盖合并**,跨 pane 不串台。(需求"同 pane 合并 + 跨 pane 不误跳"的核心机制。)
  2. **应用层节流表**:在投递前(`workspace/view.rs:13023` 之前)维护 `HashMap<key, (Instant, content_hash)>`,窗口内同 key 抑制/合并。复用现成 `app/src/throttle.rs` / `debounce.rs` Stream 组合子。
- **key 粒度 [待产品定]**:`pane + event_type`(**建议含 event_type**,否则"完成"与"等待批准"会被误合并——子系统 E/F 均警示)。
- **状态放哪 [风险]**:`WorkspaceView` 是每窗口一个,跨窗口同 pane 去重需 **App 级状态**。建议节流表挂 App 级,否则多窗口漏合并。

### 5.3 平台抽象与渠道 trait(为 P2/phase2 铺路)

- **投递已经是平台抽象**(warpui_core `platform/mod.rs:232` trait:`send_desktop_notification` / `request_permissions`;mac=objc,linux=notify_rust)。**不要改 warpui 平台层**(它是系统通知后端,不是渠道插件点)。
- **[新] 在 app 层再抽一层 `NotificationChannel` trait**:默认实现 = 现有 `send_desktop_notification`。为需求 §6"未来转发手机 / Tailscale"预留。DND 检测也做成该 trait 的方法。**本期只落 trait + 默认 desktop 实现,不做转发渠道**(明确 Out)。

### 5.4 Linux 缺口(本期"留抽象",可选实现)

- `winit/notifications/linux.rs:18` 当前**丢弃 data + 无声音 + 无点击回调**(只 on_close 打日志);`platform/app.rs:104` `notification_clicked` 在 Linux 标 dead_code。
- **要 Linux 点击直达需补**:notify_rust action + 透传 data + 新 `CustomEvent::NotificationClicked`(照 `winit/app.rs:83` `SendNotification` 范本)→ `callbacks.notification_clicked`。**工作量非平凡。**
- **本期决策**:需求说 Linux 只需"留通知层抽象接口"。**建议 P1 Linux 仅弹不可点击(投递可用,点击 no-op),把点击回路作为后续项。** macOS 全功能。

---

## 6. P1 / P2 边界

| | P1(OSC 通用通知) | P2(原生 agent turn 识别) |
|---|---|---|
| 触发源 | 程序写 OSC 9/777 | omw 消费 `CLIAgentSessionsModelEvent::StatusChanged` |
| 事件分类 | **单一"通用通知"类别**(裸 OSC 9 无 kind) | Success→完成 / Blocked→等待批准(现成) |
| 接入点 | `terminal/view.rs:11295` | `agent_management_model.rs:147/394` |
| 投递/聚焦/配置 | 共用地基 | **共用同一地基**(最终走 workspace SendNotification 带 pane_id) |
| 前台抑制 | 新 `is_pane_in_foreground` | 复用现成 `is_terminal_view_visible`(:410) |

**关键边界决策**:OSC 9 裸文本**天生无 kind**。因此:
- **P1 只做"通用通知"单一类别**(声音用 `NotificationEvent::Generic`)。
- **"完成 vs 等待批准"两类事件的区分交给 P2 的 `CLIAgentEventType`**(Stop→Success、PermissionRequest/QuestionAsked→Blocked,现成)。
- 若维护者坚持 P1 也要区分两类,则需约定 OSC 9 子参数(照 `PromptMarker::try_from` 范式)携带 kind——**见 §7 开放问题 1,需先与维护者对齐 OSC 选型。**

---

## 7. 风险与开放问题(需维护者拍板)

### 最关键 3 个开放问题

1. **【OSC kind 语义】P1 纯 OSC 9 如何携带"完成/等待批准"分类?**
   - 选项 A(推荐):P1 只做单一"通用通知",两类区分留给 P2 的 `CLIAgentEventType`。
   - 选项 B:约定 OSC 9 子参数(照 `PromptMarker::try_from` 范式)带 kind,需扩 `Handler::pluggable_notification` 签名(牵连 4 个 impl)。
   - 影响需求第4点(覆盖两类事件)+ 按事件配声音/开关。**必须先定,决定是否改 handler 签名。**

2. **【总开关默认语义】"默认开"如何与现有 `mode=Unset` 协调?**
   - 现有 `mode` 默认 `Unset`(非 Enabled),改默认会牵连 `terminal/view.rs:14335` 未配置 banner。
   - 建议:不动 `mode` 默认,用新 `is_escape_sequence_enabled`(默认 true)+ feature flag 解门控达成"默认开"。**需确认不破坏 banner 逻辑。**

3. **【自定义声音的 macOS 可行性】`UNNotificationSound soundNamed:` 只认 bundle/`~/Library/Sounds`,不认任意路径。**
   - 决定"按事件配自定义音频文件"是纯上层(拷文件入 `~/Library/Sounds`)还是要改打包脚本,或本期降级为"仅内置名 + Default"。**需实测 macOS 行为。**

### 其他风险

- **R1 成对修改陷阱**:`OMW_LOCAL_FLAGS` 加 + `OMW_LOCAL_DISABLED_FLAGS` 删必须同做(已核实两处),否则 flag 仍关。加回归测试锁死。
- **R2 门控冲突**:`view.rs:11295` 现"仅离开窗口才弹"与需求"默认始终弹"相反;改动须受新设置约束,且不破坏 long-running/agent 既有触发语义。
- **R3 Codex-listener 抑制**:`view.rs:11304` 会吞裸 OSC 9(若 pane 有 Codex listener),可能违反"任意工具可用"。建议仅抑制 sentinel。
- **R4 identifier 覆盖**:mac 写死常量 identifier 会跨 pane 串台/丢通知,必须改 per-pane。
- **R5 handler 签名同步**:若改 `pluggable_notification` 签名,alt_screen/block/blocks/early_output 全部 impl 要同步,漏改编译失败。
- **R6 节流状态层级**:跨窗口同 pane 去重需 App 级状态,Workspace 级会漏合并。
- **R7 P2 flag 依赖**:`AgentMode` 在 omw 下被禁(`lib.rs:2453`),P2 复用 cli_agent_sessions 需确认上游 flag(HOANotifications 等)在 omw 下可用。
- **R8 未装插件的 agent**:P2 结构化事件依赖 agent 端 Warp CLI 插件(`view.rs:11640`);未装插件时 session 停在 InProgress 永不 emit Success/Blocked。需在设计/UI 说明或提示装插件。

### 与上游对齐要点

1. OSC 选型与命名(是否扩子参数带 kind),与上游 Warp `PluggableNotifications` 约定的兼容。
2. 点击聚焦统一走 `root_view:handle_notification_click` + `PaneViewLocator`(勿绕 Quake 特判);P2 是否复用还是走 `FocusTerminalViewInWorkspace`,需 B/C/E 交叉钉死。
3. 配置键命名与 `[tui]`/`[desktop]`/`appearance.panes` 风格一致;独立通知页是否 `#[cfg(feature="omw_local")]`。
4. 默认声音选哪个(功能默认开、默认始终弹已定)。
5. P2 turn 边界信号复用 `Stop`(Success)/ `PermissionRequest`+`QuestionAsked`(Blocked)——已现成。
6. 作为上游特色功能的对外表述与文档位置。
