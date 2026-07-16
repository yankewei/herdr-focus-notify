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
├── .env.example        # Documented configuration template for plugin users
├── README.md           # English documentation
└── README.zh-CN.md     # Chinese documentation
```

There are no submodules, no external crates beyond serde/serde_json, and no build scripts.

## Architecture and Control Flow

1. **Entry point**: `main()` calls `run()`, prints real errors to stderr, and returns a non-zero exit code.
2. **Event source**:
   - In normal mode, the binary reads the Herdr event from the `HERDR_PLUGIN_EVENT_JSON` environment variable.
   - In test mode (`--test` CLI arg), it fabricates a notification for the currently focused pane.
   - `--help` and `--version` print to stdout before plugin setup.
3. **Notification decision**:
   - Only `blocked` and `done` statuses can produce notifications (they are the ones that need user action); `HERDR_FOCUS_NOTIFY_STATUSES` can only narrow that set, never extend it.
   - The notification is skipped if the target pane is already focused **and** the frontmost macOS application belongs to the same bundle ID as the configured `ACTIVATE_APP` (see `should_skip_from_focus_and_bundles`). If either check fails, the plugin sends the notification to avoid missing a state change.
   - Recognized agent names are matched to bundled local PNG icons and passed to `alerter` with `--app-icon`.
4. **Binary resolution**:
   - `herdr` is resolved from `HERDR_BIN_PATH`, then `PATH`, then hard-coded candidates (`~/.local/bin/herdr`, `/opt/homebrew/bin/herdr`, `/usr/local/bin/herdr`), defaulting to `"herdr"`.
   - The notifier backend is resolved from `HERDR_FOCUS_NOTIFY_NOTIFIER`, then `PATH`, then hard-coded candidates for `alerter`.
5. **Focus script generation**:
   - A shell script is written to `HERDR_PLUGIN_STATE_DIR` (falling back to `$TMPDIR/herdr-focus-notify`).
   - The script name is a hash of the notification fields plus config, so repeated identical events reuse the same script path.
   - The script is made executable with mode `0o700`.
6. **Notification delivery**:
   - Normal plugin events spawn the script detached via `nohup sh ... &`. The script itself calls `alerter`, then runs `herdr agent focus <pane>` if the user clicks the notification.
   - `--test` runs the generated script in the foreground so notifier failures surface through stderr and a non-zero exit code.

## Configuration

Users configure the plugin by creating a `.env` file in the directory returned by `herdr plugin config-dir herdr-focus-notify`. The runtime loads it via `HERDR_PLUGIN_CONFIG_DIR`. See `.env.example` for all documented variables.

Key variables observed in code:

| Variable | Effect |
|---|---|
| `HERDR_FOCUS_NOTIFY_ENABLED` | `0`/`false`/`no`/`off` disables the plugin. |
| `HERDR_FOCUS_NOTIFY_STATUSES` | Comma-separated subset of `blocked,done` that triggers notifications. Default: `blocked,done`. |
| `HERDR_FOCUS_NOTIFY_NOTIFIER` | Path to the notifier backend. Defaults to auto-detected `alerter`. |
| `HERDR_FOCUS_NOTIFY_ACTIVATE_APP` | Terminal app name (e.g. `kitty`) or `.app` path (e.g. `/Applications/kitty.app`) to activate on click and to resolve a bundle ID for skip detection. |
| `HERDR_FOCUS_NOTIFY_TIMEOUT` | Alerter auto-dismiss timeout in seconds; `0` disables auto-dismiss. Default: `3600`. |
| `HERDR_FOCUS_NOTIFY_DEBUG` | `1`/`true`/`yes`/`on` enables stderr diagnostics and a `focus-click.log` in the state directory. |

Important: `HERDR_FOCUS_NOTIFY_ACTIVATE_APP` values containing `/` are passed to `open` directly; values without `/` are passed to `open -a`. Paths are **not shell-expanded**, so `~` is treated literally.

Bundled agent icons are extracted from `@lobehub/icons-static-png` and attributed in `assets/icons/NOTICE.md`.

## Code Patterns and Conventions

- **Module boundaries**: `src/main.rs` stays thin; parsing, config, focus checks, notifier delivery, shell script generation, and small utilities live in separate modules.
- **Error style**: Top-level functions that can fail return `Result<T, String>` with prefixed messages (e.g. `"failed to write focus script: {err}"`).
- **Option-heavy parsing**: Event and CLI JSON fields are mostly optional; the code uses `Option<T>` everywhere and falls back to defaults or skips silently.
- **Shell quoting**: `shell_quote()` wraps values in single quotes and escapes embedded single quotes with `'\''`.
- **Platform gating**: `#[cfg(unix)]` guards executable-bit checks and `make_executable`; other platforms compile but behave differently.
- **No logging crate**: Debug output is controlled by the custom `HERDR_FOCUS_NOTIFY_DEBUG` env var and is written to the generated script's log file, not to a Rust logger.

## Testing

- Unit tests are inline under each module's `#[cfg(test)] mod tests`; process-level CLI behavior lives under `tests/`.
- Run with `cargo test`.
- Tests cover JSON parsing, notification body construction, shell quoting, focus script generation, and skip logic.
- Some runtime behavior (AppleScript bundle-ID detection, actual alerter invocation, `herdr agent list`) cannot be exercised in CI and is only validated manually on macOS.

## Important Gotchas

- **macOS only**: The plugin manifest declares `platforms = ["macos"]`. The binary uses AppleScript (`osascript`) and macOS-specific app/bundle APIs; it will not behave correctly on other platforms.
- **No-event quiet path**: A normal plugin invocation without `HERDR_PLUGIN_EVENT_JSON` exits quietly with `0`. Real configuration, parsing, script, and notifier errors should surface through stderr and non-zero exit codes.
- **Skip logic is conservative**: A notification is only suppressed when the plugin can *confirm* the pane is focused and the frontmost app matches. Any ambiguity (missing bundle ID, AppleScript failure, missing `ACTIVATE_APP`) results in a notification being sent.
- **State directory hygiene**: Generated scripts are keyed by a hash of notification content and config. They are not automatically cleaned up; over time the state directory may accumulate scripts.
- **`herdr-plugin.toml` is the source of truth for execution**: Herdr invokes `target/release/herdr-focus-notify` directly for events and actions, not `cargo run`. The binary must be built before the plugin action/event works.
- **Env file parsing is custom**: `parse_env_file()` supports `KEY=value`, optional `export KEY=value`, quoted values, double-quoted escapes for common whitespace characters, and unquoted inline comments. It does not support variable interpolation.
