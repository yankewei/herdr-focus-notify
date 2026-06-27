# Herdr Focus Notify

Clickable macOS notifications that focus the matching Herdr agent pane.

Herdr already has native notifications. This plugin is for the missing click target: when a Herdr agent becomes `blocked` or `done`, it sends a macOS notification whose click action runs:

```bash
herdr agent focus <pane_id>
```

## Recommended Setup

To avoid duplicate notifications, turn off Herdr's native toast delivery and let this plugin handle the clickable desktop notification.

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

After it is enabled, the plugin runs without plugin-specific configuration. Herdr provides the plugin context and Herdr binary path; the plugin also checks common macOS locations for the notifier.

The plugin requires Herdr `0.7.0` or newer.

## Test

Run the test action:

```bash
herdr plugin action invoke test --plugin local.herdr-focus-notify
```

## Troubleshooting

If notifications do not appear, install the macOS notifier:

```bash
brew install terminal-notifier
```
