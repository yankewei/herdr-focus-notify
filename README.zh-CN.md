# Herdr Focus Notify

[English](README.md) | 简体中文

`herdr-focus-notify` 是一个 macOS Herdr 插件。当 agent 进入 `blocked` 或 `done` 状态时，它会发送可点击的桌面通知。点击后会聚焦到对应的 Herdr pane。

它只在状态变化容易被错过时提醒你：Herdr 不在前台，或你正在查看另一个 pane。

## Herdr 版本兼容性

**Herdr 0.9.0 请使用插件 tag `v0.5.0` 或更新版本**。`v0.4.0` 尚未适配客户端焦点变化，点击通知可能只激活终端而不切换到目标 pane。后续版本会把目标 pane 显式投射到已连接的客户端视图。

最低支持的 Herdr 版本仍为 `0.7.5`。workspace 与终端的绑定行为保持不变。

## 快速开始

### 1. 安装前提条件

- macOS
- Herdr `0.7.5` 或更高版本
- [alerter](https://github.com/vjeantet/alerter)：用于显示可点击通知

安装 alerter：

```bash
brew install vjeantet/tap/alerter
```

### 2. 安装插件

从 GitHub 安装：

```bash
herdr plugin install yankewei/herdr-focus-notify
```

或者在本地构建并链接：

```bash
cargo build --release
herdr plugin link .
```

### 3. 完成——零配置

插件**零配置开箱即用**。当你第一次在 Herdr 中聚焦 pane 时,插件会把当时最前面的终端绑定到该 pane 所在的 workspace,之后用它来在点击通知时激活终端、判断你是否正在查看对应 pane。每个 workspace 独立绑定——你在 kitty 里用完,换到 Ghostty 继续同一个 pane,点击通知就会激活 Ghostty。点击通知时，即使浏览器或其它 App 在前台，也不会改变已有绑定。

你不需要创建任何配置文件。唯一的外部依赖是通知程序 `alerter`,插件会从 `PATH` 和常见 Homebrew 路径自动查找:

```bash
brew install vjeantet/tap/alerter
```

## 通知规则

默认情况下，`blocked` 和 `done` 状态变化会触发通知。只有在插件无法确认你正在查看对应 pane 时，才会真正发出通知。

| 当前状态 | 是否通知 |
|---|---|
| 其它 App 在前台 | 发送 |
| Herdr 在前台，但焦点位于另一个 pane | 发送 |
| Herdr 在前台，且焦点就是对应 pane | 跳过 |
| 该 workspace 绑定的终端在前台，且焦点就是对应 pane | 跳过（你正在看 Herdr） |
| 无法确定前台 App | 发送，避免遗漏状态变化 |

点击通知后,如果该 workspace 已有终端绑定,插件会激活该终端,然后通过 Herdr socket 发送目标 pane 的 `pane.focus` 请求。该请求会一次性显示对应的 workspace、tab 和 pane，也支持没有检测到 agent 的普通 shell pane。多个客户端连接同一服务端时,会一起切换到该 pane。如果 workspace 没有绑定,点击不会激活 App,也不会发送 Herdr focus 请求；请先在预期终端中手动聚焦一次 pane。

### 多个终端 window 和 tab

终端开了多个 window 或 tab 时，只激活终端 App 可能会把没有运行 Herdr 的那个窗口带到前面。在受支持的终端中，点击会先选中该 session 的 Herdr 客户端所在的 window、tab 或分屏，再连同它所在的 OS window 一起带到前面。连接了多个客户端时，优先选择最近使用的那个。

| 终端 | 配置 |
|---|---|
| iTerm2 | 无需配置。插件会把客户端的 `ITERM_SESSION_ID` 交给 iTerm2 内置的 reveal URL。 |
| kitty | 需要开启远程控制，见下方配置。 |

```conf
# kitty.conf（修改后需重启 kitty）
allow_remote_control socket-only
listen_on unix:/tmp/kitty
```

其他终端，或未开启上述设置的 kitty，点击只会激活终端 App，具体带到前面的是哪个窗口由 macOS 决定。

通知的排版与 Agent 侧边栏的行一致：标题是 `{状态} · {workspace} · {tab}`，副标题是 agent 名称加上 pane 的 git 状态，正文是 pane 的终端标题。git 状态采用 shell 提示符和 diffstat 通用的简写：`main* · +120/-45` 表示分支 `main`、有未提交的改动、相对 `HEAD` 新增 120 行、删除 45 行（未跟踪的文件不计入）。不涉及文本行的改动（例如二进制文件或可执行位）只显示 `main*`。分支名过长、会把行数挤出副标题那一行时，会从中间截断。Herdr 没有报告标题时，正文回退为状态提示：`blocked` 引导你查看和回复，`done` 引导你查看结果。插件不会读取或总结 pane 内容，也不会要求 Herdr 解释它的检测结果。

终端 App 在前台时，你在 Herdr 中手动聚焦对应 pane 后，待处理通知会被移除。如果通知到达时 pane 已经是 active，切回该终端 App 后，通知会在数秒内移除。

## 如何保持安静

- 只在 `blocked` 和 `done` 两种状态发通知——只有它们需要你参与。
- 如果 pane 已聚焦且最前面的 App 是该 workspace 绑定的终端,通知会被跳过。
- 如果通知到达时你不在该终端,切回绑定终端后通知会在几秒内自动移除。

`--test` 会发送一条真实测试通知(超时上限 10 秒),可以完整验证整条链路。

## 排查问题

| 问题 | 检查方式 |
|---|---|
| 没有收到通知 | 确认 `alerter` 已安装且可执行;插件从 `PATH` 和常见 Homebrew 路径自动查找(`brew install vjeantet/tap/alerter`)。 |
| 激活了正确的终端，但不是运行 Herdr 的那个 window 或 tab | 只有 iTerm2 和开启远程控制的 kitty 支持选中 window 和 tab（见[多个终端 window 和 tab](#多个终端-window-和-tab)），其他终端只会激活 App。 |
| 点击后没有激活预期终端 | 使用插件的“清除已保存终端绑定” action,然后在预期终端中手动聚焦一次 pane。 |
| workspace 保存了错误的终端绑定 | 使用插件的“清除已保存终端绑定” action,然后在预期终端中手动聚焦一次 pane。 |
| 正在看 Herdr 时仍收到通知 | 说明那一刻你不在该 workspace 绑定的终端里;插件优先保证不错过状态变化。 |
| 需要诊断信息 | 运行 `--test` 或 `--check-pane-visibility <pane_id>` 直接验证通知链路和聚焦判断。 |

## 内置图标

已识别的 agent 名称会使用内置本地图标，包括 Codex、Claude Code、Cursor、Gemini、GitHub Copilot、DeepSeek、Qwen、Kimi、OpenCode、OpenHands、Cline、Windsurf、Devin、omp、pi 和 v0。

图标来自 `@lobehub/icons-static-png`，以 MIT 许可证提供，其中 `omp.png` 和 `pi.png` 使用 Oh My Pi 与 Pi coding agent 的官方 logo。详见 `assets/icons/NOTICE.md`。
