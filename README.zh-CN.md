# Herdr Focus Notify

[English](README.md) | 简体中文

当 Herdr agent 需要关注（`blocked`）或完成（`done`）时，发送可点击的 macOS 通知。点击通知会聚焦到对应的 Herdr pane。

通知只会在你**没有在看这个 pane** 时发送：

- 你当前在其它 App（Herdr 不在前台）
- 你在 Herdr 里，但聚焦的是另一个 pane

## 前提条件

- macOS
- Herdr `0.7.0` 或更新版本
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

## 配置

找到插件配置目录：

```bash
herdr plugin config-dir herdr-focus-notify
```

在该目录下创建 `.env` 文件。

### 推荐配置

```env
HERDR_FOCUS_NOTIFY_NOTIFIER=/opt/homebrew/bin/alerter
HERDR_FOCUS_NOTIFY_ACTIVATE_APP=kitty
```

`ACTIVATE_APP` 填 app 名称（如 `kitty`）、`.app` 路径（如 `/Applications/kitty.app`）都可以，比 bundle id 更容易找到。

建议配置 `ACTIVATE_APP`。它用于点击通知时把终端 App 提到前台，也用于判断你是否正在看当前 Herdr pane。只有在插件能确认「当前 focused pane 是这个 pane」并且「前台 App 是 `ACTIVATE_APP` 对应的 App」时，才会跳过通知；如果 macOS 前台 App 查询失败或 App 名称无法解析，插件会选择发送通知，避免漏掉需要关注的状态。

### 常用配置

| 变量 | 说明 | 默认值 |
|---|---|---|
| `HERDR_FOCUS_NOTIFY_NOTIFIER` | 通知后端路径；找不到可执行通知后端时会报错 | 自动查找 `alerter` |
| `HERDR_FOCUS_NOTIFY_ACTIVATE_APP` | 点击通知时激活的终端 app 名或 `.app` 路径 | 无 |
| `HERDR_FOCUS_NOTIFY_TIMEOUT` | 通知自动消失时间（秒），`0` 表示不自动消失 | `3600` |

如果没有配置 `ACTIVATE_APP`，通知点击后仍会执行 `herdr agent focus <pane>`，但插件无法可靠判断前台 App 是否就是 Herdr 所在终端，因此可能会多发通知。

排障时可以临时配置 `HERDR_FOCUS_NOTIFY_DEBUG=1`；需要暂停插件时可以配置 `HERDR_FOCUS_NOTIFY_ENABLED=0`。
