# Herdr Focus Notify

[English](README.md) | 简体中文

`herdr-focus-notify` 是一个 macOS Herdr 插件。当 agent 进入 `blocked` 或 `done` 状态时，它会发送可点击的桌面通知。点击后会聚焦到对应的 Herdr pane。

它只在状态变化容易被错过时提醒你：Herdr 不在前台，或你正在查看另一个 pane。

## 快速开始

### 1. 安装前提条件

- macOS
- Herdr `0.7.3` 或更高版本
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

### 3. 配置终端 App

找到插件配置目录：

```bash
herdr plugin config-dir herdr-focus-notify
```

在该目录创建 `.env` 文件。建议先只添加这一项：

```env
HERDR_FOCUS_NOTIFY_ACTIVATE_APP=kitty
```

可以填写终端 App 名称，例如 `kitty`；也可以填写绝对 `.app` 路径，例如 `/Applications/kitty.app`。

这项配置让插件能在点击通知时激活终端，并可靠判断你是否已经看过对应 pane。通知程序通常会自动找到；只有自动查找失败时，才需要配置 `HERDR_FOCUS_NOTIFY_NOTIFIER`。

## 通知规则

默认情况下，`blocked` 和 `done` 状态变化会触发通知。正确配置 `ACTIVATE_APP` 后，只有在插件无法确认你正在查看对应 pane 时，才会真正发出通知。

| 当前状态 | 是否通知 |
|---|---|
| 其它 App 在前台 | 发送 |
| Herdr 在前台，但焦点位于另一个 pane | 发送 |
| Herdr 在前台，且焦点就是对应 pane | 跳过 |
| 无法确定前台 App | 发送，避免遗漏状态变化 |

点击通知后，插件会激活配置的终端 App，然后执行 `herdr agent focus <pane>`。未配置 `ACTIVATE_APP` 时，聚焦仍能工作，但插件无法可靠判断你是否已查看 pane，因此可能会多发通知。

配置的终端 App 在前台时，你在 Herdr 中手动聚焦对应 pane 后，待处理通知会被移除。如果通知到达时 pane 已经是 active，切回该终端 App 后，通知会在数秒内移除。

## 可选设置

插件一共支持六项设置，但通常只需要配置 `HERDR_FOCUS_NOTIFY_ACTIVATE_APP`，其它项都有可用的默认值。

- `HERDR_FOCUS_NOTIFY_STATUSES`：触发通知的状态，以逗号分隔；默认 `blocked,done`。
- `HERDR_FOCUS_NOTIFY_TIMEOUT`：自动关闭秒数；默认 `3600`，设为 `0` 则保持显示。
- `HERDR_FOCUS_NOTIFY_ENABLED=0`：暂停通知，但不移除插件。
- `HERDR_FOCUS_NOTIFY_NOTIFIER`：自动查找失败时，填写 `alerter` 的完整路径。
- `HERDR_FOCUS_NOTIFY_DEBUG=1`：在插件日志和 `focus-click.log` 中输出诊断信息。

`.env` 支持 `KEY=value`、可选的 `export KEY=value`、带引号的值和行尾注释。`ACTIVATE_APP` 中的路径会直接传给 `open`，请使用绝对路径，不要使用 `~`。

## 排查问题

| 问题 | 检查方式 |
|---|---|
| 没有收到通知 | 确认已安装且可执行 `alerter`。必要时把其完整路径写入 `HERDR_FOCUS_NOTIFY_NOTIFIER`。 |
| 点击后没有激活预期终端 | 将 `HERDR_FOCUS_NOTIFY_ACTIVATE_APP` 设为 App 名称或绝对 `.app` 路径。 |
| 正在看 Herdr 时仍收到通知 | 配置 `ACTIVATE_APP`；未配置时，插件会优先保证不错过状态变化。 |
| 需要诊断信息 | 临时设置 `HERDR_FOCUS_NOTIFY_DEBUG=1`，然后检查插件日志和 `focus-click.log`。 |
| 想暂停通知 | 设置 `HERDR_FOCUS_NOTIFY_ENABLED=0`。 |

## 内置图标

已识别的 agent 名称会使用内置本地图标，包括 Codex、Claude Code、Cursor、Gemini、GitHub Copilot、DeepSeek、Qwen、OpenCode、OpenHands、Cline、Windsurf、Devin 和 v0。

图标来自 `@lobehub/icons-static-png`，以 MIT 许可证提供。详见 `assets/icons/NOTICE.md`。
