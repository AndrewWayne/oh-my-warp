# oh my warp 0.0.15（Mac Pre-release）

本次将已验收的 `default-links-r2` 修订发布为 0.0.15，保留 0.0.14 的全部自定义功能，重点完善 Codex 相对文件路径跳转。

## 更新内容

- 支持默认启动的 Codex 共享后台服务，通过当前终端的会话身份确认工作目录。
- 支持已验收的普通启动、`-C` / `--cd`、恢复会话和 worktree 场景。
- 修复窄窗口中文路径因光标跳过空白而识别不完整的问题，完整保留 `.md` 后缀和行号。
- 同一终端切换 Codex 会话时及时清除旧链接，避免打开上一工作区的同名文件。
- 新增 `omw codex-links`，备份配置并启用所需的 Codex 原生会话标题，保留其他配置。

通知、模型连接检测、无密钥 Ollama、手机共享、日志脱敏、窗口操作、标签页和分屏等既有功能继续保留。本次没有整体同步上游，也没有改变 Codex 提示词或启动参数。

## 下载与设置

本版提供 **Mac Apple Silicon** 安装包：`omw-warp-oss-v0.0.15-aarch64-apple-darwin.dmg`，以及校验文件、源码和构建清单。Windows 继续使用 [0.0.14 Pre-release](https://github.com/AndrewWayne/oh-my-warp/releases/tag/omw-local-preview-v0.0.14) 的现有安装包；本版没有 Windows 更新。

源码归档对应发布标签，保留 `.gitmodules`；子模块固定提交列在构建清单中，归档不包含子模块内容。

打开 DMG，将 `omw-warp-oss.app` 拖入 Applications。请先保存工作并退出旧应用。安装不会自动改写原有配置。

默认启动的 Codex 相对路径识别，需要 Codex 共享后台服务 **0.162 或更新版本**，并启用原生会话标题。在 oh my warp 的普通终端运行：

```sh
omw codex-links --dry-run
omw codex-links
```

随后重新启动对应 Codex 客户端。如使用独立配置目录，先设置与该 Codex 一致的 `CODEX_HOME` 再运行命令。工具会保存配置备份，并保留其他设置和已有标题项目。oh my warp 会隐藏新增的会话身份前缀；其他终端可能显示这段身份信息。

Mac 包采用 ad-hoc 签名，尚未经过 Apple 公证。更换构建后，macOS 可能重新询问“桌面”等目录访问权限；项目位于这些目录时，请自行确认系统提示。未确认时可能出现终端初始化等待。若下载隔离属性阻止启动，可使用：

```sh
xattr -d com.apple.quarantine /Applications/omw-warp-oss.app
```

## 验证与已知边界

r2 已完成默认启动、恢复会话、worktree、窄窗口中文路径和实际 Command 点击验收，并验证同一终端切换会话时旧悬停链接失效。发布包另行检查版本、签名、校验值、源码对应关系、路径回放、通知和随包 Agent。

无法唯一确认会话目录时会跳过相对路径；现有八行、8192 字节的扫描边界保持不变。较旧 Codex 版本及未来版本兼容性、TUI 内任意目录变化未穷尽验证。

最低系统标识仍为 macOS 11.0，实际 GUI 验收在 macOS 27 完成，较旧系统运行尚未验证。输入编辑、真实模型服务凭据、实体手机/Tailscale 等保留此前验收边界。Mac Intel 和 Linux 安装包暂不提供。

这是公开可下载的 **Pre-release（试用版）**。旧版本下载继续保留。
