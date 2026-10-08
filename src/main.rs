mod cli;
mod event;
mod executable;
mod focus;
mod git;
mod icons;
mod notification;
mod notifier;
mod script;
mod state;
mod terminal;
mod util;

use std::env;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use cli::{parse_cli_args, print_usage, CliAction};
use event::{
    enrich_notification, focused_pane_id_from_event_json, notification_from_event_json,
    status_is_enabled,
};
use executable::resolve_herdr_bin;
use focus::{
    frontmost_bundle_id, learn_terminal_from_frontmost, notification_decision, pane_details,
    pane_labels, should_clear_notification_on_focus, test_notification, NotificationDecision,
};
use notifier::{remove_notification, resolve_notifier_bin, send_notification};
use script::{rewrite_generated_scripts_without_activation, write_focus_script};
use state::{
    cleanup_stale_state_files, clear_terminal_bindings, mark_notification_cleared,
    prune_stale_workspace_bindings, reset_notification_clearance,
};

/// Time the Herdr and git calls behind one notification get, together: the
/// `agent get` that decides whether to show it, and the lookups behind its
/// text. Every one of them is best-effort, so a slow Herdr or a pathological
/// repository costs at most this much delay, and the notification goes out
/// with whatever was answered in time.
const LOOKUP_BUDGET: Duration = Duration::from_secs(2);

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("herdr-focus-notify: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let action = parse_cli_args(env::args().skip(1))?;

    match action {
        CliAction::Help => {
            print_usage();
            return Ok(());
        }
        CliAction::Version => {
            println!("herdr-focus-notify {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        CliAction::Cleanup => {
            cleanup_stale_state_files()
                .map_err(|err| format!("failed to clean stale state files: {err}"))?;
            rewrite_generated_scripts_without_activation()
                .map_err(|err| format!("failed to update generated focus scripts: {err}"))?;
            // Terminal bindings for workspaces that no longer exist are stale
            // too; best-effort, since Herdr may not be reachable.
            if let Ok(herdr_bin) = resolve_herdr_bin() {
                let _ = prune_stale_workspace_bindings(&herdr_bin);
            }
            return Ok(());
        }
        CliAction::ClearTerminalBindings => {
            clear_terminal_bindings()
                .map_err(|err| format!("failed to clear terminal bindings: {err}"))?;
            rewrite_generated_scripts_without_activation()
                .map_err(|err| format!("failed to update generated focus scripts: {err}"))?;
            return Ok(());
        }
        CliAction::FocusPane(pane_id) => {
            return focus::focus_pane(&pane_id);
        }
        CliAction::CheckPaneVisibility(pane_id) => {
            let herdr_bin = resolve_herdr_bin()?;
            let deadline = Instant::now() + LOOKUP_BUDGET;
            let details = pane_details(&pane_id, &herdr_bin, deadline).unwrap_or_default();
            if notification_decision(&pane_id, details.focused) == NotificationDecision::Skip {
                return Ok(());
            }
            return Err("pane is not visible in its workspace's bound terminal".to_string());
        }
        CliAction::Event | CliAction::Test => {}
    }

    let herdr_bin = resolve_herdr_bin()?;

    let mut notification = match action {
        CliAction::Test => test_notification(&herdr_bin),
        CliAction::Event => {
            let Ok(event_json) = env::var("HERDR_PLUGIN_EVENT_JSON") else {
                return Ok(());
            };

            if env::var("HERDR_PLUGIN_EVENT").as_deref() == Ok("pane.focused") {
                let Some(pane_id) = focused_pane_id_from_event_json(&event_json)? else {
                    return Ok(());
                };

                // Zero-configuration terminal detection: bind the frontmost
                // terminal to this pane's workspace. Both decisions below share
                // one frontmost lookup, taken as early as possible because the
                // app it reports must still be the one the user focused from.
                // learn_terminal_from_frontmost ignores notification-originated
                // focus events and obvious non-terminal apps, while keeping this
                // event path best-effort.
                let frontmost = frontmost_bundle_id();
                let workspace = util::workspace_of(&pane_id);
                learn_terminal_from_frontmost(workspace, frontmost.as_deref());

                if should_clear_notification_on_focus(workspace, frontmost.as_deref()) {
                    let notifier_bin = resolve_notifier_bin()?;
                    mark_notification_cleared(&pane_id)
                        .map_err(|err| format!("failed to mark notification as cleared: {err}"))?;
                    remove_notification(&pane_id, &notifier_bin)
                        .map_err(|err| format!("failed to remove notification: {err}"))?;
                }

                return Ok(());
            }

            match notification_from_event_json(&event_json)? {
                Some(notification) => notification,
                None => return Ok(()),
            }
        }
        CliAction::Help
        | CliAction::Version
        | CliAction::Cleanup
        | CliAction::ClearTerminalBindings
        | CliAction::CheckPaneVisibility(_)
        | CliAction::FocusPane(_) => {
            unreachable!("handled before notification setup")
        }
    };

    if action != CliAction::Test && !status_is_enabled(&notification.status) {
        return Ok(());
    }

    // One `herdr agent get` answers both questions below: whether the user is
    // already looking at this pane, and what the pane is working on. The skip
    // decision runs before the remaining Herdr and git calls, because a
    // suppressed notification must not pay for them. An unanswered lookup
    // counts as unfocused, so the notification is sent.
    let deadline = Instant::now() + LOOKUP_BUDGET;
    let details = pane_details(&notification.pane_id, &herdr_bin, deadline).unwrap_or_default();

    let mut notification_decision = notification_decision(&notification.pane_id, details.focused);
    if notification_decision == NotificationDecision::Skip {
        if action == CliAction::Test {
            // When enabled, --test validates the pipeline end to end, so it
            // never suppresses the notification; it just goes without the
            // visibility monitor.
            notification_decision = NotificationDecision::Send;
        } else {
            return Ok(());
        }
    }

    // Resolved before the lookups behind the text, so an install without a
    // notifier fails without paying for them.
    let notifier_bin = resolve_notifier_bin()?;

    // `--test` deliberately keeps its own copy: it describes the plugin rather
    // than a pane, and it is the text a user reads while checking their setup
    // against what they expect to see.
    if action != CliAction::Test {
        let labels = pane_labels(&notification.pane_id, details, &herdr_bin, deadline);
        enrich_notification(&mut notification, &labels);
    }

    reset_notification_clearance(&notification.pane_id)
        .map_err(|err| format!("failed to reset notification clearance: {err}"))?;

    let script_path = write_focus_script(
        &notification,
        &herdr_bin,
        &notifier_bin,
        notification_decision == NotificationDecision::SendWithVisibilityMonitor,
        action == CliAction::Test,
    )
    .map_err(|err| format!("failed to write focus script: {err}"))?;

    send_notification(&script_path, action == CliAction::Test)
        .map_err(|err| format!("failed to send notification: {err}"))?;

    Ok(())
}
