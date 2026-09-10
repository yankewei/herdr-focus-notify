# Agent Notes for `herdr-focus-notify`

## Project Type

A Rust CLI binary that runs as a **Herdr plugin** on macOS. It listens for Herdr's `pane.agent_status_changed` event and emits clickable macOS desktop notifications via `alerter`. Clicking a notification focuses the matching Herdr agent pane.

The plugin manifest is in [`herdr-plugin.toml`](herdr-plugin.toml). The binary is built by Herdr itself using the command declared in that manifest.

## Essential Commands

| Command | Purpose |
|---|---|
| `cargo build --release` | Build the release binary to `target/release/herdr-focus-notify`. This is exactly what the plugin manifest uses. |
| `cargo test` | Run unit and integration tests. |
| `cargo fmt -- --check` | Check formatting. |
| `cargo clippy --all-targets --all-features -- -D warnings` | Lint; CI treats warnings as errors. |
| `herdr plugin link .` | Install the plugin locally from the repo root. |
| `target/release/herdr-focus-notify --test` | Trigger a test notification manually (declared as an action in `herdr-plugin.toml`). |

CI runs on `macos-latest` and executes all of the above in order (see [`.github/workflows/ci.yml`](.github/workflows/ci.yml)).

## Project Structure

```
.
├── Cargo.toml          # Rust package metadata; minimal dependencies (serde, serde_json)
├── herdr-plugin.toml   # Herdr plugin manifest: build command, actions, event subscriptions
├── src/main.rs         # Thin CLI/plugin entry point
├── src/*.rs            # Focused modules for CLI, config, event parsing, focus checks, scripts, and notifier delivery
├── assets/icons        # Bundled local agent icons used by alerter --app-icon
├── tests/cli_test.rs   # Process-level CLI contract tests
├── README.md           # English documentation
└── README.zh-CN.md     # Chinese documentation
```

There are no submodules, no external crates beyond serde/serde_json, and no build scripts.

## Architecture and Control Flow

1. **Entry point**: `main()` calls `run()`, prints real errors to stderr, and returns a non-zero exit code.
2. **Event source**:
   - In normal mode, the binary reads the Herdr event from the `HERDR_PLUGIN_EVENT_JSON` environment variable.
   - In test mode (`--test` CLI arg), it fabricates a notification for the currently focused pane. When the plugin is enabled, test mode always sends: it bypasses the status filter and downgrades a `Skip` focus decision to a plain send (no visibility monitor), so the action can validate the pipeline even in a fully configured setup.
   - `--help` and `--version` print to stdout before plugin setup.
3. **Notification decision**:
   - Only `blocked` and `done` statuses can produce notifications (they are the ones that need user action). There is no configuration to change this set.
   - The decision is one of `Skip`, `Send`, or `SendWithVisibilityMonitor`: `Skip` only when the target pane is focused **and** the frontmost macOS app matches the terminal bound to the pane's workspace (learned from Herdr focus events); `SendWithVisibilityMonitor` when the pane is focused but the frontmost app differs from the bound terminal, so the notification auto-dismisses once the pane is seen; plain `Send` otherwise (including when the frontmost app or the binding is unknown), to avoid missing a state change.
   - `pane.focused` and `tab.focused` events bind the frontmost terminal to the event's workspace (`learn_terminal_from_frontmost`), so the plugin works with zero configuration. For `tab.focused`, notification clearing prefers Herdr's event-scoped `HERDR_PANE_ID` over a live pane lookup, avoiding races if the user switches tabs again before the asynchronous hook runs.
   - Recognized agent names are matched to bundled local PNG icons and passed to `alerter` with `--app-icon`.
   - Notification titles and bodies use short status-specific copy: blocked agents ask the user to review and respond, while done agents ask the user to review the result. The plugin does not read or summarize pane contents.
4. **Binary resolution**:
   - `herdr` is resolved from `HERDR_BIN_PATH`, then `PATH`, then hard-coded candidates (`~/.local/bin/herdr`, `/opt/homebrew/bin/herdr`, `/usr/local/bin/herdr`), defaulting to `"herdr"`.
   - The notifier backend is resolved from `PATH`, then hard-coded candidates for `alerter` (e.g. Homebrew paths).
5. **Focus script generation**:
   - A shell script is written to `HERDR_PLUGIN_STATE_DIR` (falling back to `$TMPDIR/herdr-focus-notify`).
   - The script name is a hash of the pane ID, so repeated events for one pane reuse the same script path. Old generated scripts and crashed notifier temp files are cleaned up opportunistically.
   - The script is made executable with mode `0o700`.
