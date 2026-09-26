use std::process::Command;

#[cfg(unix)]
use std::fs;
#[cfg(unix)]
use std::io::{BufRead, BufReader, Write};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(unix)]
use std::os::unix::net::UnixListener;
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

    write_fixed_frontmost(&temp_dir, "com.example.terminal");

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
fn focus_event_learns_terminal_from_cfbundleidentifier() {
    let temp_dir = temp_test_dir();
    let state_dir = temp_dir.join("state");

    write_executable(
        &temp_dir.join("lsappinfo"),
        "#!/bin/sh\ncase \"$1\" in\n  front) printf '%s\\n' 'ASN:0x0-0x1:' ;;\n  *) printf '%s\\n' '\"CFBundleIdentifier\"=\"com.mitchellh.ghostty\"' ;;\nesac\n",
    );
    write_executable(&temp_dir.join("alerter"), "#!/bin/sh\nexit 0\n");

    let output = binary()
        .env("HERDR_PLUGIN_EVENT", "pane.focused")
        .env(
            "HERDR_PLUGIN_EVENT_JSON",
            r#"{"event":"pane.focused","data":{"pane_id":"w1:p2"}}"#,
        )
        .env("HERDR_PLUGIN_STATE_DIR", &state_dir)
        .env("PATH", path_with_temp_dir(&temp_dir))
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(state_dir.join("terminal-memory.json")).unwrap(),
        r#"{"workspaces":{"w1":"com.mitchellh.ghostty"}}"#
    );
    fs::remove_dir_all(temp_dir).unwrap();
}

#[cfg(unix)]
#[test]
fn visible_focused_pane_removes_its_pending_notification() {
    let temp_dir = temp_test_dir();

    let frontmost_state = temp_dir.join("frontmost-bundle-id");

    write_state_frontmost(&temp_dir);

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

    // The frontmost app is the configured terminal, so the pane counts as
    // visible and a normal event would be skipped.
    write_fixed_frontmost(&temp_dir, "com.example.terminal");

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
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$NOTIFIER_LOG.tmp\"\nmv \"$NOTIFIER_LOG.tmp\" \"$NOTIFIER_LOG\"\n",
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
    // The notifier runs in a detached script. The fake notifier renames its
    // log into place, so the file appears only once all arguments are written.
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
    wait_for_detached_notifier(&temp_dir.join("state"));
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

    write_state_frontmost(&temp_dir);

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
    wait_for_detached_notifier(&temp_dir.join("state"));
    fs::remove_dir_all(temp_dir).unwrap();
}

