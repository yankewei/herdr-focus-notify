use std::collections::hash_map::DefaultHasher;
use std::env;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io;
use std::path::{Path, PathBuf};

use crate::notification::FocusNotification;
use crate::state::{
    cleanup_stale_state_files, cleared_notification_marker_path, plugin_state_dir,
    prune_stale_workspace_bindings, remembered_terminal,
};
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
    let _ = rewrite_generated_scripts_without_activation();
    let _ = prune_stale_workspace_bindings(herdr_bin);

    let timeout_secs = if test_mode {
        test_timeout_secs(ALERTER_TIMEOUT_SECS)
    } else {
        ALERTER_TIMEOUT_SECS
    };

    let mut hasher = DefaultHasher::new();
    notification.pane_id.hash(&mut hasher);

    let script_path = state_dir.join(format!("focus-{:016x}.sh", hasher.finish()));
    let executable_path = env::current_exe()?;
    let script = focus_script_content_with_timeout(
        notification,
        herdr_bin,
        notifier_bin,
        timeout_secs,
        monitor_visibility.then_some(executable_path.as_path()),
        &executable_path,
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
    focus_binary: &Path,
) -> String {
    let workspace = crate::util::workspace_of(&notification.pane_id);
    // The visibility monitor needs a terminal it can match the frontmost app
    // against; with none learned, deliver a plain notification.
    let visibility_check_binary = if remembered_terminal(workspace).is_none() {
        None
    } else {
        executable_path
    };

    alerter_focus_script(
        notification,
        herdr_bin,
        notifier_bin,
        timeout_secs,
        visibility_check_binary,
        focus_binary,
    )
}

fn test_timeout_secs(configured: u64) -> u64 {
    if configured == 0 {
        10
    } else {
        configured.min(10)
    }
}

/// One of alerter's free-text arguments, in the `--option=value` form. Text
/// such as the pane's terminal title can start with `-` (`-zsh`,
/// `--resume ...`); passed as a separate value, alerter takes it for an option
/// and refuses to notify.
fn alerter_text_arg(option: &str, text: &str) -> String {
    format!(" --{option}={}", shell_quote(text))
}

fn alerter_focus_script(
    notification: &FocusNotification,
    herdr_bin: &str,
    notifier_bin: &str,
    timeout_secs: u64,
    visibility_check_binary: Option<&Path>,
    focus_binary: &Path,
) -> String {
    let title_arg = alerter_text_arg("title", &notification.title);
    let message_arg = alerter_text_arg("message", &notification.body);
    let group_q = shell_quote(&notification.group);
    let pane_q = shell_quote(&notification.pane_id);
    let herdr_q = shell_quote(herdr_bin);
    let focus_binary_q = shell_quote(&focus_binary.to_string_lossy());
    let notifier_q = shell_quote(notifier_bin);
    let cleared_marker = cleared_notification_marker_path(&notification.pane_id);
    let cleared_marker_q = shell_quote(cleared_marker.to_string_lossy().as_ref());
    let app_icon_args = notification
        .app_icon
        .as_ref()
        .map(|path| format!(" --app-icon {}", shell_quote(path)))
        .unwrap_or_default();
    let subtitle_arg = notification
        .subtitle
        .as_deref()
        .map(|subtitle| alerter_text_arg("subtitle", subtitle))
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
        "result_path=$(mktemp {result_template}) || exit 1\nstatus_path=$(mktemp {status_template}) || {{ rm -f \"$result_path\"; exit 1; }}\nmonitor_pid=\ncleanup() {{\n  [ -z \"$monitor_pid\" ] || kill \"$monitor_pid\" 2>/dev/null\n  rm -f \"$result_path\" \"$status_path\"\n}}\ntrap cleanup EXIT\n(\n  {notifier}{title_arg}{message_arg}{subtitle_arg} --group {group}{app_icon_args} --actions {action} --close-label {close_label}{timeout_args} > \"$result_path\" 2>/dev/null\n  printf '%s' \"$?\" > \"$status_path\"\n) &\nnotifier_pid=$!\n",
        result_template = result_template_q,
        status_template = status_template_q,
        notifier = notifier_q,
        title_arg = title_arg,
        message_arg = message_arg,
        subtitle_arg = subtitle_arg,
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
        "  Focus|@ACTIONCLICKED|@CONTENTCLICKED)\n    HERDR_BIN_PATH={herdr} exec {focus_binary} --focus-pane {pane}\n    ;;\n",
        herdr = herdr_q,
        focus_binary = focus_binary_q,
        pane = pane_q,
    ));
    script.push_str("esac\n");

    script
}

