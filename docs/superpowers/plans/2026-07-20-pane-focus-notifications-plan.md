# 实施计划:面向 pane 的可点击通知(Pane-Focus Notifications)

- 日期:2026-07-20
- 关联设计:`docs/superpowers/specs/2026-07-20-pane-focus-notifications-design.md`
- 代码根:`vendor/warp-stripped`(所有 file:line 相对此根)
- 分支:`feat/codex-formula-rendering-v1`(建议在其上开 `feat/pane-focus-notifications`)

## 阅读顺序与总原则

- **先解门控,再改行为,后加配置,最后接 P2。** 每一步都能独立 `cargo check`,并尽量给可跑的验证。
- **复用优先**:凡设计文档标 `[免改]` 的链路不碰。改动集中在 5 类增量:解门控 / 行为门控 / 事件分类+声音 / 去重节流勿扰 / 设置 UI。
- **TDD 切入点**:feature flag 契约、settings 序列化往返、模板变量渲染、节流 key 逻辑——这四处是纯逻辑,先写测试。UI 与平台投递靠 smoke。

---

## 里程碑总览

| MS | 内容 | 阶段 | 可验证产物 |
|---|---|---|---|
| MS0 | 解门控:PluggableNotifications 在 omw 默认开 | P1 | 裸 OSC 9 能弹通知(现有链路) |
| MS1 | "始终弹" + pane 前台抑制设置 | P1 | 前台也弹;开关生效 |
| MS2 | 事件分类骨架 + 声音枚举 + per-pane identifier | P1 | 声音可配;同 pane 合并 |
| MS3 | 模板变量 + 文案 | P1 | 标题带项目名/pane 标题 |
| MS4 | 去重/节流/勿扰 | P1 | 连发不刷屏;DND 静音 |
| MS5 | 设置 UI(独立通知页) | P1 | 面板可配全部键 |
| MS6 | P2:agent turn → 原生通知 | P2 | codex/claude 完成/等待自动弹 |
| MS7 | NotificationChannel trait + Linux 抽象留口 | P1/P2 | 渠道可插拔骨架 |

---

## MS0 — 解门控(必须最先做,阻塞一切)

**依赖**:无。**这是所有后续步骤的前置。**

### 步骤

1. **[改]** `crates/warp_features/src/lib.rs:865` `OMW_LOCAL_FLAGS`:加入 `FeatureFlag::PluggableNotifications`。
2. **[改]** `app/src/lib.rs:2568` `OMW_LOCAL_DISABLED_FLAGS`:**删除** `FeatureFlag::PluggableNotifications` 行。
3. **[待核实→改]** `app/src/lib.rs:2988` `#[cfg(feature = "pluggable_notifications")]`:核实该 cargo feature 是否在 omw build profile 启用。若否,在对应 Cargo.toml 启用该 feature,或去掉 cfg 门控让 flag 逻辑生效。
4. **[新/TDD 先行]** `crates/warp_features/src/features_test.rs:33`:照 `omw_local_flags_enable_codex_formula_rendering` 加测试 `omw_local_flags_enable_pluggable_notifications`,断言 `OMW_LOCAL_FLAGS.contains(&PluggableNotifications)`。在 app crate 加断言 `OMW_LOCAL_DISABLED_FLAGS` 不含它(DISABLED 列表在 app crate)。

### 验证

- `cargo test -p warp_features omw_local_flags_enable_pluggable_notifications`
- `cargo check -p <app crate>`
- **Smoke**:omw_local build 启动,在某 pane `printf '\033]9;hello from omw\007'`,应弹系统通知(现有链路);切到别的窗口再发,点击通知应聚焦该 pane。**这一步验证"地基本来就通,只是被关着"。**

### TDD 切入点

先写 MS0 步骤 4 的测试(会失败,因 flag 还在 disabled 列表)→ 改 1/2 → 测试通过。锁死"成对修改"契约(设计 R1)。

---

## MS1 — "始终弹" + pane 前台抑制

**依赖**:MS0。

### 步骤

1. **[改]** `terminal/session_settings.rs:69` `NotificationsSettings`:加字段
   - `is_escape_sequence_enabled: bool`(默认 true)
   - `always_notify: bool`(默认 true)
   - `suppress_when_pane_foreground: bool`(默认 false)
   更新 Default impl(:99)。**依赖已有 `#[serde(default)]`(已核实 :69)。**
2. **[新]** `terminal/view.rs`(`is_navigated_away_from_window` :19638 旁)新增 `is_pane_in_foreground(&self, ctx) -> bool`。
   - **[待核实]** 取数路径:能否直接拿 Workspace 活动 tab / `PaneGroup::focused_pane_id`;参考 `agent_management_model.rs:410/466` `is_terminal_view_visible` 的实现。
