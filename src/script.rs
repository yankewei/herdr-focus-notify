use std::collections::hash_map::DefaultHasher;
use std::env;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io;
use std::path::{Path, PathBuf};

use crate::config::{activate_app, alerter_timeout_secs, is_debug_enabled};
use crate::notification::FocusNotification;
use crate::state::{cleared_notification_marker_path, plugin_state_dir};
use crate::util::shell_quote;

pub(crate) fn write_focus_script(
    notification: &FocusNotification,
    herdr_bin: &str,
    notifier_bin: &str,
) -> io::Result<PathBuf> {
    let state_dir = plugin_state_dir();
    fs::create_dir_all(&state_dir)?;

    let mut hasher = DefaultHasher::new();
    notification.pane_id.hash(&mut hasher);
    notification.status.hash(&mut hasher);
    notification.title.hash(&mut hasher);
    notification.body.hash(&mut hasher);
    notification.group.hash(&mut hasher);
    notification.app_icon.hash(&mut hasher);
    herdr_bin.hash(&mut hasher);
    notifier_bin.hash(&mut hasher);
    alerter_timeout_secs().hash(&mut hasher);
    activate_app().hash(&mut hasher);
    is_debug_enabled().hash(&mut hasher);

    let script_path = state_dir.join(format!("focus-{:016x}.sh", hasher.finish()));
    let debug_log_path = is_debug_enabled().then(|| state_dir.join("focus-click.log"));
    let executable_path = env::current_exe().ok();
    let script = focus_script_content(
        notification,
        herdr_bin,
        notifier_bin,
        executable_path.as_deref(),
        debug_log_path.as_deref(),
    );

    fs::write(&script_path, script)?;
    make_executable(&script_path)?;

    Ok(script_path)
}

fn focus_script_content(
    notification: &FocusNotification,
    herdr_bin: &str,
    notifier_bin: &str,
    executable_path: Option<&Path>,
    debug_log_path: Option<&Path>,
) -> String {
    alerter_focus_script(
        notification,
        herdr_bin,
        notifier_bin,
        alerter_timeout_secs(),
        activation_command().as_deref(),
        activate_app()
            .is_some()
            .then_some(executable_path)
            .flatten(),
        debug_log_path,
    )
}

fn alerter_focus_script(
    notification: &FocusNotification,
    herdr_bin: &str,
    notifier_bin: &str,
    timeout_secs: u64,
    activate_command: Option<&str>,
    visibility_check_binary: Option<&Path>,
    debug_log_path: Option<&Path>,
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

    match debug_log_path {
        Some(log_path) => {
            let log_q = shell_quote(log_path.to_string_lossy().as_ref());
            script.push_str(&format!(
                "printf '%s alerter status=%s result=%s\\n' \"$(date -u '+%Y-%m-%dT%H:%M:%SZ')\" \"$notifier_status\" \"$result\" >> {log} 2>&1\n",
                log = log_q,
            ));
            script.push_str("if [ \"$notifier_status\" -ne 0 ]; then\n");
            script.push_str("    exit \"$notifier_status\"\n");
            script.push_str("fi\n");
            script.push_str("status=0\n");
            script.push_str("case \"$result\" in\n");
            script.push_str(&format!(
                "  Focus|@ACTIONCLICKED|@CONTENTCLICKED)\n{activate}    {herdr} agent focus {pane} >> {log} 2>&1\n    status=$?\n    printf '%s focus exited %s\\n' \"$(date -u '+%Y-%m-%dT%H:%M:%SZ')\" \"$status\" >> {log} 2>&1\n    ;;\n",
                activate = activation_script(activate_command, Some(log_q.as_str())),
                herdr = herdr_q,
                pane = pane_q,
                log = log_q,
            ));
            script.push_str("esac\n");
            script.push_str("exit \"$status\"\n");
        }
        None => {
            script.push_str("if [ \"$notifier_status\" -ne 0 ]; then\n");
            script.push_str("    exit \"$notifier_status\"\n");
            script.push_str("fi\n");
            script.push_str("case \"$result\" in\n");
            script.push_str(&format!(
                "  Focus|@ACTIONCLICKED|@CONTENTCLICKED)\n{activate}    exec {herdr} agent focus {pane}\n    ;;\n",
                activate = activation_script(activate_command, None),
                herdr = herdr_q,
                pane = pane_q,
            ));
            script.push_str("esac\n");
        }
    }

    script
}

