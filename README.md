# Herdr Focus Notify

English | [简体中文](README.zh-CN.md)

`herdr-focus-notify` is a macOS plugin for Herdr. It shows a clickable desktop notification when an agent is `blocked` or `done`. Clicking it brings the matching Herdr pane into focus.

It is designed to notify you only when the change is easy to miss: when Herdr is not frontmost, or when you are looking at a different pane.

## Quick start

### 1. Install the requirements

- macOS
- Herdr `0.7.3` or later
- [alerter](https://github.com/vjeantet/alerter), which displays the clickable notification

Install alerter:

```bash
brew install vjeantet/tap/alerter
```

### 2. Install the plugin

Install from GitHub:

```bash
herdr plugin install yankewei/herdr-focus-notify
```

Or build and link the local checkout:

```bash
cargo build --release
herdr plugin link .
```

### 3. Configure your terminal app

Find the plugin configuration directory:

```bash
herdr plugin config-dir herdr-focus-notify
```

Create a `.env` file there. This is the recommended minimal setup:

```env
HERDR_FOCUS_NOTIFY_ACTIVATE_APP=kitty
```

Use your terminal app's name, such as `kitty`, or an absolute `.app` path, such as `/Applications/kitty.app`.

This lets the plugin activate the terminal when you click a notification and reliably recognise when you have seen the relevant pane.

The notifier path is usually found automatically. Set `HERDR_FOCUS_NOTIFY_NOTIFIER` only when auto-detection fails.

## How notifications behave

By default, `blocked` and `done` status changes can produce a notification. With a valid `ACTIVATE_APP` configuration, the plugin sends one only when it cannot confirm that you are already looking at that pane.

| Your current view | Notification |
|---|---|
| Another app is frontmost | Sent |
| Herdr is frontmost, but a different pane is focused | Sent |
| Herdr is frontmost and the matching pane is focused | Skipped |
| The focused app cannot be determined | Sent, to avoid missing a change |

Clicking a notification activates the configured terminal app, then runs `herdr agent focus <pane>`.

Without `ACTIVATE_APP`, focusing still works, but the plugin cannot reliably detect that you have already seen a pane. It may therefore send extra notifications.

When you manually focus the matching pane in Herdr while the configured terminal is frontmost, its pending notification is removed.

If the pane was already active when the notification arrived, returning to that terminal removes it within a few seconds.

## Optional settings

There are six supported settings, but you normally need only `HERDR_FOCUS_NOTIFY_ACTIVATE_APP`. Everything else has a working default.

- `HERDR_FOCUS_NOTIFY_STATUSES`: comma-separated notification statuses; default `blocked,done`.
- `HERDR_FOCUS_NOTIFY_TIMEOUT`: auto-dismiss time in seconds; default `3600`, or `0` to keep notifications open.
- `HERDR_FOCUS_NOTIFY_ENABLED=0`: pause notifications without removing the plugin.
- `HERDR_FOCUS_NOTIFY_NOTIFIER`: full `alerter` path when auto-detection fails.
- `HERDR_FOCUS_NOTIFY_DEBUG=1`: enable diagnostics in the plugin logs and `focus-click.log`.

The `.env` file supports `KEY=value`, optional `export KEY=value`, quoted values, and inline comments. Paths in `ACTIVATE_APP` are passed directly to `open`; use an absolute path rather than `~`.

## Troubleshooting

| Problem | What to check |
|---|---|
| No notification appears | Confirm `alerter` is installed and executable. Set `HERDR_FOCUS_NOTIFY_NOTIFIER` to its full path if needed. |
| Click does not bring forward the expected terminal | Set `HERDR_FOCUS_NOTIFY_ACTIVATE_APP` to the app name or its absolute `.app` path. |
| Notifications appear while you are viewing Herdr | Configure `ACTIVATE_APP`; without it the plugin deliberately errs on the side of notifying you. |
| Need diagnostic information | Temporarily set `HERDR_FOCUS_NOTIFY_DEBUG=1`, then inspect the plugin logs and `focus-click.log`. |
| Need to pause notifications | Set `HERDR_FOCUS_NOTIFY_ENABLED=0`. |

## Bundled icons

Recognised agent names use bundled local icons, including Codex, Claude Code, Cursor, Gemini, GitHub Copilot, DeepSeek, Qwen, OpenCode, OpenHands, Cline, Windsurf, Devin, and v0.

The icons are vendored from `@lobehub/icons-static-png` under the MIT license. See `assets/icons/NOTICE.md`.