3. **[改]** `terminal/view.rs:11295` `ModelEvent::PluggableNotification` 处理块:
   - 读 `is_escape_sequence_enabled`:false 则 return。
   - 把"仅 `is_navigated_away_from_window` 为真才发桌面通知"改为:`always_notify` 为真时**始终**走 `SendNotification`;`suppress_when_pane_foreground` 为真且 `is_pane_in_foreground()` 为真则跳过。
   - **保守**:保留在前台时的 in-app toast 作为并行提示(或按 `always_notify` 决定,见设计开放问题 R2 待定)。
4. **[改/待核实]** `terminal/view.rs:11304-11313` Codex-listener 抑制:改为仅抑制 P2 sentinel(title=="warp://cli-agent"),放行裸 OSC 9。**若与 MS6 冲突可推迟到 MS6 一起改。**

### 验证

- `cargo test -p <app> notifications`(序列化往返,见下 TDD)
- `cargo check`
- **Smoke**:前台 pane 发 OSC 9 → 弹(始终弹生效);在 settings.toml 设 `suppress_when_pane_foreground = true` → 前台不弹、后台弹。

### TDD 切入点

`NotificationsSettings` 序列化往返测试:构造带新字段的 struct → toml round-trip → 断言默认值 + 旧 toml(无新字段)反序列化成功(向后兼容,设计 R2)。这类测试 session_settings 已有先例(toml_path_tests / schema_validation_tests)。

---

## MS2 — 事件分类骨架 + 声音枚举 + per-pane identifier

**依赖**:MS0。可与 MS1 并行(不同文件)。

### 步骤

1. **[新]** `terminal/session_settings.rs:40` 附近:`NotificationEvent { TaskCompleted, NeedsAttention, Generic }`、`NotificationSound { None, Default, Named(String), File(PathBuf) }`、`FocusBehavior` 枚举。全套 derive + SettingsValue。
2. **[改]** `terminal/session_settings.rs:69`:加 `event_sounds: HashMap<NotificationEvent, NotificationSound>`(默认全 `Default`)、`focus_on_click: bool`(true)、`focus_behavior`(RaiseSelectTabAndPane)。
3. **[改]** `crates/warpui_core/src/notification.rs:11` `UserNotification`:`play_sound: bool` → `sound: SoundSpec`(None/Default/Named/File)。更新构造点 `new_with_sound`。
4. **[改]** `crates/warpui/.../mac/delegate.rs:329` send FFI:增加 `soundName` + `identifier` 参数。
5. **[改]** `crates/warpui/.../mac/objc/notifications/notifications.m:46-101`:
   - `content.sound`:`defaultSound` → 按 SoundSpec 选 `UNNotificationSound soundNamed:`(**[待核实]** 自定义文件路径 vs `~/Library/Sounds`)。
   - `identifier`:`@"CUSTOMIZED_NOTIFICATION"`(:77)→ 含 pane_id 的 per-pane 字符串(同 pane 合并、跨 pane 不串)。
6. **[改]** `workspace/view.rs:13044` `UserNotification::new_with_sound`:按事件 kind 从 `event_sounds` 选声音;identifier 用 pane_id。
7. **[改]** `terminal/view.rs:796` `NotificationsTrigger`:加 `EscapeSequence`(P1 用 `Generic`)。P2 变体留 MS6。
8. **[改/条件]** 若决定 P1 也带 kind(设计开放问题 1 选 B):扩 `Handler::pluggable_notification` 签名 + 同步 4 个 impl(handler.rs:401 默认 + alt_screen/block/blocks/early_output)。**默认选 A:P1 不带 kind,本步跳过签名改动。**

### 验证

- `cargo check`(FFI 跨语言边界:确认 mac 编译通过)
- **Smoke(macOS)**:配 `event_sounds` 为不同内置音 → 发通知听到对应音;同 pane 连发两条 → 后一条覆盖(通知中心只留一条);两个 pane 各发 → 互不覆盖、点各自回各自。

### 风险提示

FFI 面广(设计 R4/自定义声音):`UserNotification.sound` 改动波及 warpui_core + mac objc + mac delegate + linux + windows + wasm 六处 send;先只让 mac 全功能,其余保留 Default 行为不 break 编译。

---

## MS3 — 模板变量 + 文案

**依赖**:MS2。

### 步骤

1. **[改]** `terminal/session_settings.rs:69`:加 `title_template: Option<String>` / `body_template: Option<String>`。
2. **[新]** 模板渲染函数(建议放 `terminal/view.rs:836` `create_notification_content` 内或旁):替换 `{project}`/`{pane_title}`/`{tool}`(P1 来源可空)。**[待核实]** `{duration}` 需 turn 开始时间戳,P1 OSC 无此信息,先不支持或留空。
3. **[改]** `create_notification_content`(view.rs:836)+ `NotificationsTrigger` 文案分支:模板为空时用内置文案。

