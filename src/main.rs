mod cli;
mod config;
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
use config::{is_enabled, status_is_enabled};
use event::{focused_pane_id_from_event_json, notification_from_event_json};
use executable::resolve_herdr_bin;
use focus::{should_clear_notification_on_focus, should_skip_notification, test_notification};
use notifier::{remove_notification, resolve_notifier_bin, send_notification};
use script::write_focus_script;
use state::{mark_notification_cleared, reset_notification_clearance};

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
        CliAction::CheckPaneVisibility(pane_id) => {
            let herdr_bin = resolve_herdr_bin();
            return should_skip_notification(&pane_id, &herdr_bin)
                .then_some(())
                .ok_or_else(|| "pane is not visible in the configured app".to_string());
        }
        CliAction::Event | CliAction::Test => {}
    }

    if !is_enabled() {
        return Ok(());
    }

    let herdr_bin = resolve_herdr_bin();

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

                if should_clear_notification_on_focus() {
                    mark_notification_cleared(&pane_id)
                        .map_err(|err| format!("failed to mark notification as cleared: {err}"))?;
                    let notifier_bin = resolve_notifier_bin()?;
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
        CliAction::Help | CliAction::Version | CliAction::CheckPaneVisibility(_) => {
            unreachable!("handled before notification setup")
        }
    };

    if !status_is_enabled(&notification.status) {
        return Ok(());
    }

    if should_skip_notification(&notification.pane_id, &herdr_bin) {
        return Ok(());
    }

    reset_notification_clearance(&notification.pane_id)
        .map_err(|err| format!("failed to reset notification clearance: {err}"))?;

    let notifier_bin = resolve_notifier_bin()?;
    let script_path = write_focus_script(&notification, &herdr_bin, &notifier_bin)
        .map_err(|err| format!("failed to write focus script: {err}"))?;

    send_notification(&script_path, action == CliAction::Test)
        .map_err(|err| format!("failed to send notification: {err}"))?;

    Ok(())
}
