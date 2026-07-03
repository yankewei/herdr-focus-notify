mod cli;
mod config;
mod event;
mod executable;
mod focus;
mod notification;
mod notifier;
mod script;
mod util;

use std::env;
use std::process::ExitCode;

use cli::{parse_cli_args, print_usage, CliAction};
use config::{is_enabled, status_is_enabled};
use event::notification_from_event_json;
use executable::resolve_herdr_bin;
use focus::{should_skip_notification, test_notification};
use notifier::{resolve_notifier_bin, send_notification};
use script::write_focus_script;

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

            match notification_from_event_json(&event_json)? {
                Some(notification) => notification,
                None => return Ok(()),
            }
        }
        CliAction::Help | CliAction::Version => unreachable!("handled before notification setup"),
    };

    if !status_is_enabled(&notification.status) {
        return Ok(());
    }

    if should_skip_notification(&notification.pane_id, &herdr_bin) {
        return Ok(());
    }

    let notifier_bin = resolve_notifier_bin()?;
    let script_path = write_focus_script(&notification, &herdr_bin, &notifier_bin)
        .map_err(|err| format!("failed to write focus script: {err}"))?;

    send_notification(&script_path, action == CliAction::Test)
        .map_err(|err| format!("failed to send notification: {err}"))?;

    Ok(())
}
