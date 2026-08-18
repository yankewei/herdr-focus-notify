# Herdr Focus Notify

[English](README.md) | 简体中文

`herdr-focus-notify` 是一个 macOS Herdr 插件。当 agent 进入 `blocked` 或 `done` 状态时，它会发送可点击的桌面通知。点击后会聚焦到对应的 Herdr pane。

它只在状态变化容易被错过时提醒你：Herdr 不在前台，或你正在查看另一个 pane。

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

插件**零配置开箱即用**。当你第一次在 Herdr 中聚焦 pane 时,插件会把当时最前面的终端绑定到该 pane 所在的 workspace,之后用它来在点击通知时激活终端、判断你是否正在查看对应 pane。每个 workspace 独立绑定——你在 kitty 里用完,换到 Ghostty 继续同一个 pane,点击通知就会激活 Ghostty。

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
| 已知终端在前台，且焦点就是对应 pane | 跳过（你正在看 Herdr） |
| 无法确定前台 App | 发送，避免遗漏状态变化 |

点击通知后,插件会激活该 pane 所在 workspace 绑定的终端,然后执行 `herdr agent focus <pane>`。

`blocked` 通知会提示 Agent 需要你的输入，并引导你查看和回复；`done` 通知会提示 Agent 已完成，并引导你查看结果。插件不会读取或总结 pane 内容。

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
| 点击后没有激活预期终端 | 先在 Herdr 中手动聚焦一次该 pane——插件从那一刻起按 workspace 学习终端。 |
| 正在看 Herdr 时仍收到通知 | 说明那一刻你不在该 workspace 绑定的终端里;插件优先保证不错过状态变化。 |
| 需要诊断信息 | 运行 `--test` 或 `--check-pane-visibility <pane_id>` 直接验证通知链路和聚焦判断。 |

## 内置图标

已识别的 agent 名称会使用内置本地图标，包括 Codex、Claude Code、Cursor、Gemini、GitHub Copilot、DeepSeek、Qwen、Kimi、OpenCode、OpenHands、Cline、Windsurf、Devin、omp、pi 和 v0。

图标来自 `@lobehub/icons-static-png`，以 MIT 许可证提供，其中 `omp.png` 和 `pi.png` 使用 Oh My Pi 与 Pi coding agent 的官方 logo。详见 `assets/icons/NOTICE.md`。
