# Herdr Focus Notify

Clickable macOS notifications that focus the matching Herdr agent pane.

Herdr already has native notifications. This plugin is for the missing click target: when a Herdr agent becomes `blocked` or `done`, it sends a macOS notification whose click action runs:

```bash
herdr agent focus <pane_id>
```

## Recommended Setup

To avoid duplicate notifications, turn off Herdr's native toast delivery and let this plugin handle the clickable desktop notification.

## Notification backend

Click-to-focus requires [alerter](https://github.com/vjeantet/alerter), an actively maintained fork of terminal-notifier with reliable click actions. The legacy `terminal-notifier` (`-execute`) silently dropped click callbacks on macOS 12+, which is why this plugin defaults to alerter.

Install alerter:

```bash
brew install vjeantet/tap/alerter
```

If alerter is missing the plugin falls back to terminal-notifier for display-only notifications (clicks won't focus).

Add this to `~/.config/herdr/config.toml`:

```toml
[ui.toast]
delivery = "off"
```

## Installation

Install and enable the plugin:

```bash
cargo build --release
herdr plugin link .
```

After it is enabled, the plugin runs without plugin-specific configuration. Herdr provides the plugin context and Herdr binary path; the plugin also checks common macOS locations for the notifier binary.

The plugin prefers alerter for reliable click-to-focus and falls back to terminal-notifier (display-only). Set `HERDR_FOCUS_NOTIFY_NOTIFIER` to force a specific binary.

The plugin requires Herdr `0.7.0` or newer.

## Test

Run the test action:

```bash
herdr plugin action invoke test --plugin herdr-focus-notify
```

## Troubleshooting

If notifications do not appear, install alerter:

```bash
brew install vjeantet/tap/alerter
```

If notifications appear but clicking does nothing, you are on the legacy terminal-notifier backend (its `-execute` callback is broken on macOS 12+). Install alerter as above, or set `HERDR_FOCUS_NOTIFY_NOTIFIER=/opt/homebrew/bin/alerter`.

Set `HERDR_FOCUS_NOTIFY_DEBUG=1` to log the click result and focus command exit status into `focus-click.log` inside the plugin state dir.
