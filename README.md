# Herdr Focus Notify

English | [简体中文](README.zh-CN.md)

`herdr-focus-notify` is a macOS plugin for Herdr. It shows a clickable desktop notification when an agent is `blocked` or `done`. Clicking it brings the matching Herdr pane into focus.

It is designed to notify you only when the change is easy to miss: when Herdr is not frontmost, or when you are looking at a different pane.

## Quick start

### 1. Install the requirements

- macOS
- Herdr `0.7.5` or later
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

### 3. Done — zero configuration

The plugin works with **zero configuration**. The first time you focus a pane in Herdr, the plugin binds the frontmost terminal to that pane's workspace, then uses it to activate the terminal on click and to recognise when you are already looking at a pane. Bindings are per-workspace: switch from kitty to Ghostty and keep working on the same pane, and clicking a notification activates Ghostty.

No configuration files needed. The only external dependency is alerter, auto-detected from `PATH` and common Homebrew locations:

```bash
brew install vjeantet/tap/alerter
```

## How notifications behave

By default, `blocked` and `done` status changes can produce a notification. The plugin sends one only when it cannot confirm that you are already looking at that pane.

| Your current view | Notification |
|---|---|
| Another app is frontmost | Sent |
| Herdr is frontmost, but a different pane is focused | Sent |
| Herdr is frontmost and the matching pane is focused | Skipped |
| A known terminal is frontmost and the pane is focused | Skipped (you are looking at Herdr) |
| The focused app cannot be determined | Sent, to avoid missing a change |

Clicking a notification activates the terminal (configured, or the learned one), then runs `herdr agent focus <pane>`.

Blocked notifications say that the agent needs your input and prompt you to review and respond. Done notifications say that the agent finished and prompt you to review the result. The plugin does not read or summarize pane contents.

When you manually focus the matching pane in Herdr while its terminal is frontmost, its pending notification is removed.

If the pane was already active when the notification arrived, returning to that terminal removes it within a few seconds.

## How it stays quiet

- Notifications only fire for `blocked` and `done` — the two statuses that actually need you.
- A notification is skipped when the pane is already focused **and** the frontmost app is the terminal bound to its workspace (learned automatically).
- If you are elsewhere when the pane becomes active, the notification auto-removes within a few seconds once you switch back to the bound terminal.

The `--test` action sends a real test notification (capped at 10 seconds) so you can verify the whole pipeline.

## Troubleshooting

| Problem | What to check |
|---|---|
| No notification appears | Make sure `alerter` is installed and executable; it is auto-detected from `PATH` and common Homebrew locations (`brew install vjeantet/tap/alerter`). |
| Click does not bring forward the expected terminal | Focus any pane once in Herdr first — the plugin learns your terminal from that moment, per workspace. |
| Notifications appear while you are viewing Herdr | You were not in the workspace's bound terminal at that moment; the plugin errs on the side of notifying rather than missing a state change. |
| Need diagnostic information | Run the plugin with `--test` or `--check-pane-visibility <pane_id>` to exercise the pipeline and focus checks directly. |

## Bundled icons

Recognised agent names use bundled local icons, including Codex, Claude Code, Cursor, Gemini, GitHub Copilot, DeepSeek, Qwen, Kimi, OpenCode, OpenHands, Cline, Windsurf, Devin, omp, pi, and v0.

The icons are vendored from `@lobehub/icons-static-png` under the MIT license, except `omp.png` and `pi.png`, which use the official logos of Oh My Pi and the Pi coding agent. See `assets/icons/NOTICE.md`.
