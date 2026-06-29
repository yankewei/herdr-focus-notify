# Herdr Focus Notify

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

### 配置项

| 变量 | 说明 | 默认值 |
|---|---|---|
| `HERDR_FOCUS_NOTIFY_ENABLED` | 是否启用插件 | `1` |
| `HERDR_FOCUS_NOTIFY_STATUSES` | 触发通知的状态，逗号分隔 | `blocked,done` |
| `HERDR_FOCUS_NOTIFY_NOTIFIER` | 通知后端路径 | 自动查找 `alerter` |
| `HERDR_FOCUS_NOTIFY_ACTIVATE_APP` | 点击通知时激活的终端 app 名或 `.app` 路径 | 无 |
| `HERDR_FOCUS_NOTIFY_TIMEOUT` | 通知自动消失时间（秒），`0` 表示不自动消失 | `3600` |
| `HERDR_FOCUS_NOTIFY_DEBUG` | 是否开启调试日志 | `0` |

`ACTIVATE_APP` 用于点击通知时把终端 App 提到前台，同时也用来判断你是否离开了 Herdr。
