#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_herdr-focus-notify"))
}

#[test]
fn tab_focused_prefers_event_scoped_pane_context_over_live_lookup() {
    let temp_dir = temp_test_dir();

    let osascript = temp_dir.join("osascript");
    write_executable(
        &osascript,
        "#!/bin/sh\nprintf '%s\\n' 'com.example.terminal'\n",
    );

    // HERDR_BIN_PATH must resolve to an executable, but the normal
    // tab.focused path should not execute it when HERDR_PANE_ID is present.
    let herdr = temp_dir.join("herdr");
    let herdr_call_log = temp_dir.join("herdr-called");
    write_executable(
        &herdr,
        "#!/bin/sh\ntouch \"$HERDR_CALL_LOG\"\nexit 99\n",
    );

    let notifier = temp_dir.join("alerter");
    write_executable(
        &notifier,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$NOTIFIER_LOG\"\n",
    );

    let notifier_log = temp_dir.join("notifier.log");
    let state_dir = temp_dir.join("state");
    let output = binary()
        .env("HERDR_PLUGIN_EVENT", "tab.focused")
        .env(
            "HERDR_PLUGIN_EVENT_JSON",
            r#"{"event":"tab.focused","data":{"workspace_id":"w7","tab_id":"w7:t2"}}"#,
        )
        .env("HERDR_PANE_ID", " w7:p2 ")
        .env("HERDR_BIN_PATH", &herdr)
        .env("HERDR_PLUGIN_STATE_DIR", &state_dir)
        .env("HERDR_CALL_LOG", &herdr_call_log)
        .env("NOTIFIER_LOG", &notifier_log)
        .env("PATH", path_with_temp_dir(&temp_dir))
        .output()
        .unwrap();

    assert!(output.status.success());
    assert_eq!(
        fs::read_to_string(&notifier_log).unwrap(),
        "--remove\nherdr-w7-p2\n"
    );
    assert!(
        !herdr_call_log.exists(),
        "tab.focused should use the pane captured in HERDR_PANE_ID, not query live focus"
    );
    assert!(
        fs::read_to_string(state_dir.join("terminal-memory.json"))
            .unwrap()
            .contains("com.example.terminal")
    );

    fs::remove_dir_all(temp_dir).unwrap();
}

fn write_executable(path: &Path, content: &str) {
    fs::write(path, content).unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
}

fn temp_test_dir() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "herdr-focus-notify-tab-context-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn path_with_temp_dir(temp_dir: &Path) -> std::ffi::OsString {
    let mut paths = vec![temp_dir.to_path_buf()];
    if let Some(existing) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&existing));
    }
    std::env::join_paths(paths).unwrap()
}
