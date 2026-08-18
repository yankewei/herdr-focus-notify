use std::collections::hash_map::DefaultHasher;
use std::env;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io;
use std::path::{Path, PathBuf};

use crate::focus::effective_terminal_bundle_ids;
use crate::notification::FocusNotification;
use crate::state::{cleanup_stale_state_files, cleared_notification_marker_path, plugin_state_dir, prune_stale_workspace_bindings};
use crate::util::shell_quote;

/// How long an unclicked notification stays up (seconds) before alerter
/// auto-dismisses it; 0 would keep it forever.
const ALERTER_TIMEOUT_SECS: u64 = 3600;

pub(crate) fn write_focus_script(
    notification: &FocusNotification,
    herdr_bin: &str,
    notifier_bin: &str,
    monitor_visibility: bool,
    test_mode: bool,
) -> io::Result<PathBuf> {
    let state_dir = plugin_state_dir();
    fs::create_dir_all(&state_dir)?;
    // State cleanup is maintenance work; a stale file must not prevent a new
    // notification from being delivered.
    let _ = cleanup_stale_state_files();
    let _ = prune_stale_workspace_bindings(herdr_bin);

    let timeout_secs = if test_mode {
        test_timeout_secs(ALERTER_TIMEOUT_SECS)
    } else {
        ALERTER_TIMEOUT_SECS
    };

    let mut hasher = DefaultHasher::new();
    notification.pane_id.hash(&mut hasher);

    let script_path = state_dir.join(format!("focus-{:016x}.sh", hasher.finish()));
    let executable_path = monitor_visibility
        .then(|| env::current_exe().ok())
        .flatten();
    let script = focus_script_content_with_timeout(
        notification,
        herdr_bin,
        notifier_bin,
        timeout_secs,
        executable_path.as_deref(),
    );

    fs::write(&script_path, script)?;
    make_executable(&script_path)?;

    Ok(script_path)
}

fn focus_script_content_with_timeout(
    notification: &FocusNotification,
    herdr_bin: &str,
    notifier_bin: &str,
    timeout_secs: u64,
    executable_path: Option<&Path>,
) -> String {
    let workspace = crate::util::workspace_id_from_pane_id(&notification.pane_id).unwrap_or("default");
    // The visibility monitor needs a terminal it can match the frontmost app
    // against; with none configured or learned, deliver a plain notification.
    let visibility_check_binary = if effective_terminal_bundle_ids(workspace).is_empty() {
        None
    } else {
        executable_path
    };

    alerter_focus_script(
        notification,
        herdr_bin,
        notifier_bin,
        timeout_secs,
        activation_command(workspace).as_deref(),
        visibility_check_binary,
    )
}

fn test_timeout_secs(configured: u64) -> u64 {
    if configured == 0 {
        10
    } else {
        configured.min(10)
    }
}

