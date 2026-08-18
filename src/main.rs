mod cli;
mod event;
mod executable;
mod focus;
mod icons;
mod notification;
mod notifier;
mod script;
mod state;
mod util;

use std::env;
use std::process::ExitCode;

use cli::{parse_cli_args, print_usage, CliAction};
use event::{focused_pane_id_from_event_json, notification_from_event_json, status_is_enabled};
use executable::resolve_herdr_bin;
use focus::{
    learn_terminal_from_frontmost, notification_decision, should_clear_notification_on_focus,
    test_notification, NotificationDecision,
};
use notifier::{remove_notification, resolve_notifier_bin, send_notification};
use script::write_focus_script;
use state::{cleanup_stale_state_files, mark_notification_cleared, prune_stale_workspace_bindings, reset_notification_clearance};

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
            // Terminal bindings for workspaces that no longer exist are stale
            // too; best-effort, since Herdr may not be reachable.
            if let Ok(herdr_bin) = resolve_herdr_bin() {
                let _ = prune_stale_workspace_bindings(&herdr_bin);
            }
            return Ok(());
        }
        CliAction::CheckPaneVisibility(pane_id) => {
            let herdr_bin = resolve_herdr_bin()?;
            return (notification_decision(&pane_id, &herdr_bin) == NotificationDecision::Skip)
                .then_some(())
                .ok_or_else(|| "pane is not visible in the configured app".to_string());
        }
        CliAction::Event | CliAction::Test => {}
    }

    let herdr_bin = resolve_herdr_bin()?;

    let notification = match action {
        CliAction::Test => test_notification(&herdr_bin),
        CliAction::Event => {
            let event_json = match env::var("HERDR_PLUGIN_EVENT_JSON") {
                Ok(value) => value,
                Err(_) => return Ok(()),
            };

            if env::var("HERDR_PLUGIN_EVENT").as_deref() == Ok("pane.focused") {
                let Some(pane_id) = focused_pane_id_from_event_json(&event_json)? else {
                    return Ok(());
                };

                // Zero-configuration terminal detection: bind the frontmost
                // app to this pane's workspace, so future clicks can activate
                // it and skip checks can match it. Trusted without a whitelist
                // because a genuine pane.focused only fires while the user is
                // inside Herdr; a click-spawned focus (frontmost = browser)
                // gets corrected by the next genuine focus. Best-effort, so
                // it never hijacks the pane.focused handling.
                let workspace = util::workspace_id_from_pane_id(&pane_id).unwrap_or("default");
                learn_terminal_from_frontmost(workspace);

                if should_clear_notification_on_focus(workspace) {
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
        | CliAction::CheckPaneVisibility(_) => {
            unreachable!("handled before notification setup")
        }
    };

    if action != CliAction::Test && !status_is_enabled(&notification.status) {
        return Ok(());
    }

    let mut notification_decision = notification_decision(&notification.pane_id, &herdr_bin);
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

    reset_notification_clearance(&notification.pane_id)
        .map_err(|err| format!("failed to reset notification clearance: {err}"))?;

    let notifier_bin = resolve_notifier_bin()?;
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