fn activation_script(activate_command: Option<&str>, log_q: Option<&str>) -> String {
    let Some(command) = activate_command else {
        return String::new();
    };

    match log_q {
        Some(log_q) => format!(
            "    {command} >> {log} 2>&1\n    activate_status=$?\n    printf '%s activate exited %s\\n' \"$(date -u '+%Y-%m-%dT%H:%M:%SZ')\" \"$activate_status\" >> {log} 2>&1\n",
            command = command,
            log = log_q,
        ),
        None => format!("    {command} >/dev/null 2>&1\n", command = command),
    }
}

fn activation_command() -> Option<String> {
    activate_app().map(activation_command_from)
}

fn activation_command_from(app: String) -> String {
    if app.contains('/') {
        format!("open {}", shell_quote(&app))
    } else {
        format!("open -a {}", shell_quote(&app))
    }
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
            title: "Codex needs attention".to_string(),
            body: "Needs an answer".to_string(),
            group: "herdr-w1-p3".to_string(),
            app_icon: Some("/tmp/codex icon.png".to_string()),
        }
    }

    #[test]
    fn focus_script_can_include_debug_click_log() {
        let notification = FocusNotification {
            pane_id: "pane ' one".to_string(),
            status: "blocked".to_string(),
            title: "x".to_string(),
            body: "y".to_string(),
            group: "g".to_string(),
            app_icon: None,
        };
        let script = focus_script_content(
            &notification,
            "/tmp/herdr bin",
            "/opt/homebrew/bin/alerter",
            None,
            Some(Path::new("/tmp/focus clicks.log")),
        );

        assert!(script.contains("alerter status=%s result=%s"));
        assert!(script.contains(">> '/tmp/focus clicks.log' 2>&1"));
        assert!(script.contains("'/tmp/herdr bin' agent focus 'pane '\\'' one'"));
        assert!(script.contains("focus exited %s"));
        assert!(script.contains("exit \"$status\""));
    }

    #[test]
    fn alerter_script_invokes_alerter_and_runs_focus_on_click() {
        let script = focus_script_content(
            &sample_notification(),
            "/usr/local/bin/herdr",
            "/opt/homebrew/bin/alerter",
            None,
            None,
        );

        assert!(script.starts_with("#!/bin/sh\n"));
        assert!(script.contains("'/opt/homebrew/bin/alerter' --title 'Codex needs attention'"));
        assert!(script.contains("--message 'Needs an answer'"));
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
            None,
        );

        assert!(!script.contains("--timeout"));
    }

    #[test]
    fn alerter_debug_script_logs_result() {
        let script = alerter_focus_script(
            &sample_notification(),
            "/usr/local/bin/herdr",
            "/opt/homebrew/bin/alerter",
            1800,
            None,
            None,
            Some(Path::new("/tmp/click.log")),
        );

        assert!(script.contains("alerter status=%s result=%s"));
        assert!(script.contains(">> '/tmp/click.log' 2>&1"));
        assert!(script.contains("notifier_status=$(cat \"$status_path\""));
        assert!(script.contains("status=0\n"));
        assert!(script.contains("focus exited %s"));
        assert!(script.contains("Focus|@ACTIONCLICKED|@CONTENTCLICKED)"));
        assert!(!script.contains("content click ignored"));
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
            None,
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

    #[test]
    fn activation_command_opens_app_names_and_paths() {
        assert_eq!(
            activation_command_from("kitty".to_string()),
            "open -a 'kitty'".to_string()
        );
        assert_eq!(
            activation_command_from("/Applications/kitty.app".to_string()),
            "open '/Applications/kitty.app'".to_string()
        );
    }
}