#[cfg(unix)]
fn temp_test_dir() -> PathBuf {
    static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let temp_dir = std::env::temp_dir().join(format!(
        "herdr-focus-notify-test-{}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    fs::create_dir(&temp_dir).unwrap();
    temp_dir
}

/// Waits for a detached focus script to remove its notifier temp files. The
/// script writes the notifier status after the fake notifier has logged, so
/// removing the state directory earlier races with that write and fails with
/// `DirectoryNotEmpty`.
#[cfg(unix)]
fn wait_for_detached_notifier(state_dir: &Path) {
    let pending = || {
        fs::read_dir(state_dir).is_ok_and(|entries| {
            entries.flatten().any(|entry| {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                name.contains(".result.") || name.contains(".status.")
            })
        })
    };
    for _ in 0..500 {
        if !pending() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("detached notifier script did not finish");
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

/// Fakes the frontmost-app lookup with a fixed bundle id. Tests cannot drive
/// the real window server, so the shim answers both `lsappinfo` calls the
/// plugin makes: `front` (an ASN) and `info -only bundleID <asn>`.
#[cfg(unix)]
fn write_fixed_frontmost(temp_dir: &Path, bundle_id: &str) {
    write_executable(
        &temp_dir.join("lsappinfo"),
        &format!(
            "#!/bin/sh\ncase \"$1\" in\n  front) printf '%s\\n' 'ASN:0x0-0x1:' ;;\n  *) printf 'bundleID=\"{bundle_id}\"\\n' ;;\nesac\n"
        ),
    );
}

/// Fakes the frontmost-app lookup so it answers from `$FRONTMOST_STATE`,
/// letting a test change the frontmost app while the binary is running.
#[cfg(unix)]
fn write_state_frontmost(temp_dir: &Path) {
    write_executable(
        &temp_dir.join("lsappinfo"),
        "#!/bin/sh\ncase \"$1\" in\n  front) printf '%s\\n' 'ASN:0x0-0x1:' ;;\n  *) printf 'bundleID=\"%s\"\\n' \"$(cat \"$FRONTMOST_STATE\")\" ;;\nesac\n",
    );
}

/// A short `/tmp` socket path. The bound path must stay under `sun_path`'s
/// ~104-byte limit, so it cannot live in the long per-test temp directory.
#[cfg(unix)]
fn unique_socket_path() -> PathBuf {
    static NEXT_SOCKET_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    PathBuf::from(format!(
        "/tmp/herdr-focus-notify-{}-{}.sock",
        std::process::id(),
        NEXT_SOCKET_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ))
}

#[cfg(unix)]
fn focus_socket(expected_pane_id: &str, response: &str) -> (PathBuf, std::thread::JoinHandle<()>) {
    let socket_path = unique_socket_path();
    let listener = UnixListener::bind(&socket_path).unwrap();
    listener.set_nonblocking(true).unwrap();
    let cleanup_path = socket_path.clone();
    let expected_pane_id = expected_pane_id.to_string();
    let response = response.to_string();
    let handle = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let (mut stream, _) = loop {
            match listener.accept() {
                Ok(connection) => break connection,
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "plugin did not connect to the Herdr socket"
                    );
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(err) => panic!("failed to accept Herdr socket connection: {err}"),
            }
        };
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();

        let mut request_line = String::new();
        BufReader::new(stream.try_clone().unwrap())
            .read_line(&mut request_line)
            .unwrap();
        let request: serde_json::Value = serde_json::from_str(&request_line).unwrap();
        assert_eq!(request["id"], "herdr-focus-notify:focus");
        assert_eq!(request["method"], "pane.focus");
        assert_eq!(request["params"]["pane_id"], expected_pane_id);

        writeln!(stream, "{response}").unwrap();
        fs::remove_file(cleanup_path).unwrap();
    });
    (socket_path, handle)
}

#[cfg(unix)]
#[test]
fn focus_click_targets_an_arbitrary_pane_through_the_socket_api() {
    let temp_dir = temp_test_dir();
    let open_log = temp_dir.join("open.log");
    write_terminal_binding(&temp_dir.join("state"), "w2", "com.example.terminal");
    write_executable(
        &temp_dir.join("open"),
        "#!/bin/sh\nprintf '%s\\n' \"$*\" > \"$OPEN_LOG\"\n",
    );
    let (socket_path, socket_handle) = focus_socket(
        "w2:p7",
        r#"{"id":"herdr-focus-notify:focus","result":{"type":"pane_info","pane":{"pane_id":"w2:p7","workspace_id":"w2"}}}"#,
    );
    let output = binary()
        .args(["--focus-pane", "w2:p7"])
        .env("HERDR_SOCKET_PATH", &socket_path)
        .env("HERDR_PLUGIN_STATE_DIR", temp_dir.join("state"))
        .env("OPEN_LOG", &open_log)
        .env("PATH", path_with_temp_dir(&temp_dir))
        .output()
        .unwrap();
    socket_handle.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(open_log).unwrap(),
        "-b com.example.terminal\n"
    );
    fs::remove_dir_all(temp_dir).unwrap();
}

