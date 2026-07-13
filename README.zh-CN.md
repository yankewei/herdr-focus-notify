# Herdr Focus Notify

[English](README.md) | 简体中文

当 Herdr agent 需要关注（`blocked`）或完成（`done`）时，发送可点击的 macOS 通知。点击通知会聚焦到对应的 Herdr pane。

常见 agent 会自动使用插件内置的本地图标，包括 Codex、Claude Code、Claude、Cursor、Gemini CLI、Gemini、GitHub Copilot、DeepSeek、Grok、Qwen、OpenCode、OpenHands、Roo Code、Cline、Windsurf、Devin、Manus、Kiro、Trae、Zencoder、Lovable、v0。

通知只会在你**没有在看这个 pane** 时发送：

- 你当前在其它 App（Herdr 不在前台）
- 你在 Herdr 里，但聚焦的是另一个 pane

之后如果你直接在 Herdr 中聚焦该 pane，对应的待处理通知会被移除。如果该 pane 原本就是 active，切回配置的终端 App 后，对应通知也会在数秒内移除。插件只会在确认配置的终端 App 位于前台后才移除，因此后台脚本或 API 改变 Herdr 焦点时，不会隐藏你尚未看到的通知。

## 前提条件

- macOS
- Herdr `0.7.3` 或更新版本
- [alerter](https://github.com/vjeantet/alerter)：用于显示可点击通知

安装 alerter：

```bash
brew install vjeantet/tap/alerter
```

## 安装

本地构建并链接：

```bash
cargo build --release
herdr plugin link .
```

或从 GitHub 安装：

```bash
herdr plugin install yankewei/herdr-focus-notify
```

## CLI

```bash
herdr-focus-notify --help
herdr-focus-notify --version
herdr-focus-notify --test
```

`--help` 和 `--version` 会输出到 stdout。`--test` 会发送一条前台测试通知。配置错误或通知后端错误会输出到 stderr，并返回非零退出码。普通插件调用如果没有 `HERDR_PLUGIN_EVENT_JSON`，仍会安静地以 `0` 退出。

## 配置

找到插件配置目录：

```bash
herdr plugin config-dir herdr-focus-notify
```

在该目录下创建 `.env` 文件。

`.env` 解析支持 `KEY=value`、可选的 `export KEY=value`、单引号值、双引号值，以及未加引号值后面的行尾注释。

### 推荐配置

```env
HERDR_FOCUS_NOTIFY_NOTIFIER=/opt/homebrew/bin/alerter
HERDR_FOCUS_NOTIFY_ACTIVATE_APP=kitty
```

`ACTIVATE_APP` 填 app 名称（如 `kitty`）、`.app` 路径（如 `/Applications/kitty.app`）都可以，比 bundle id 更容易找到。

建议配置 `ACTIVATE_APP`。它用于点击通知时把终端 App 提到前台、判断你是否正在看当前 Herdr pane，以及在你手动聚焦 pane 或切回已 active 的 pane 后移除对应通知。只有在插件能确认前台 App 是 `ACTIVATE_APP` 对应的 App 时，才会跳过或移除通知；如果 macOS 前台 App 查询失败或 App 名称无法解析，插件会保留或发送通知，避免漏掉需要关注的状态。

### 常用配置

| 变量 | 说明 | 默认值 |
|---|---|---|
| `HERDR_FOCUS_NOTIFY_NOTIFIER` | 通知后端路径；找不到可执行通知后端时会报错 | 自动查找 `alerter` |
| `HERDR_FOCUS_NOTIFY_ACTIVATE_APP` | 点击通知时激活的终端 app 名或 `.app` 路径 | 无 |
| `HERDR_FOCUS_NOTIFY_TIMEOUT` | 通知自动消失时间（秒），`0` 表示不自动消失 | `3600` |

如果没有配置 `ACTIVATE_APP`，通知点击后仍会执行 `herdr agent focus <pane>`，但插件无法可靠判断前台 App 是否就是 Herdr 所在终端，因此可能会多发通知。

排障时可以临时配置 `HERDR_FOCUS_NOTIFY_DEBUG=1`；需要暂停插件时可以配置 `HERDR_FOCUS_NOTIFY_ENABLED=0`。

内置 agent 图标来自 `@lobehub/icons-static-png`，许可证为 MIT。见 `assets/icons/NOTICE.md`。
