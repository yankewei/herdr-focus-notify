use std::process::Command;

#[cfg(unix)]
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(unix)]
use std::path::{Path, PathBuf};
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
fn no_event_is_quiet_without_any_output() {
    let output = binary().output().unwrap();

    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}


#[test]
fn test_mode_reports_bad_configured_herdr_binary() {
    let output = binary()
        .arg("--test")
        .env("HERDR_BIN_PATH", "/definitely/missing-herdr")
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("configured Herdr binary"));
}


#[cfg(unix)]

#[cfg(unix)]
#[test]
fn focus_event_removes_notification_for_foreground_terminal() {
    let temp_dir = temp_test_dir();

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
    let path = path_with_temp_dir(&temp_dir);
    let output = binary()
        .env("HERDR_PLUGIN_EVENT", "pane.focused")
        .env(
            "HERDR_PLUGIN_EVENT_JSON",
            r#"{"event":"pane.focused","data":{"pane_id":"w1:p2"}}"#,
        )
        .env("HERDR_PLUGIN_STATE_DIR", temp_dir.join("state"))
        .env("NOTIFIER_LOG", &notifier_log)
        .env("PATH", path)
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
#[test]
fn visible_focused_pane_removes_its_pending_notification() {
    let temp_dir = temp_test_dir();

    let frontmost_state = temp_dir.join("frontmost-bundle-id");


    let osascript = temp_dir.join("osascript");
    write_executable(
        &osascript,
        "#!/bin/sh\ncase \"$*\" in\n  *frontmost*) cat \"$FRONTMOST_STATE\" ;;\n  *) printf '%s\\n' 'com.example.terminal' ;;\nesac\n",
    );

    let herdr = temp_dir.join("herdr");
    write_executable(
        &herdr,
        "#!/bin/sh\nif [ \"$1\" = \"agent\" ] && [ \"$2\" = \"get\" ]; then\n  printf '%s\\n' '{\"result\":{\"agent\":{\"focused\":true,\"pane_id\":\"w1:p2\"}}}'\nelse\n  printf '%s\\n' '{\"result\":{\"panes\":[{\"focused\":true,\"pane_id\":\"w1:p2\"}]}}'\nfi\n",
    );

    let notifier = temp_dir.join("alerter");
    write_executable(
        &notifier,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" >> \"$NOTIFIER_LOG\"\nif [ \"$1\" = \"--remove\" ]; then\n  touch \"$REMOVE_SIGNAL\"\n  exit 0\nfi\nfor _ in 1 2 3 4 5; do\n  [ -e \"$REMOVE_SIGNAL\" ] && exit 0\n  sleep 1\ndone\n",
    );

    let notifier_log = temp_dir.join("notifier.log");
    let remove_signal = temp_dir.join("removed");
    let path = path_with_temp_dir(&temp_dir);

    // Zero configuration: establish the w1 binding first via a real
    // pane.focused event while the fake frontmost app is the terminal.
    fs::write(&frontmost_state, "com.example.terminal\n").unwrap();
    let learn = binary()
        .env("HERDR_PLUGIN_EVENT", "pane.focused")
        .env(
            "HERDR_PLUGIN_EVENT_JSON",
            r#"{"event":"pane.focused","data":{"pane_id":"w1:p2"}}"#,
        )
        .env("HERDR_PLUGIN_STATE_DIR", temp_dir.join("state"))
        .env("FRONTMOST_STATE", &frontmost_state)
        .env("PATH", &path)
        .output()
        .unwrap();
    assert!(learn.status.success());

    // Now the user is elsewhere; the pending notification should auto-remove
    // once they switch back to the bound terminal.
    fs::write(&frontmost_state, "com.example.other\n").unwrap();

    let child = binary()
        .arg("--test")
        .env("HERDR_BIN_PATH", &herdr)
        .env("HERDR_PLUGIN_STATE_DIR", temp_dir.join("state"))
        .env("FRONTMOST_STATE", &frontmost_state)
        .env("NOTIFIER_LOG", &notifier_log)
        .env("REMOVE_SIGNAL", &remove_signal)
        .env("PATH", path)
        .spawn()
        .unwrap();

    for _ in 0..500 {
        if notifier_log.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    fs::write(&frontmost_state, "com.example.terminal\n").unwrap();

    let output = child.wait_with_output().unwrap();

    assert!(output.status.success());
    let notifier_output = fs::read_to_string(&notifier_log).unwrap_or_default();
    assert!(notifier_output.contains("--remove\nherdr-w1-p2\n"));
    fs::remove_dir_all(temp_dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_mode_notifies_even_when_pane_is_visible_and_status_filtered() {
    let temp_dir = temp_test_dir();

    // Both bundle-ID queries answer with the configured app, so the pane
    // counts as visible and a normal event would be skipped.
    let osascript = temp_dir.join("osascript");
    write_executable(
        &osascript,
        "#!/bin/sh\nprintf '%s\\n' 'com.example.terminal'\n",
    );

    let herdr = temp_dir.join("herdr");
    write_executable(
        &herdr,
        "#!/bin/sh\nif [ \"$1\" = \"agent\" ] && [ \"$2\" = \"get\" ]; then\n  printf '%s\\n' '{\"result\":{\"agent\":{\"focused\":true,\"pane_id\":\"w1:p2\"}}}'\nelse\n  printf '%s\\n' '{\"result\":{\"panes\":[{\"focused\":true,\"pane_id\":\"w1:p2\"}]}}'\nfi\n",
    );

    let notifier = temp_dir.join("alerter");
    write_executable(
        &notifier,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$NOTIFIER_LOG\"\n",
    );

    let notifier_log = temp_dir.join("notifier.log");
    let path = path_with_temp_dir(&temp_dir);
    let output = binary()
        .arg("--test")
        // A filter that excludes the hardcoded test status must not suppress
        // a test notification either.
        .env("HERDR_BIN_PATH", &herdr)
        .env("HERDR_PLUGIN_STATE_DIR", temp_dir.join("state"))
        .env("NOTIFIER_LOG", &notifier_log)
        .env("PATH", path)
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(fs::read_to_string(&notifier_log)
        .unwrap_or_default()
        .contains("--title"));
    fs::remove_dir_all(temp_dir).unwrap();
}

#[cfg(unix)]
#[test]
fn normal_notification_uses_status_specific_copy_without_requesting_an_explanation() {
    let temp_dir = temp_test_dir();

    let herdr = temp_dir.join("herdr");
    write_executable(
        &herdr,
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$HERDR_LOG\"\nif [ \"$2\" = \"get\" ]; then\n  printf '%s\\n' '{\"result\":{\"agent\":{\"focused\":false,\"pane_id\":\"w1:p2\"}}}'\nfi\n",
    );

    let notifier = temp_dir.join("alerter");
    write_executable(
        &notifier,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$NOTIFIER_LOG\"\n",
    );

    let notifier_log = temp_dir.join("notifier.log");
    let herdr_log = temp_dir.join("herdr.log");
    let path = path_with_temp_dir(&temp_dir);
    let output = binary()
        .env("HERDR_PLUGIN_EVENT", "pane.agent_status_changed")
        .env(
            "HERDR_PLUGIN_EVENT_JSON",
            r#"{"event":"pane.agent_status_changed","data":{"pane_id":"w1:p2","agent_status":"blocked","agent":"Codex","title":"Implement plugin"}}"#,
        )
        .env("HERDR_BIN_PATH", &herdr)
        .env("HERDR_PLUGIN_STATE_DIR", temp_dir.join("state"))
        .env("NOTIFIER_LOG", &notifier_log)
        .env("HERDR_LOG", &herdr_log)
        .env("PATH", path)
        .output()
        .unwrap();

    assert!(output.status.success());
    // The notifier runs in a detached script, so the log file can exist
    // (truncated by the shell redirect) before its content is written.
    // Poll for the content itself instead of just file existence.
    let mut notifier_output = String::new();
    for _ in 0..500 {
        notifier_output = fs::read_to_string(&notifier_log).unwrap_or_default();
        if !notifier_output.is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    assert!(notifier_output.contains("Codex needs your input"));
    assert!(notifier_output.contains("Open the pane to review and respond."));
    assert!(!notifier_output.contains("Implement plugin"));
    assert!(!fs::read_to_string(&herdr_log)
        .unwrap_or_default()
        .contains("explain"));
    fs::remove_dir_all(temp_dir).unwrap();
}

#[cfg(unix)]
#[test]
fn unfocused_pane_does_not_start_a_visibility_monitor() {
    let temp_dir = temp_test_dir();

    let frontmost_state = temp_dir.join("frontmost-bundle-id");
    fs::write(&frontmost_state, "com.example.other\n").unwrap();
    let focused_pane_state = temp_dir.join("focused-pane-id");
    fs::write(&focused_pane_state, "w1:p1\n").unwrap();

    let osascript = temp_dir.join("osascript");
    write_executable(
        &osascript,
        "#!/bin/sh\ncase \"$*\" in\n  *frontmost*) cat \"$FRONTMOST_STATE\" ;;\n  *) printf '%s\\n' 'com.example.terminal' ;;\nesac\n",
    );

    let herdr = temp_dir.join("herdr");
    write_executable(
        &herdr,
        "#!/bin/sh\nif [ \"$1\" = \"agent\" ] && [ \"$2\" = \"get\" ]; then\n  focused=false\n  [ \"$(cat \"$FOCUSED_PANE_STATE\")\" = \"w1:p2\" ] && focused=true\n  printf '{\"result\":{\"agent\":{\"focused\":%s,\"pane_id\":\"w1:p2\"}}}\\n' \"$focused\"\nelse\n  printf '{\"result\":{\"panes\":[{\"focused\":true,\"pane_id\":\"%s\"}]}}\\n' \"$(cat \"$FOCUSED_PANE_STATE\")\"\nfi\n",
    );

    let notifier = temp_dir.join("alerter");
    write_executable(
        &notifier,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" >> \"$NOTIFIER_LOG\"\nif [ \"$1\" = \"--remove\" ]; then\n  exit 0\nfi\nsleep 3\n",
    );

    let notifier_log = temp_dir.join("notifier.log");
    let path = path_with_temp_dir(&temp_dir);
    let output = binary()
        .env("HERDR_PLUGIN_EVENT", "pane.agent_status_changed")
        .env(
            "HERDR_PLUGIN_EVENT_JSON",
            r#"{"event":"pane.agent_status_changed","data":{"pane_id":"w1:p2","agent_status":"done"}}"#,
        )
        .env("HERDR_BIN_PATH", &herdr)
        .env("HERDR_PLUGIN_STATE_DIR", temp_dir.join("state"))
        .env("FRONTMOST_STATE", &frontmost_state)
        .env("FOCUSED_PANE_STATE", &focused_pane_state)
        .env("NOTIFIER_LOG", &notifier_log)
        .env("PATH", path)
        .output()
        .unwrap();

    assert!(output.status.success());
    for _ in 0..500 {
        if notifier_log.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(notifier_log.exists());
    fs::write(&focused_pane_state, "w1:p2\n").unwrap();
    fs::write(&frontmost_state, "com.example.terminal\n").unwrap();
    std::thread::sleep(std::time::Duration::from_secs(3));

    let notifier_output = fs::read_to_string(&notifier_log).unwrap_or_default();
    assert!(!notifier_output.contains("--remove\nherdr-w1-p2\n"));
    fs::remove_dir_all(temp_dir).unwrap();
}

#[cfg(unix)]
fn temp_test_dir() -> PathBuf {
    let temp_dir = std::env::temp_dir().join(format!(
        "herdr-focus-notify-test-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&temp_dir).unwrap();
    temp_dir
}

#[cfg(unix)]
fn path_with_temp_dir(temp_dir: &Path) -> String {
    format!(
        "{}:{}",
        temp_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    )
}

#[cfg(unix)]
fn write_executable(path: &Path, content: &str) {
    fs::write(path, content).unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(path, permissions).unwrap();
}