/// Removes the activation command captured by scripts generated before
/// terminal activation moved into `--focus-pane`. This lets the clear-bindings
/// action make already-visible notifications safe to click as well.
pub(crate) fn rewrite_generated_scripts_without_activation() -> io::Result<()> {
    let state_dir = plugin_state_dir();
    let entries = match fs::read_dir(&state_dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };

    for entry in entries {
        let Ok(entry) = entry else {
            continue;
        };
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.starts_with("focus-") || !name.ends_with(".sh") || !path.is_file() {
            continue;
        }

        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let rewritten = without_captured_activation(&content);
        if rewritten != content {
            let _ = fs::write(path, rewritten);
        }
    }

    Ok(())
}

fn without_captured_activation(content: &str) -> String {
    let mut rewritten = content
        .lines()
        .filter(|line| !line.trim_start().starts_with("open -b "))
        .collect::<Vec<_>>()
        .join("\n");
    if content.ends_with('\n') {
        rewritten.push('\n');
    }
    rewritten
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
            title: "Blocked · sample-repo · status".to_string(),
            body: "Open the pane to review and respond.".to_string(),
            subtitle: Some("Codex".to_string()),
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
            Path::new("/tmp/herdr-focus-notify"),
        );

        assert!(script.starts_with("#!/bin/sh\n"));
        assert!(
            script.contains("'/opt/homebrew/bin/alerter' --title='Blocked · sample-repo · status'")
        );
        assert!(script.contains("--message='Open the pane to review and respond.'"));
        assert!(script.contains("--subtitle='Codex'"));
        assert!(script.contains("--group 'herdr-w1-p3'"));
        assert!(script.contains("--app-icon '/tmp/codex icon.png'"));
        assert!(script.contains("--actions 'Focus'"));
        assert!(script.contains("--close-label 'Dismiss'"));
        assert!(script.contains(".cleared' ] && exit 0"));
        assert!(
            script.find(".cleared' ] && exit 0").unwrap()
                < script.find("'/opt/homebrew/bin/alerter' --title=").unwrap()
        );
        assert!(script.contains("notifier_status=$(cat \"$status_path\""));
        assert!(script.contains("exit \"$notifier_status\""));
        assert!(script.contains("Focus|@ACTIONCLICKED|@CONTENTCLICKED)"));
        assert!(script.contains("HERDR_BIN_PATH='/usr/local/bin/herdr' exec '/tmp/herdr-focus-notify' --focus-pane 'w1:p3'"));
    }

    #[test]
    fn alerter_script_includes_timeout_when_configured() {
        let script = alerter_focus_script(
            &sample_notification(),
            "/usr/local/bin/herdr",
            "/opt/homebrew/bin/alerter",
            120,
            None,
            Path::new("/tmp/herdr-focus-notify"),
        );

        assert!(script.contains("--timeout 120"));
    }

    #[test]
    fn alerter_script_keeps_a_dash_led_message_attached_to_its_option() {
        let mut notification = sample_notification();
        notification.body = "--resume the migration".to_string();

        let script = alerter_focus_script(
            &notification,
            "/usr/local/bin/herdr",
            "/opt/homebrew/bin/alerter",
            3600,
            None,
            Path::new("/tmp/herdr-focus-notify"),
        );

        assert!(script.contains("--message='--resume the migration'"));
    }

    #[test]
    fn alerter_script_omits_subtitle_when_there_is_none() {
        let mut notification = sample_notification();
        notification.subtitle = None;

        let script = alerter_focus_script(
            &notification,
            "/usr/local/bin/herdr",
            "/opt/homebrew/bin/alerter",
            3600,
            None,
            Path::new("/tmp/herdr-focus-notify"),
        );

        assert!(!script.contains("--subtitle"));
    }

    #[test]
    fn alerter_script_omits_timeout_when_zero() {
        let script = alerter_focus_script(
            &sample_notification(),
            "/usr/local/bin/herdr",
            "/opt/homebrew/bin/alerter",
            0,
            None,
            Path::new("/tmp/herdr-focus-notify"),
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
    fn alerter_script_defers_activation_to_focus_helper() {
        let script = alerter_focus_script(
            &sample_notification(),
            "/usr/local/bin/herdr",
            "/opt/homebrew/bin/alerter",
            3600,
            None,
            Path::new("/tmp/herdr-focus-notify"),
        );

        assert!(!script.contains("open -b "));
        assert!(script.contains("HERDR_BIN_PATH='/usr/local/bin/herdr' exec '/tmp/herdr-focus-notify' --focus-pane 'w1:p3'"));
    }

    #[test]
    fn alerter_script_monitors_visibility_after_starting_the_notifier() {
        let script = alerter_focus_script(
            &sample_notification(),
            "/usr/local/bin/herdr",
            "/opt/homebrew/bin/alerter",
            3600,
            Some(Path::new("/tmp/herdr-focus-notify")),
            Path::new("/tmp/herdr-focus-notify"),
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
    fn removes_old_captured_activation_commands() {
        let old = "#!/bin/sh\n    open -b 'com.google.Chrome' >/dev/null 2>&1\n    exec focus\n";
        assert_eq!(
            without_captured_activation(old),
            "#!/bin/sh\n    exec focus\n"
        );
    }
}
