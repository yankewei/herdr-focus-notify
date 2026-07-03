use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::executable::{executable_in_path, find_executable, home_dir, is_executable_file};
use crate::util::shell_quote;

pub(crate) fn resolve_notifier_bin() -> Result<String, String> {
    if let Some(configured) = crate::config::config_var("HERDR_FOCUS_NOTIFY_NOTIFIER") {
        if is_executable_file(Path::new(&configured)) || executable_in_path(&configured).is_some() {
            return Ok(configured);
        }

        return Err(format!(
            "configured notifier is not executable: {configured}; install alerter with `brew install vjeantet/tap/alerter`"
        ));
    }

    find_executable("alerter", alerter_candidate_paths()).ok_or_else(|| {
        "no alerter notifier found; install alerter with `brew install vjeantet/tap/alerter` or set HERDR_FOCUS_NOTIFY_NOTIFIER".to_string()
    })
}

pub(crate) fn send_notification(script_path: &Path, foreground: bool) -> io::Result<()> {
    if foreground {
        run_script_foreground(script_path)
    } else {
        spawn_detached_script(script_path)
    }
}

fn alerter_candidate_paths() -> Vec<PathBuf> {
    let mut paths = vec![
        PathBuf::from("/opt/homebrew/bin/alerter"),
        PathBuf::from("/usr/local/bin/alerter"),
    ];
    if let Some(home) = home_dir() {
        paths.push(home.join(".local/bin/alerter"));
    }
    paths
}

fn run_script_foreground(script_path: &Path) -> io::Result<()> {
    match Command::new("sh").arg(script_path).status() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(io::Error::other(format!(
            "notification script exited with {status}"
        ))),
        Err(err) => Err(err),
    }
}

fn spawn_detached_script(script_path: &Path) -> io::Result<()> {
    let script_str = script_path.to_string_lossy();
    let cmd = format!(
        "nohup sh {} >/dev/null 2>&1 &",
        shell_quote(script_str.as_ref())
    );

    Command::new("sh")
        .arg("-c")
        .arg(&cmd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alerter_candidates_include_common_homebrew_paths() {
        let paths = alerter_candidate_paths();

        assert!(paths.contains(&PathBuf::from("/opt/homebrew/bin/alerter")));
        assert!(paths.contains(&PathBuf::from("/usr/local/bin/alerter")));
    }
}
