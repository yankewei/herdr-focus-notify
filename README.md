# Herdr Focus Notify

English | [简体中文](README.zh-CN.md)

Send clickable macOS notifications when a Herdr agent needs attention (`blocked`) or finishes (`done`). Clicking a notification focuses the matching Herdr pane.

Common agent notifications use bundled local icons, including Codex, Claude Code, Claude, Cursor, Gemini CLI, Gemini, GitHub Copilot, DeepSeek, Grok, Qwen, OpenCode, OpenHands, Roo Code, Cline, Windsurf, Devin, Manus, Kiro, Trae, Zencoder, Lovable, and v0.

Notifications are sent only when you are **not already looking at that pane**:

- Herdr is not the frontmost app.
- Herdr is frontmost, but another pane is focused.

When you later focus that pane directly in Herdr, its pending notification is removed. The plugin only clears it after confirming that your configured terminal app is frontmost, so a background script or API call that changes Herdr's focus cannot hide a notification you have not seen.

## Requirements

- macOS
- Herdr `0.7.3` or later
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

## CLI

```bash
herdr-focus-notify --help
herdr-focus-notify --version
herdr-focus-notify --test
```

`--help` and `--version` print to stdout. `--test` sends a foreground test notification. Configuration or notification backend failures are printed to stderr and return a non-zero exit code. Normal plugin invocations without `HERDR_PLUGIN_EVENT_JSON` still exit quietly with `0`.

## Configuration

Find the plugin config directory:

```bash
herdr plugin config-dir herdr-focus-notify
```

Create a `.env` file in that directory.

The `.env` parser supports `KEY=value`, optional `export KEY=value`, single-quoted values, double-quoted values, and inline comments after unquoted values.

### Recommended

```env
HERDR_FOCUS_NOTIFY_NOTIFIER=/opt/homebrew/bin/alerter
HERDR_FOCUS_NOTIFY_ACTIVATE_APP=kitty
```

`ACTIVATE_APP` can be an app name, such as `kitty`, or a `.app` path, such as `/Applications/kitty.app`. This is easier to configure than a bundle id.

Configuring `ACTIVATE_APP` is recommended. It is used to bring your terminal app to the front when you click a notification, to decide whether you are already looking at the current Herdr pane, and to clear a notification once you manually focus its pane. The plugin skips or clears a notification only when it can confirm that the frontmost app is the app resolved from `ACTIVATE_APP`. If macOS frontmost-app detection fails, or the app name cannot be resolved, the plugin keeps or sends the notification to avoid missing an important state change.

### Common Options

| Variable | Description | Default |
|---|---|---|
| `HERDR_FOCUS_NOTIFY_NOTIFIER` | Notification backend path. The plugin reports an error if no executable notifier is found. | Auto-detect `alerter` |
| `HERDR_FOCUS_NOTIFY_ACTIVATE_APP` | Terminal app name or `.app` path to activate when a notification is clicked. | None |
| `HERDR_FOCUS_NOTIFY_TIMEOUT` | Seconds before a notification auto-dismisses. Set to `0` to keep it until dismissed. | `3600` |

If `ACTIVATE_APP` is not configured, clicking a notification still runs `herdr agent focus <pane>`, but the plugin cannot reliably tell whether the frontmost app is the terminal that hosts Herdr, so it may send extra notifications.

For troubleshooting, temporarily set `HERDR_FOCUS_NOTIFY_DEBUG=1`. To pause the plugin without unlinking it, set `HERDR_FOCUS_NOTIFY_ENABLED=0`.

Bundled agent icons are vendored from `@lobehub/icons-static-png` under the MIT license. See `assets/icons/NOTICE.md`.
