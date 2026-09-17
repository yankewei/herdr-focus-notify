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

/// Environment variable that overrides how long an unclicked notification
/// stays up.
const TIMEOUT_ENV_VAR: &str = "HERDR_FOCUS_NOTIFY_TIMEOUT_SECS";

/// How long an unclicked notification stays up (seconds) before alerter
/// auto-dismisses it; 0 would keep it forever.
///
/// `alerter` grows its resident memory for as long as it waits — measured at a
/// steady ~12.9 MB/min — so the timeout doubles as a leak bound. At one hour a
/// single unclicked notification reaches roughly 774 MB; five minutes caps it
/// near 65 MB. Agents that finish while nobody is at the keyboard would
/// otherwise stack one such waiter per pane.
const DEFAULT_ALERTER_TIMEOUT_SECS: u64 = 300;

/// Reads the notification timeout, falling back to the default when the
/// override is absent or unparseable.
fn alerter_timeout_secs() -> u64 {
    parse_timeout_secs(env::var(TIMEOUT_ENV_VAR).ok().as_deref())
}

fn parse_timeout_secs(raw: Option<&str>) -> u64 {
    raw.and_then(|raw| raw.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_ALERTER_TIMEOUT_SECS)
}

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

    let configured_timeout_secs = alerter_timeout_secs();
    let timeout_secs = if test_mode {
        test_timeout_secs(configured_timeout_secs)
    } else {
        configured_timeout_secs
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
    let workspace =
        crate::util::workspace_id_from_pane_id(&notification.pane_id).unwrap_or("default");
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

fn alerter_focus_script(
    notification: &FocusNotification,
    herdr_bin: &str,
    notifier_bin: &str,
    timeout_secs: u64,
    visibility_check_binary: Option<&Path>,
    focus_binary: &Path,
) -> String {
    let title_q = shell_quote(&notification.title);
    let body_q = shell_quote(&notification.body);
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
            DEFAULT_ALERTER_TIMEOUT_SECS,
            None,
            Path::new("/tmp/herdr-focus-notify"),
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
        assert!(script.contains("HERDR_BIN_PATH='/usr/local/bin/herdr' exec '/tmp/herdr-focus-notify' --focus-pane 'w1:p3'"));
    }

    #[test]
    fn timeout_defaults_when_override_is_absent_or_invalid() {
        assert_eq!(parse_timeout_secs(None), DEFAULT_ALERTER_TIMEOUT_SECS);
        assert_eq!(parse_timeout_secs(Some("")), DEFAULT_ALERTER_TIMEOUT_SECS);
        assert_eq!(
            parse_timeout_secs(Some("soon")),
            DEFAULT_ALERTER_TIMEOUT_SECS
        );
        assert_eq!(parse_timeout_secs(Some("-5")), DEFAULT_ALERTER_TIMEOUT_SECS);
    }

    #[test]
    fn timeout_override_is_parsed() {
        assert_eq!(parse_timeout_secs(Some("900")), 900);
        assert_eq!(parse_timeout_secs(Some("  900  ")), 900);
    }

    #[test]
    fn timeout_override_of_zero_keeps_the_notification_forever() {
        // 0 is a deliberate opt-out: `alerter_focus_script` omits `--timeout`.
        assert_eq!(parse_timeout_secs(Some("0")), 0);
    }

    #[test]
    fn default_timeout_bounds_the_alerter_memory_leak() {
        // `alerter` grows ~12.9 MB/min while it waits, so the default has to
        // stay short enough that one unclicked notification cannot balloon.
        let default = parse_timeout_secs(None);

        assert!(
            default > 0 && default <= 600,
            "default timeout {default}s lets a single waiter leak past ~130 MB"
        );
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