fn alerter_focus_script(
    notification: &FocusNotification,
    herdr_bin: &str,
    notifier_bin: &str,
    timeout_secs: u64,
    activate_command: Option<&str>,
    visibility_check_binary: Option<&Path>,
) -> String {
    let title_q = shell_quote(&notification.title);
    let body_q = shell_quote(&notification.body);
    let group_q = shell_quote(&notification.group);
    let pane_q = shell_quote(&notification.pane_id);
    let herdr_q = shell_quote(herdr_bin);
    let notifier_q = shell_quote(notifier_bin);
    let cleared_marker = cleared_notification_marker_path(&notification.pane_id);
    let cleared_marker_q = shell_quote(cleared_marker.to_string_lossy().as_ref());
    let app_icon_args = notification
        .app_icon
        .as_ref()
        .map(|path| format!(" --app-icon {}", shell_quote(path)))
        .unwrap_or_default();
    let timeout_args = if timeout_secs > 0 {
        format!(" --timeout {}", timeout_secs)
    } else {
        String::new()
    };
    let visibility_check_command = visibility_check_binary.map(|binary| {
        format!(
            "{} --check-pane-visibility {}",
            shell_quote(binary.to_string_lossy().as_ref()),
            pane_q
        )
    });
    let result_template_q = shell_quote(&format!("{}.result.XXXXXX", cleared_marker.display()));
    let status_template_q = shell_quote(&format!("{}.status.XXXXXX", cleared_marker.display()));

    let mut script = String::from("#!/bin/sh\n");
    script.push_str(&format!(
        "[ -e {cleared_marker} ] && exit 0\n",
        cleared_marker = cleared_marker_q
    ));
    script.push_str(&format!(
        "result_path=$(mktemp {result_template}) || exit 1\nstatus_path=$(mktemp {status_template}) || {{ rm -f \"$result_path\"; exit 1; }}\nmonitor_pid=\ncleanup() {{\n  [ -z \"$monitor_pid\" ] || kill \"$monitor_pid\" 2>/dev/null\n  rm -f \"$result_path\" \"$status_path\"\n}}\ntrap cleanup EXIT\n(\n  {notifier} --title {title} --message {body} --group {group}{app_icon_args} --actions {action} --close-label {close_label}{timeout_args} > \"$result_path\" 2>/dev/null\n  printf '%s' \"$?\" > \"$status_path\"\n) &\nnotifier_pid=$!\n",
        result_template = result_template_q,
        status_template = status_template_q,
        notifier = notifier_q,
        title = title_q,
        body = body_q,
        group = group_q,
        app_icon_args = app_icon_args,
        action = shell_quote("Focus"),
        close_label = shell_quote("Dismiss"),
        timeout_args = timeout_args,
    ));
    if let Some(ref visibility_check_command) = visibility_check_command {
        script.push_str(&format!(
            "(\n  while kill -0 \"$notifier_pid\" 2>/dev/null; do\n    sleep 2\n    kill -0 \"$notifier_pid\" 2>/dev/null || exit 0\n    if {visibility_check} >/dev/null 2>&1 && {notifier} --remove {group} >/dev/null 2>&1; then\n      exit 0\n    fi\n  done\n) &\nmonitor_pid=$!\n",
            visibility_check = visibility_check_command,
            notifier = notifier_q,
            group = group_q,
        ));
    }
    script.push_str("wait \"$notifier_pid\"\n");
    if visibility_check_command.is_some() {
        script.push_str(
            "kill \"$monitor_pid\" 2>/dev/null\nwait \"$monitor_pid\" 2>/dev/null\nmonitor_pid=\n",
        );
    }
    script.push_str("notifier_status=$(cat \"$status_path\" 2>/dev/null || printf '1')\nresult=$(cat \"$result_path\")\nrm -f \"$result_path\" \"$status_path\"\n");

    script.push_str("if [ \"$notifier_status\" -ne 0 ]; then\n");
    script.push_str("    exit \"$notifier_status\"\n");
    script.push_str("fi\n");
    script.push_str("case \"$result\" in\n");
    script.push_str(&format!(
        "  Focus|@ACTIONCLICKED|@CONTENTCLICKED)\n{activate}    exec {herdr} agent focus {pane}\n    ;;\n",
        activate = activation_script(activate_command),
        herdr = herdr_q,
        pane = pane_q,
    ));
    script.push_str("esac\n");

    script
}

fn activation_script(activate_command: Option<&str>) -> String {
    let Some(command) = activate_command else {
        return String::new();
    };
    format!("    {command} >/dev/null 2>&1\n", command = command)
}

/// The command that brings the terminal/Herdr host app to the front before
/// focusing the agent pane: whichever terminal is bound to the pane's
/// workspace, learned from `pane.focused` events. Zero configuration, and it
/// follows the user across terminals per-workspace.
fn activation_command(workspace: &str) -> Option<String> {
    crate::state::remembered_terminal(workspace)
        .map(|bound| format!("open -b {}", shell_quote(&bound)))
}