### 验证

- `cargo test` 模板渲染单测(TDD 切入点:给定变量 map + 模板串 → 断言输出;含缺失变量的降级)。
- **Smoke**:设 `title_template = "{project}: done"` → 通知标题正确替换。

---

## MS4 — 去重 / 节流 / 勿扰

**依赖**:MS1(有 pane 前台判定)、MS2(有事件 kind 做 dedup key)。

### 步骤

1. **[改]** `terminal/session_settings.rs:69`:加 `respect_system_dnd: bool`(true)、`throttle_window_secs`(默认 5 **[待产品定]**)。
2. **[新]** App 级节流表(设计 R6:跨窗口同 pane 去重需 App 级,非 Workspace 级)。`HashMap<(PaneId, NotificationEvent), (Instant, content_hash)>`。dedup key **含 event_type**(避免"完成"与"等待批准"误合并,设计 §5.2)。
3. **[改]** `workspace/view.rs:13023` 投递前:
   - 查节流表,窗口内同 key 抑制/合并。
   - `respect_system_dnd` → 接到"是否 playSound"层面(MVP best-effort,DND 时静音仍投递;不做主动 DND 检测,设计 §5.1)。
   - 可复用 `app/src/throttle.rs` / `debounce.rs`。

### 验证

- `cargo test` 节流逻辑单测(TDD:同 key 短时间连发→只留一条;不同 event_type→各留一条;超窗口→放行)。
- **Smoke**:脚本 1 秒内发 5 条同 pane OSC 9 → 系统只弹/合并成 1 条。

---

## MS5 — 设置 UI(独立通知页)

**依赖**:MS1-MS4 的设置字段就绪。

### 步骤(方案 B 独立页,推荐)

1. **[新]** `app/src/settings_view/notifications_page.rs`:照 `omw_agent_page.rs` 全套(View + SettingsPageMeta:1179 返回 section + SettingsWidget + TypedActionView),`#[cfg(feature="omw_local")]`。
2. **[改]** `settings_view/mod.rs:191` 加 `SettingsSection::Notifications`;:1281 `settings_pages` vec 加页;:1318 nav_items 加 `SettingsNavItem::Page`;:1231 区 `add_typed_action_view` 注册。
3. **[改]** 页内渲染:
   - 总开关 / 各事件开关:`render_notification_toggle`(features_page.rs:3740)。
   - "何时弹" / `focus_behavior` / 内置声音:`Dropdown`+`update_*_dropdown`(:2768,枚举需 `as_dropdown_label()`)。
   - 自定义声音路径 / 节流秒数:`text_input EditorView`(:3706)。
   - 每个控件:`FeaturesPageAction` 风格 action 变体 + dispatch handler(照抄 features_page.rs:1510 clone→改字段→set_value)。
4. **[新/待核实]** 自定义声音路径 `validate`:`NotificationsSettings` 是 struct 型,validate 默认 no-op(lib.rs:348);手写 Setting impl 或在 handler 校验 `File(path)` 存在/可读/扩展名,非法回落 Default。**无原生文件选择器,只能 text_input。**
5. **[改/待定]** cloud sync:`event_sounds` 的 `File` 项 per-machine,考虑标 `Never`/private 或 `current_value_is_syncable()` 过滤(设计 §4.1)。

### 验证

- `cargo check`(注意 `#[cfg(feature="omw_local")]` 门控:omw build 出现、上游 build 不出现)。
- **Smoke**:打开 omw 设置 → 见独立"通知"页 → 切换每个开关/下拉/路径 → 确认写入 settings.toml `[notifications]` 且行为变化。
- `cargo run --bin generate_settings_schema`(app/src/bin):确认新键进 JSON schema。

---

## MS6 — P2:agent turn → 原生通知(共用地基)

**依赖**:MS0-MS4(投递/聚焦/声音/节流地基)。**P2 不碰 cli_agent_sessions 内部,只在扇出层接入。**

### 步骤

1. **[改]** `terminal/view.rs:796` `NotificationsTrigger`:加 `AgentTurnComplete` / `AgentNeedsApproval`。
2. **[改]** `ai/agent_management/agent_management_model.rs:147` `handle_cli_agent_session_event`(或收敛进 :394 `add_notification`):
   - Success → `AgentTurnComplete`(→ `NotificationEvent::TaskCompleted` 声音);Blocked → `AgentNeedsApproval`(→ `NeedsAttention`)。
   - 扇出到 OS 通知渠道:**最终仍走 `pane_group::Event::SendNotification` 带 pane_id / `NotificationContext::BlockOrigin`**(与 P1 同一路径),而非新链。
   - **[待核实]** `terminal_view_id` → `PaneViewLocator{pane_group_id, pane_id}` 的映射。若不便,退回复用 `WorkspaceAction::FocusTerminalViewInWorkspace`(toast_stack.rs:257)**但须确认它经 `RootView::focus_pane`(Quake 特判)**(设计 §3 交叉确认项)。
   - 前台抑制:复用现成 `is_terminal_view_visible`(:410)。