#[cfg(unix)]
#[test]
fn focus_click_reports_a_missing_plugin_socket() {
    let temp_dir = temp_test_dir();
    write_terminal_binding(&temp_dir.join("state"), "w2", "com.example.terminal");
    write_executable(&temp_dir.join("open"), "#!/bin/sh\nexit 0\n");

    let output = binary()
        .args(["--focus-pane", "w2:p7"])
        .env_remove("HERDR_SOCKET_PATH")
        .env("HERDR_PLUGIN_STATE_DIR", temp_dir.join("state"))
        .env("PATH", path_with_temp_dir(&temp_dir))
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("HERDR_SOCKET_PATH is unavailable"));
    assert!(!temp_dir.join("state/focus-origin-w2.marker").exists());
    fs::remove_dir_all(temp_dir).unwrap();
}

#[cfg(unix)]
#[test]
fn focus_click_reports_socket_api_failures() {
    let temp_dir = temp_test_dir();
    write_terminal_binding(&temp_dir.join("state"), "w2", "com.example.terminal");
    write_executable(&temp_dir.join("open"), "#!/bin/sh\nexit 0\n");
    for (response, expected) in [
        (
            r#"{"id":"herdr-focus-notify:focus","error":{"code":"not_found","message":"pane not found"}}"#,
            "failed to focus pane: pane not found",
        ),
        ("not json", "invalid pane focus response"),
        (
            r#"{"id":"herdr-focus-notify:focus","result":{"type":"ok"}}"#,
            "missing pane_info result",
        ),
    ] {
        let (socket_path, socket_handle) = focus_socket("w2:p7", response);
        let output = binary()
            .args(["--focus-pane", "w2:p7"])
            .env("HERDR_SOCKET_PATH", &socket_path)
            .env("HERDR_PLUGIN_STATE_DIR", temp_dir.join("state"))
            .env("PATH", path_with_temp_dir(&temp_dir))
            .output()
            .unwrap();
        socket_handle.join().unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains(expected));
        assert!(!temp_dir.join("state/focus-origin-w2.marker").exists());
    }
    fs::remove_dir_all(temp_dir).unwrap();
}

#[cfg(unix)]
#[test]
fn focus_click_reports_a_socket_that_never_answers() {
    let temp_dir = temp_test_dir();
    write_terminal_binding(&temp_dir.join("state"), "w2", "com.example.terminal");
    write_executable(&temp_dir.join("open"), "#!/bin/sh\nexit 0\n");

    let socket_path = unique_socket_path();
    let listener = UnixListener::bind(&socket_path).unwrap();
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop_in_thread = stop.clone();
    let handle = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        // Hold the connection open without answering: the client has to give up
        // on its own read timeout, not on this side closing the socket. The
        // bounded hold turns a missing timeout into a failed assertion instead
        // of a test that hangs forever.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        while !stop_in_thread.load(std::sync::atomic::Ordering::Relaxed)
            && std::time::Instant::now() < deadline
        {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        drop(stream);
    });

    let started = std::time::Instant::now();
    let output = binary()
        .args(["--focus-pane", "w2:p7"])
        .env("HERDR_SOCKET_PATH", &socket_path)
        .env("HERDR_PLUGIN_STATE_DIR", temp_dir.join("state"))
        .env("PATH", path_with_temp_dir(&temp_dir))
        .output()
        .unwrap();
    let elapsed = started.elapsed();

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("timed out after 5s"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!temp_dir.join("state/focus-origin-w2.marker").exists());
    assert!(
        elapsed < std::time::Duration::from_secs(8),
        "client waited {elapsed:?}"
    );

    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    handle.join().unwrap();
    fs::remove_file(&socket_path).unwrap();
    fs::remove_dir_all(temp_dir).unwrap();
}

#[cfg(unix)]
#[test]
fn notification_content_click_runs_the_focus_helper() {
    let temp_dir = temp_test_dir();
    let herdr = temp_dir.join("herdr");
    let notifier_log = temp_dir.join("notifier.log");
    let open_log = temp_dir.join("open.log");
    write_terminal_binding(&temp_dir.join("state"), "w2", "com.example.terminal");
    write_executable(
        &herdr,
        r#"#!/bin/sh
case "$1 $2" in
  'pane list') echo '{"result":{"panes":[{"focused":true,"pane_id":"w2:p7"}]}}' ;;
  'agent get') echo '{"result":{"agent":{"focused":false,"pane_id":"w2:p7"}}}' ;;
  *) exit 1 ;;
