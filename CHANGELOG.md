# Changelog

All notable changes to `herdr-focus-notify` are documented here.

## [0.3.9] - 2026-08-12

### Changed

- Reuse already-extracted agent icons instead of rewriting them on every notification.

### Fixed

- Clean stale `.cleared` focus markers during state-directory maintenance.
- Do not leave a `.cleared` marker behind when the notifier binary cannot be resolved.

## [Unreleased]

### Changed

- Replace Herdr detection-rule explanations with simple, status-specific notification titles and descriptions.

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
