#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CliAction {
    Event,
    Test,
    Help,
    Version,
    CheckPaneVisibility(String),
}

pub(crate) fn parse_cli_args<I, S>(args: I) -> Result<CliAction, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut action = CliAction::Event;

    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let arg = arg.as_ref();
        match arg {
            "--test" => set_action(&mut action, CliAction::Test, arg)?,
            "-h" | "--help" => set_action(&mut action, CliAction::Help, arg)?,
            "-V" | "--version" => set_action(&mut action, CliAction::Version, arg)?,
            "--check-pane-visibility" => {
                if action != CliAction::Event {
                    return Err(format!("cannot combine {arg} with another command"));
                }
                let pane_id = args
                    .next()
                    .ok_or_else(|| format!("missing pane ID after {arg}"))?;
                action = CliAction::CheckPaneVisibility(pane_id.as_ref().to_string());
            }
            _ => {
                return Err(format!(
                    "unknown argument: {arg}; run with --help for usage"
                ));
            }
        }
    }

    Ok(action)
}

fn set_action(action: &mut CliAction, next: CliAction, arg: &str) -> Result<(), String> {
    if *action != CliAction::Event {
        return Err(format!("cannot combine {arg} with another command"));
    }

    *action = next;
    Ok(())
}

pub(crate) fn print_usage() {
    println!(
        "herdr-focus-notify {}\n\nUsage:\n  herdr-focus-notify\n  herdr-focus-notify --test\n\nOptions:\n  --test       Send a test focus notification\n  -h, --help   Show this help\n  -V, --version\n              Show the version",
        env!("CARGO_PKG_VERSION")
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cli_actions() {
        assert_eq!(
            parse_cli_args(Vec::<&str>::new()).unwrap(),
            CliAction::Event
        );
        assert_eq!(parse_cli_args(["--test"]).unwrap(), CliAction::Test);
        assert_eq!(parse_cli_args(["--help"]).unwrap(), CliAction::Help);
        assert_eq!(parse_cli_args(["-h"]).unwrap(), CliAction::Help);
        assert_eq!(parse_cli_args(["--version"]).unwrap(), CliAction::Version);
        assert_eq!(parse_cli_args(["-V"]).unwrap(), CliAction::Version);
        assert_eq!(
            parse_cli_args(["--check-pane-visibility", "w1:p2"]).unwrap(),
            CliAction::CheckPaneVisibility("w1:p2".to_string())
        );
    }

    #[test]
    fn rejects_unknown_or_combined_cli_args() {
        assert!(parse_cli_args(["--wat"]).is_err());
        assert!(parse_cli_args(["--test", "--help"]).is_err());
        assert!(parse_cli_args(["--check-pane-visibility"]).is_err());
    }
}