6. **Notification delivery**:
   - Normal plugin events spawn the script detached via `nohup sh ... &`. The script itself calls `alerter`, then activates the terminal learned for the pane's workspace (`open -b <bound bundle id>`) and invokes the binary’s internal `--focus-pane` action if the user clicks the notification. It runs `herdr agent focus <pane>`, then `herdr tab focus <tab_id>` with the returned tab ID to synchronize Herdr 0.9.0 client views.
   - `--test` runs the generated script in the foreground so notifier failures surface through stderr and a non-zero exit code.

## Configuration

The plugin is zero-config: as of 0.4.0 there is no `.env` file and no `HERDR_FOCUS_NOTIFY_*` variables. Notification statuses (`blocked`, `done`), the 3600-second auto-dismiss timeout, `alerter` auto-detection, and per-workspace terminal activation are built-in defaults.

Two environment hooks remain for tests and unusual installs:

| Variable | Effect |
|---|---|
| `HERDR_BIN_PATH` | Explicit path to the `herdr` binary; takes precedence over `PATH` and the hard-coded candidates. |
| `HERDR_PLUGIN_STATE_DIR` | Overrides the state directory, where generated scripts, `terminal-memory.json`, and cleanup markers live (falls back to `$TMPDIR/herdr-focus-notify`). |

Herdr itself sets `HERDR_PLUGIN_EVENT_JSON` (event payload), `HERDR_PLUGIN_EVENT` (event name), and event-scoped context such as `HERDR_PANE_ID` when invoking the plugin.

Bundled agent icons are extracted from `@lobehub/icons-static-png` (except `omp.png` and `pi.png`, which use the official Oh My Pi and Pi logos) and attributed in `assets/icons/NOTICE.md`.

## Code Patterns and Conventions

- **Module boundaries**: `src/main.rs` stays thin; parsing, focus checks, notifier delivery, shell script generation, state management, and small utilities live in separate modules.
- **Error style**: Top-level functions that can fail return `Result<T, String>` with prefixed messages (e.g. `"failed to write focus script: {err}"`).
- **Option-heavy parsing**: Event and CLI JSON fields are mostly optional; the code uses `Option<T>` everywhere and falls back to defaults or skips silently.
- **Shell quoting**: `shell_quote()` wraps values in single quotes and escapes embedded single quotes with `'\''`.
- **Platform gating**: `#[cfg(unix)]` guards executable-bit checks and `make_executable`; other platforms compile but behave differently.
- **No logging crate**: The production code writes no debug logs; errors go to stderr and a non-zero exit code.

## Testing

- Unit tests are inline under each module's `#[cfg(test)] mod tests`; process-level CLI behavior lives under `tests/`.
- Run with `cargo test`.
- Tests cover JSON parsing, notification body construction, shell quoting, focus script generation, and skip logic.
- Some runtime behavior (AppleScript bundle-ID detection, actual alerter invocation, `herdr agent get`, and the test-mode `herdr pane list`) cannot be exercised in CI and is only validated manually on macOS.

## Important Gotchas

- **macOS only**: The plugin manifest declares `platforms = ["macos"]`. The binary uses AppleScript (`osascript`) and macOS-specific app/bundle APIs; it will not behave correctly on other platforms.
- **No-event quiet path**: A normal plugin invocation without `HERDR_PLUGIN_EVENT_JSON` exits quietly with `0`. Real parsing, script, and notifier errors should surface through stderr and non-zero exit codes.
- **Skip logic is conservative**: A notification is only suppressed when the plugin can *confirm* the pane is focused and the frontmost app is the terminal bound to the pane's workspace. Any ambiguity (AppleScript failure, unknown frontmost app, missing binding) results in a notification being sent. The learned-terminal file lives in the state directory as `terminal-memory.json` and is deliberately excluded from the stale-file sweep.
- **State directory hygiene**: Generated scripts are keyed by a hash of the pane ID, so repeated events for one pane reuse the same script path. A retention sweep removes stale generated scripts (30 days), crashed notifier temp files (24 hours), and `.cleared` focus markers (24 hours); it runs on `--cleanup` (including the Herdr startup hook) and opportunistically before each notification.
- **`herdr-plugin.toml` is the source of truth for execution**: Herdr invokes `target/release/herdr-focus-notify` directly for events and actions, not `cargo run`. The binary must be built before the plugin action/event works.
