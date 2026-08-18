# Changelog

All notable changes to `herdr-focus-notify` are documented here.

## [Unreleased]

### Added

- Bind the terminal to each Herdr workspace from `pane.focused` events, replacing the bundled terminal whitelist: any terminal (or IDE with an integrated terminal) is now learned automatically and per-workspace, so switching terminals follows you across workspaces.
- Prune terminal bindings for workspaces that no longer exist (checked against `herdr pane list` during cleanup), so the memory file stays bounded.

### Changed

- Remove the hardcoded terminal bundle-id whitelist (`is_known_terminal_bundle`); terminal recognition now relies entirely on the per-workspace binding learned at focus time.
- Remove all configuration: no `.env` file, no `HERDR_FOCUS_NOTIFY_*` variables. `blocked`/`done` notification, the 3600-second auto-dismiss timeout, alerting via `alerter` (auto-detected from `PATH` and common Homebrew paths), and per-workspace terminal activation are all built-in defaults. `HERDR_BIN_PATH` and `HERDR_PLUGIN_STATE_DIR` remain as environment hooks for tests and unusual installs.

- Zero-configuration terminal detection: the plugin now learns the terminal Herdr runs in from `pane.focused` events (restricted to a whitelist of known terminals and IDEs) and uses it to activate the terminal on click and to skip notifications for panes you are already looking at.
- `HERDR_FOCUS_NOTIFY_ACTIVATE_APP` accepts a comma-separated list, and each entry may be an app name, an absolute `.app` path (with `~/` expansion), or a bundle id (`open -b`).

### Changed

- Skip detection no longer requires `ACTIVATE_APP`: a focused pane is skipped when the frontmost app is a known terminal or an explicitly configured/learned one. Any other or unknown frontmost app still notifies, preserving the conservative default.

## [0.3.11] - 2026-08-15

### Added

- Show bundled notification icons for the `omp` and `pi` agents.

## [0.3.10] - 2026-08-13

### Changed

- Replace Herdr detection-rule explanations with simple, status-specific notification titles and descriptions.

## [0.3.9] - 2026-08-12

### Changed

- Reuse already-extracted agent icons instead of rewriting them on every notification.

### Fixed

- Clean stale `.cleared` focus markers during state-directory maintenance.
- Do not leave a `.cleared` marker behind when the notifier binary cannot be resolved.

## [0.3.8] - 2026-08-08

### Added

- Use Herdr's `agent explain --json` output to enrich `blocked` and `done` notifications with the matched detection rule and screen evidence.
- Support Herdr `state_labels` as a fallback notification detail.
- Add a Herdr startup hook that cleans stale generated scripts and notifier temporary files.
- Add `--cleanup` for manual state-directory maintenance.

### Changed

- Use `herdr agent get <pane_id>` for normal pane-focus checks.
- Use `herdr pane list` to find the focused pane in test mode.
- Reuse generated notification scripts per pane instead of creating a new script for every notification variant.
- Cap test notification timeouts at 10 seconds.
- Raise the minimum supported Herdr version to `0.7.5` for startup-hook support.

### Fixed

- Keep Cargo, plugin manifest, and lockfile versions aligned at `0.3.8`.
- Validate an explicitly configured Herdr binary before attempting notification delivery.
- Escape backslashes and quotes in AppleScript app lookups.
- Clean stale generated state files opportunistically during notification delivery.