esac
"#,
    );
    write_executable(
        &temp_dir.join("alerter"),
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$NOTIFIER_LOG\"\necho '@CONTENTCLICKED'\n",
    );
    write_executable(
        &temp_dir.join("open"),
        "#!/bin/sh\nprintf '%s\\n' \"$*\" > \"$OPEN_LOG\"\n",
    );
    write_executable(&temp_dir.join("lsappinfo"), "#!/bin/sh\nexit 1\n");
    let (socket_path, socket_handle) = focus_socket(
        "w2:p7",
        r#"{"id":"herdr-focus-notify:focus","result":{"type":"pane_info","pane":{"pane_id":"w2:p7","workspace_id":"w2"}}}"#,
    );
    let output = binary()
        .arg("--test")
        .env("HERDR_BIN_PATH", &herdr)
        .env("HERDR_SOCKET_PATH", &socket_path)
        .env("HERDR_PLUGIN_STATE_DIR", temp_dir.join("state"))
        .env("NOTIFIER_LOG", &notifier_log)
        .env("OPEN_LOG", &open_log)
        .env("PATH", path_with_temp_dir(&temp_dir))
        .output()
        .unwrap();
    socket_handle.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(open_log).unwrap(),
        "-b com.example.terminal\n"
    );
    let notifier_args = fs::read_to_string(notifier_log).unwrap();
    assert!(notifier_args.contains("Focus notification test\n"));
    assert!(notifier_args.contains("Click to return to this Herdr pane.\n"));
    fs::remove_dir_all(temp_dir).unwrap();
}

#[cfg(unix)]
#[test]
fn focus_click_without_terminal_binding_does_not_activate_or_focus() {
    let temp_dir = temp_test_dir();
    let herdr_log = temp_dir.join("herdr.log");
    let open_log = temp_dir.join("open.log");
    write_executable(
        &temp_dir.join("herdr"),
        "#!/bin/sh\nprintf '%s\\n' \"$*\" > \"$HERDR_LOG\"\nexit 1\n",
    );
    write_executable(
        &temp_dir.join("open"),
        "#!/bin/sh\nprintf '%s\\n' \"$*\" > \"$OPEN_LOG\"\nexit 1\n",
    );

    let output = binary()
        .args(["--focus-pane", "w2:p7"])
        .env("HERDR_BIN_PATH", temp_dir.join("missing-herdr"))
        .env("HERDR_PLUGIN_STATE_DIR", temp_dir.join("state"))
        .env("HERDR_LOG", &herdr_log)
        .env("OPEN_LOG", &open_log)
        .env("PATH", path_with_temp_dir(&temp_dir))
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(!herdr_log.exists());
    assert!(!open_log.exists());
    fs::remove_dir_all(temp_dir).unwrap();
}