#[cfg(unix)]
fn make_executable(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(path, permissions)
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_notification() -> FocusNotification {
        FocusNotification {
            pane_id: "w1:p3".to_string(),
            status: "blocked".to_string(),
            title: "Codex needs your input".to_string(),
            body: "Open the pane to review and respond.".to_string(),
            group: "herdr-w1-p3".to_string(),
            app_icon: Some("/tmp/codex icon.png".to_string()),
        }
    }


    #[test]
    fn alerter_script_invokes_alerter_and_runs_focus_on_click() {
        let script = focus_script_content_with_timeout(
            &sample_notification(),
            "/usr/local/bin/herdr",
            "/opt/homebrew/bin/alerter",
            ALERTER_TIMEOUT_SECS,
            None,
        );

        assert!(script.starts_with("#!/bin/sh\n"));
        assert!(script.contains("'/opt/homebrew/bin/alerter' --title 'Codex needs your input'"));
        assert!(script.contains("--message 'Open the pane to review and respond.'"));
        assert!(script.contains("--group 'herdr-w1-p3'"));
        assert!(script.contains("--app-icon '/tmp/codex icon.png'"));
        assert!(script.contains("--actions 'Focus'"));
        assert!(script.contains("--close-label 'Dismiss'"));
        assert!(script.contains(".cleared' ] && exit 0"));
        assert!(
            script.find(".cleared' ] && exit 0").unwrap()
                < script.find("'/opt/homebrew/bin/alerter' --title").unwrap()
        );
        assert!(script.contains("notifier_status=$(cat \"$status_path\""));
        assert!(script.contains("exit \"$notifier_status\""));
        assert!(script.contains("Focus|@ACTIONCLICKED|@CONTENTCLICKED)"));
        assert!(script.contains("exec '/usr/local/bin/herdr' agent focus 'w1:p3'"));
    }

    #[test]
    fn alerter_script_includes_timeout_when_configured() {
        let script = alerter_focus_script(
            &sample_notification(),
            "/usr/local/bin/herdr",
            "/opt/homebrew/bin/alerter",
            120,
            None,
            None,
        );

        assert!(script.contains("--timeout 120"));
    }

    #[test]
    fn alerter_script_omits_timeout_when_zero() {
        let script = alerter_focus_script(
            &sample_notification(),
            "/usr/local/bin/herdr",
            "/opt/homebrew/bin/alerter",
            0,
            None,
            None,
        );

        assert!(!script.contains("--timeout"));
    }

    #[test]
    fn test_mode_uses_a_short_timeout() {
        assert_eq!(test_timeout_secs(3600), 10);
        assert_eq!(test_timeout_secs(0), 10);
        assert_eq!(test_timeout_secs(5), 5);
    }


    #[test]
    fn alerter_script_includes_activation_when_configured() {
        let script = alerter_focus_script(
            &sample_notification(),
            "/usr/local/bin/herdr",
            "/opt/homebrew/bin/alerter",
            3600,
            Some("open -a 'kitty'"),
            None,
        );

        assert!(script.contains("open -a 'kitty' >/dev/null 2>&1"));
        assert!(script.contains("exec '/usr/local/bin/herdr' agent focus 'w1:p3'"));
    }

    #[test]
    fn alerter_script_monitors_visibility_after_starting_the_notifier() {
        let script = alerter_focus_script(
            &sample_notification(),
            "/usr/local/bin/herdr",
            "/opt/homebrew/bin/alerter",
            3600,
            Some("open -a 'kitty'"),
            Some(Path::new("/tmp/herdr-focus-notify")),
        );

        assert!(script.contains("notifier_pid=$!"));
        assert!(script.contains("while kill -0 \"$notifier_pid\" 2>/dev/null"));
        assert!(script.contains("kill -0 \"$notifier_pid\" 2>/dev/null || exit 0"));
        assert!(script.contains("'/tmp/herdr-focus-notify' --check-pane-visibility 'w1:p3'"));
        assert!(script.contains("'/opt/homebrew/bin/alerter' --remove 'herdr-w1-p3'"));
        assert!(
            script.find("notifier_pid=$!").unwrap()
                < script.find("while kill -0 \"$notifier_pid\"").unwrap()
        );
        assert!(script.contains("kill \"$monitor_pid\" 2>/dev/null"));
    }

}
