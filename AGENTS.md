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
├── src/*.rs            # Focused modules for CLI, config, event parsing, focus checks, git summaries, scripts, and notifier delivery
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
   - The decision is one of `Skip`, `Send`, or `SendWithVisibilityMonitor`: `Skip` only when the target pane is focused **and** the frontmost macOS app matches the terminal bound to the pane's workspace (learned from `pane.focused` events); `SendWithVisibilityMonitor` when the pane is focused but the frontmost app differs from the bound terminal, so the notification auto-dismisses once the pane is seen; plain `Send` otherwise (including when the frontmost app or the binding is unknown), to avoid missing a state change.
   - `pane.focused` events bind the frontmost terminal to the pane's workspace (`learn_terminal_from_frontmost`), so the plugin works with zero configuration. Notification-originated focus events are marked and cannot overwrite the binding; obvious non-terminal bundle IDs are ignored as a defense in depth.
   - Recognized agent names are matched to bundled local PNG icons and passed to `alerter` with `--app-icon`.
   - Notification text is laid out like the Agent sidebar's own rows: the title is `{state} · {workspace label} · {tab label}`, the subtitle is the agent plus the pane's git state (`branch* · +inserted/-deleted`), and the message is the pane's terminal title. One `herdr agent get` supplies the skip decision and the pane's details (`pane_details()` in `src/focus.rs`); `workspace_label()` and `tab_label()` add the workspace label and tab label from `herdr workspace list` and `herdr tab get`, `worktree_branch()` takes the branch from `herdr worktree list --cwd`, and `src/git.rs` adds the line counts it changes versus `HEAD`. All of that runs only for a notification that is going to be shown. The `agent get` and every lookup after it share one 2-second budget (`LOOKUP_BUDGET` in `src/main.rs`): each call is killed with its process group at the deadline, a call the budget no longer covers is skipped (an unanswered `agent get` sends rather than skips), and the git probe runs only once a branch is known. A stale `agent get` reply about another pane is discarded whole. An unnamed tab is shown by its number. A long branch is middle-truncated so it cannot push the counts off the subtitle's single line. Everything is best-effort: any field Herdr or git does not answer leaves the event's own message in place. The title, message, and subtitle reach `alerter` in the `--option=value` form, because a separate value that starts with `-` (a terminal title such as `-zsh`) is taken for an option and the notification is lost. Static status-specific copy remains the fallback, and `--test` is never enriched. The plugin still does not read or summarize pane contents, and does not ask Herdr to explain its detection.
4. **Binary resolution**:
   - `herdr` is resolved from `HERDR_BIN_PATH`, then `PATH`, then hard-coded candidates (`~/.local/bin/herdr`, `/opt/homebrew/bin/herdr`, `/usr/local/bin/herdr`), defaulting to `"herdr"`.
   - The notifier backend is resolved from `PATH`, then hard-coded candidates for `alerter` (e.g. Homebrew paths).
5. **Focus script generation**:
   - A shell script is written to `HERDR_PLUGIN_STATE_DIR` (falling back to `$TMPDIR/herdr-focus-notify`).
   - The script name is a hash of the pane ID, so repeated events for one pane reuse the same script path. Old generated scripts and crashed notifier temp files are cleaned up opportunistically.
   - The script is made executable with mode `0o700`.
6. **Notification delivery**:
   - Normal plugin events spawn the script detached via `nohup sh ... &`. The script itself calls `alerter`, then invokes the binary's internal `--focus-pane` action if the user clicks the notification. That action re-reads the current workspace binding, activates it with `open -b <bound bundle id>` (for terminals with an adapter in `src/terminal/`, currently kitty and iTerm2, first selecting the container of a Herdr client attached to the session: kitty via `kitten @ focus-window`, iTerm2 via its `iterm2:reveal?sessionid=` URL; see `src/terminal/mod.rs` to add a terminal), marks the operation as plugin-originated, and sends Herdr's raw `pane.focus` socket request to display the target workspace, tab, and pane atomically. This works for both agent and ordinary shell panes. With no binding, the action exits without activating an app or invoking Herdr.
   - `--test` runs the generated script in the foreground so notifier failures surface through stderr and a non-zero exit code.

## Configuration

The plugin is zero-config: as of 0.4.0 there is no `.env` file and no `HERDR_FOCUS_NOTIFY_*` variables. Notification statuses (`blocked`, `done`), the 3600-second auto-dismiss timeout, `alerter` auto-detection, per-workspace terminal activation, and the **Clear saved terminal bindings** action are built-in defaults.

Three environment hooks remain for tests and unusual installs:

| Variable | Effect |
|---|---|
| `HERDR_BIN_PATH` | Explicit path to the `herdr` binary; takes precedence over `PATH` and the hard-coded candidates. |
| `HERDR_PLUGIN_STATE_DIR` | Overrides the state directory, where generated scripts, `terminal-memory.json`, focus-origin markers, and cleanup markers live (falls back to `$TMPDIR/herdr-focus-notify`). |
| `HERDR_SOCKET_PATH` | Herdr's injected local socket path, used by notification clicks to send an atomic `pane.focus` request. |

Herdr itself also sets `HERDR_PLUGIN_EVENT_JSON` (event payload) and `HERDR_PLUGIN_EVENT` (event name) when invoking the plugin.

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
- Some runtime behavior (the real `lsappinfo` frontmost-app lookup, actual alerter invocation, `herdr agent get`, and the test-mode `herdr pane list`) cannot be exercised in CI and is only validated manually on macOS.

## Important Gotchas

- **macOS only**: The plugin manifest declares `platforms = ["macos"]`. The binary shells out to macOS-only tools (`lsappinfo` for the frontmost app, `open -b` for terminal activation, `alerter` for delivery); it will not behave correctly on other platforms.
- **No-event quiet path**: A normal plugin invocation without `HERDR_PLUGIN_EVENT_JSON` exits quietly with `0`. Real parsing, script, and notifier errors should surface through stderr and non-zero exit codes.
- **Skip logic is conservative**: A notification is only suppressed when the plugin can *confirm* the pane is focused and the frontmost app is the terminal bound to the pane's workspace. Any ambiguity (a failed `lsappinfo` lookup, an unknown frontmost app, a missing binding) results in a notification being sent. The learned-terminal file lives in the state directory as `terminal-memory.json` and is deliberately excluded from the stale-file sweep. Obvious non-terminal bundle IDs are treated as unbound when read.
- **State directory hygiene**: Generated scripts are keyed by a hash of the pane ID, so repeated events for one pane reuse the same script path. A retention sweep removes stale generated scripts (30 days), focus-origin markers (15 seconds), crashed notifier temp files (24 hours), and `.cleared` focus markers (24 hours); it runs on `--cleanup` (including the Herdr startup hook) and opportunistically before each notification. Cleanup and the clear-bindings action also remove the old captured `open -b` line from scripts generated by earlier versions.
- **`herdr-plugin.toml` is the source of truth for execution**: Herdr invokes `target/release/herdr-focus-notify` directly for events and actions, not `cargo run`. The binary must be built before the plugin action/event works.
