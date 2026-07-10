use std::process::Command;

#[cfg(unix)]
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(unix)]
use std::path::Path;
#[cfg(unix)]
use std::time::{SystemTime, UNIX_EPOCH};

fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_herdr-focus-notify"))
}

#[test]
fn help_and_version_print_to_stdout() {
    let help = binary().arg("--help").output().unwrap();
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("Usage:"));
    assert!(help.stderr.is_empty());

    let version = binary().arg("--version").output().unwrap();
    assert!(version.status.success());
    assert!(String::from_utf8_lossy(&version.stdout).contains(env!("CARGO_PKG_VERSION")));
    assert!(version.stderr.is_empty());
}

#[test]
fn no_event_is_quiet_even_when_notifier_config_is_bad() {
    let output = binary()
        .env("HERDR_FOCUS_NOTIFY_NOTIFIER", "/definitely/missing")
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}

#[test]
fn test_mode_reports_bad_notifier() {
    let output = binary()
        .arg("--test")
        .env("HERDR_FOCUS_NOTIFY_NOTIFIER", "/definitely/missing")
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("configured notifier"));
}

#[cfg(unix)]
#[test]
fn focus_event_removes_notification_for_foreground_terminal() {
    let temp_dir = std::env::temp_dir().join(format!(
        "herdr-focus-notify-test-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&temp_dir).unwrap();

    let osascript = temp_dir.join("osascript");
    write_executable(
        &osascript,
        "#!/bin/sh\nprintf '%s\\n' 'com.example.terminal'\n",
    );

    let notifier = temp_dir.join("alerter");
    write_executable(
        &notifier,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$NOTIFIER_LOG\"\n",
    );

    let notifier_log = temp_dir.join("notifier.log");
    let path = format!(
        "{}:{}",
        temp_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let output = binary()
        .env("HERDR_PLUGIN_EVENT", "pane.focused")
        .env(
            "HERDR_PLUGIN_EVENT_JSON",
            r#"{"event":"pane.focused","data":{"pane_id":"w1:p2"}}"#,
        )
        .env("HERDR_FOCUS_NOTIFY_ACTIVATE_APP", "Test Terminal")
        .env("HERDR_FOCUS_NOTIFY_NOTIFIER", &notifier)
        .env("HERDR_PLUGIN_STATE_DIR", temp_dir.join("state"))
        .env("NOTIFIER_LOG", &notifier_log)
        .env("PATH", path)
        .env_remove("HERDR_FOCUS_NOTIFY_ENABLED")
        .output()
        .unwrap();

    assert!(output.status.success());
    assert_eq!(
        fs::read_to_string(&notifier_log).unwrap(),
        "--remove\nherdr-w1-p2\n"
    );
    fs::remove_dir_all(temp_dir).unwrap();
}

#[cfg(unix)]
fn write_executable(path: &Path, content: &str) {
    fs::write(path, content).unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(path, permissions).unwrap();
}
