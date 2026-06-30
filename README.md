# Herdr Focus Notify

English | [简体中文](README.zh-CN.md)

Send clickable macOS notifications when a Herdr agent needs attention (`blocked`) or finishes (`done`). Clicking a notification focuses the matching Herdr pane.

Notifications are sent only when you are **not already looking at that pane**:

- Herdr is not the frontmost app.
- Herdr is frontmost, but another pane is focused.

## Requirements

- macOS
- Herdr `0.7.0` or later
- [alerter](https://github.com/vjeantet/alerter), used for clickable notifications

Install alerter:

```bash
brew install vjeantet/tap/alerter
```

## Installation

Build and link locally:

```bash
cargo build --release
herdr plugin link .
```

Or install from GitHub:

```bash
herdr plugin install yankewei/herdr-focus-notify
```

## Configuration

Find the plugin config directory:

```bash
herdr plugin config-dir herdr-focus-notify
```

Create a `.env` file in that directory.

### Recommended

```env
HERDR_FOCUS_NOTIFY_NOTIFIER=/opt/homebrew/bin/alerter
HERDR_FOCUS_NOTIFY_ACTIVATE_APP=kitty
```

`ACTIVATE_APP` can be an app name, such as `kitty`, or a `.app` path, such as `/Applications/kitty.app`. This is easier to configure than a bundle id.

Configuring `ACTIVATE_APP` is recommended. It is used to bring your terminal app to the front when you click a notification, and to decide whether you are already looking at the current Herdr pane. The plugin skips a notification only when it can confirm both conditions: the current focused pane is the target pane, and the frontmost app is the app resolved from `ACTIVATE_APP`. If macOS frontmost-app detection fails, or the app name cannot be resolved, the plugin sends the notification to avoid missing an important state change.

### Common Options

| Variable | Description | Default |
|---|---|---|
| `HERDR_FOCUS_NOTIFY_NOTIFIER` | Notification backend path. The plugin reports an error if no executable notifier is found. | Auto-detect `alerter` |
| `HERDR_FOCUS_NOTIFY_ACTIVATE_APP` | Terminal app name or `.app` path to activate when a notification is clicked. | None |
| `HERDR_FOCUS_NOTIFY_TIMEOUT` | Seconds before a notification auto-dismisses. Set to `0` to keep it until dismissed. | `3600` |

If `ACTIVATE_APP` is not configured, clicking a notification still runs `herdr agent focus <pane>`, but the plugin cannot reliably tell whether the frontmost app is the terminal that hosts Herdr, so it may send extra notifications.

For troubleshooting, temporarily set `HERDR_FOCUS_NOTIFY_DEBUG=1`. To pause the plugin without unlinking it, set `HERDR_FOCUS_NOTIFY_ENABLED=0`.