3. **[待核实]** flag 关系:确认 P2 依赖的上游 flag(HOANotifications 等,agent_management_model.rs:88/104)在 omw 下可用;`AgentMode` 在 omw 被禁(lib.rs:2453),确认不阻断 cli_agent_sessions 事件流。
4. **[可选/不做]** Codex "等待批准":Codex OSC9 无类型(listener/mod.rs:100 一律 Stop/Success),要 Blocked 需脆弱文本匹配。**建议本期 Codex 只支持"完成"粒度**,文档说明。

### 验证

- `cargo check`
- **Smoke**:装了 Warp CLI 插件的 claude 会话,跑一个需权限确认的任务 → 到"等待批准"弹"需要你回答/批准"通知(带项目名),点击回该 pane;turn 结束弹"完成"通知。多 pane 并行各点各回。
- **Smoke(未装插件)**:确认 session 停 InProgress 不误弹(设计 R8)。

### TDD 切入点

Success/Blocked → NotificationsTrigger/Event 映射是纯函数,可单测(给定 CLIAgentSessionStatus → 断言 trigger + sound event + 文案类别)。

---

## MS7 — NotificationChannel trait + Linux 抽象留口

**依赖**:MS2(投递已收敛)。可在 P1 尾或 P2 后做。

### 步骤

1. **[新]** app 层 `NotificationChannel` trait:方法 `deliver(UserNotification, locator)` + `respects_dnd()`;默认实现 = 现有 `ctx.send_desktop_notification`。为 phase2 手机/Tailscale 转发预留。**本期只落 trait + desktop 默认实现,不做转发渠道(明确 Out)。**
2. **[改/可选]** Linux(`winit/notifications/linux.rs:18`):透传 data + 按 SoundSpec 设 `.sound_name()`。**点击回路(notify_rust action + `CustomEvent::NotificationClicked` + `platform/app.rs:104` 去 dead_code)工作量非平凡** —— 需求只要"留抽象",**建议本期 Linux 仅弹不可点击,点击回路作后续项**(设计 §5.4)。

### 验证

- `cargo check --target`(Linux 交叉编译或 CI)确认不 break。
- macOS 走 trait 默认实现,行为与 MS2 一致。

---

## 依赖顺序图

```
MS0(解门控, 阻塞全部)
 ├─> MS1(始终弹/前台抑制) ─┐
 ├─> MS2(声音/分类/identifier) ─┼─> MS3(模板) ─> MS4(节流/DND) ─> MS5(设置UI)
 │                              │                                    │
 └──────────────────────────────┴──> MS6(P2 agent turn) <───────────┘
                                 └──> MS7(渠道 trait / Linux 留口)
```

- **P1 完成线**:MS0→MS1→MS2→MS3→MS4→MS5(+MS7 trait)。
- **P2**:MS6(依赖 P1 地基就绪)。
- MS1 与 MS2 文件不重叠,可并行。

## 全局验证门(每个 PR 都过)

1. `cargo check -p <app> -p warp_features -p warpui_core -p warpui`
2. `cargo test`(feature flag 契约 / settings round-trip / 模板 / 节流 / P2 映射)
3. `cargo run --bin generate_settings_schema` 无 diff 冲突
4. **功能关闭回归**:关掉设置总开关 / 关 flag → 行为与现状完全一致(需求第8点)。
5. macOS smoke:发 OSC 9 → 弹 → 点击 → 聚焦正确 pane(多 pane 不误跳)。

## 待核实清单(实现时逐条核对,勿编造)

- [ ] `#[cfg(feature="pluggable_notifications")]`(lib.rs:2988)在 omw build profile 是否启用。
- [ ] `is_pane_in_foreground` 在 TerminalView ViewContext 的取数路径(Workspace 活动 tab / focused_pane_id 可否直接拿)。
- [ ] `UNNotificationSound soundNamed:` 对任意路径音频文件的支持(是否需拷入 `~/Library/Sounds`)。
- [ ] P2 `terminal_view_id` → `PaneViewLocator` 映射是否可得;否则 `FocusTerminalViewInWorkspace` 是否经 `RootView::focus_pane`。
- [ ] P2 依赖上游 flag(HOANotifications / cli_agent_sessions 事件流)在 omw 下可用。
- [ ] `mode=Unset` 默认与"默认开"的最终协调方案(是否动 banner 逻辑)。