#[cfg(unix)]
#[test]
fn notification_focus_does_not_overwrite_existing_terminal_binding() {
    let temp_dir = temp_test_dir();
    let state_dir = temp_dir.join("state");
    let frontmost_state = temp_dir.join("frontmost");
    write_terminal_binding(&state_dir, "w1", "com.mitchellh.ghostty");
    fs::write(&frontmost_state, "com.example.notification-app\n").unwrap();
    write_state_frontmost(&temp_dir);
    write_executable(&temp_dir.join("open"), "#!/bin/sh\nexit 0\n");
    let (socket_path, socket_handle) = focus_socket(
        "w1:p2",
        r#"{"id":"herdr-focus-notify:focus","result":{"type":"pane_info","pane":{"pane_id":"w1:p2","workspace_id":"w1"}}}"#,
    );

    let focus = binary()
        .args(["--focus-pane", "w1:p2"])
        .env("HERDR_SOCKET_PATH", &socket_path)
        .env("HERDR_PLUGIN_STATE_DIR", &state_dir)
        .env("FRONTMOST_STATE", &frontmost_state)
        .env("PATH", path_with_temp_dir(&temp_dir))
        .output()
        .unwrap();
    socket_handle.join().unwrap();
    assert!(
        focus.status.success(),
        "{}",
        String::from_utf8_lossy(&focus.stderr)
    );

    let event = binary()
        .env("HERDR_PLUGIN_STATE_DIR", &state_dir)
        .env("HERDR_PLUGIN_EVENT", "pane.focused")
        .env(
            "HERDR_PLUGIN_EVENT_JSON",
            r#"{"event":"pane.focused","data":{"pane_id":"w1:p2"}}"#,
        )
        .env("FRONTMOST_STATE", &frontmost_state)
        .env("PATH", path_with_temp_dir(&temp_dir))
        .output()
        .unwrap();
    assert!(
        event.status.success(),
        "{}",
        String::from_utf8_lossy(&event.stderr)
    );

    assert_eq!(
        fs::read_to_string(state_dir.join("terminal-memory.json")).unwrap(),
        r#"{"workspaces":{"w1":"com.mitchellh.ghostty"}}"#
    );
    fs::remove_dir_all(temp_dir).unwrap();
}

#[cfg(unix)]
#[test]
fn obvious_non_terminal_does_not_overwrite_existing_terminal_binding() {
    let temp_dir = temp_test_dir();
    let state_dir = temp_dir.join("state");
    let frontmost_state = temp_dir.join("frontmost");
    let herdr = temp_dir.join("herdr");
    write_terminal_binding(&state_dir, "w1", "com.mitchellh.ghostty");
    fs::write(&frontmost_state, "com.google.Chrome\n").unwrap();
    write_state_frontmost(&temp_dir);
    write_executable(&herdr, "#!/bin/sh\nexit 0\n");

    let output = binary()
        .env("HERDR_BIN_PATH", &herdr)
        .env("HERDR_PLUGIN_STATE_DIR", &state_dir)
        .env("HERDR_PLUGIN_EVENT", "pane.focused")
        .env(
            "HERDR_PLUGIN_EVENT_JSON",
            r#"{"event":"pane.focused","data":{"pane_id":"w1:p2"}}"#,
        )
        .env("FRONTMOST_STATE", &frontmost_state)
        .env("PATH", path_with_temp_dir(&temp_dir))
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(state_dir.join("terminal-memory.json")).unwrap(),
        r#"{"workspaces":{"w1":"com.mitchellh.ghostty"}}"#
    );
    fs::remove_dir_all(temp_dir).unwrap();
}

#[cfg(unix)]
#[test]
fn clear_terminal_bindings_removes_old_activation_commands() {
    let temp_dir = temp_test_dir();
    let state_dir = temp_dir.join("state");
    write_terminal_binding(&state_dir, "w1", "com.mitchellh.ghostty");
    let script = state_dir.join("focus-old.sh");
    fs::write(
        &script,
        "#!/bin/sh\n    open -b 'com.google.Chrome' >/dev/null 2>&1\n    exec focus\n",
    )
    .unwrap();

    let output = binary()
        .arg("--clear-terminal-bindings")
        .env("HERDR_PLUGIN_STATE_DIR", &state_dir)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!state_dir.join("terminal-memory.json").exists());
    assert!(!fs::read_to_string(script).unwrap().contains("open -b"));
    fs::remove_dir_all(temp_dir).unwrap();
}

#[cfg(unix)]
fn write_terminal_binding(state_dir: &Path, workspace: &str, bundle_id: &str) {
    fs::create_dir_all(state_dir).unwrap();
    fs::write(
        state_dir.join("terminal-memory.json"),
        format!(r#"{{"workspaces":{{"{workspace}":"{bundle_id}"}}}}"#),
    )
    .unwrap();
}
