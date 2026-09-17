# Changelog

All notable changes to `herdr-focus-notify` are documented here.

## [Unreleased]

### Fixed

- Bound the memory a pending notification can consume. `alerter` grows its resident memory for as long as it waits for a click — a steady ~12.9 MB/min with no plateau — and the notification timeout was one hour, so a single unclicked notification reached roughly 774 MB. Agents that finish while nobody is at the keyboard stack one waiter per pane: on a 29-pane session this left 11 concurrent `alerter` processes holding 2.6 GB, which pushed the machine into continuous swapping and drove load average to 289 on an 18-core Mac. The default timeout is now 5 minutes, capping one waiter near 65 MB.

### Added

- `HERDR_FOCUS_NOTIFY_TIMEOUT_SECS` overrides the notification timeout in seconds, for setups that want longer-lived notifications and can afford the memory. `0` keeps notifications up until clicked, as before.

## [0.6.0] - 2026-09-17

### Fixed

- Clicking a notification now focuses any pane, including ordinary shell panes without a detected agent. The click helper sends Herdr's raw `pane.focus` socket request over the injected `HERDR_SOCKET_PATH` instead of running `herdr agent focus` followed by `herdr tab focus`, which Herdr rejected with `agent_not_found` for shell panes: the terminal became frontmost but Herdr stayed on the previous tab and pane.

### Changed

- The `pane.focus` response is validated (echoed request id, `pane_info` result type, returned pane id), and the socket read gives up after 5 seconds so a silent Herdr server cannot strand the detached click process or hang the `--test` action in the foreground. Focus-origin marker cleanup is unchanged on every error path.
- The test notification copy is simplified to "Focus notification test / Click to return to this Herdr pane."

## [0.5.2] - 2026-09-17

### Fixed

- Learn a workspace's terminal promptly. The frontmost app is now read once per `pane.focused` event, and from `lsappinfo` instead of AppleScript/System Events, cutting the handler from ~350ms to ~20ms. A binding describes a focus change that has already happened, so the slower lookup observed whatever app was frontmost afterwards: switching apps right after focusing a pane silently left that workspace unbound, and an unbound workspace makes a notification click a silent no-op.

### Changed

- The frontmost-app lookup no longer uses AppleScript, so it no longer depends on System Events automation permission.

## [0.5.1] - 2026-09-15

### Fixed

- Prevent notification-triggered focus events from rebinding a workspace to the frontmost browser or notification app.
- Skip terminal activation and Herdr focus when a workspace has no saved terminal binding.
- Move terminal activation to click time and add a plugin action for clearing saved bindings, including captured activation commands in existing generated scripts.
- Clear the focus-origin marker on every failed notification focus path.
- Continue rewriting generated scripts when individual files disappear or cannot be read.

## [0.5.0] - 2026-09-08

### Fixed

- Restore notification click navigation on Herdr 0.9.0 by focusing the returned agent tab after selecting its pane, synchronizing attached client views.
- Document that Herdr 0.9.0 users need plugin tag `v0.5.0` or later. Workspace-to-terminal binding behavior is unchanged.

## [0.4.0] - 2026-08-18

### Added

- Bind the terminal to each Herdr workspace from `pane.focused` events, replacing the bundled terminal whitelist: any terminal (or IDE with an integrated terminal) is now learned automatically and per-workspace, so switching terminals follows you across workspaces.
- Prune terminal bindings for workspaces that no longer exist (checked against `herdr pane list` during cleanup), so the memory file stays bounded.

### Changed

- Remove the hardcoded terminal bundle-id whitelist (`is_known_terminal_bundle`); terminal recognition now relies entirely on the per-workspace binding learned at focus time.
- Internal refactor: terminal lookup collapsed from a set to the single bound value, and herdr subprocess calls deduplicated into one helper. No behavior change.
- Remove all configuration: no `.env` file, no `HERDR_FOCUS_NOTIFY_*` variables. `blocked`/`done` notification, the 3600-second auto-dismiss timeout, alerting via `alerter` (auto-detected from `PATH` and common Homebrew paths), and per-workspace terminal activation are all built-in defaults. `HERDR_BIN_PATH` and `HERDR_PLUGIN_STATE_DIR` remain as environment hooks for tests and unusual installs.
- Skip detection now keys on the learned per-workspace binding: a focused pane is skipped only when the frontmost app is the terminal bound to its workspace. Any other or unknown frontmost app still notifies, preserving the conservative default.

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
