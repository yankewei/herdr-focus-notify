use std::ffi::c_int;
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// How often a bounded command whose output has not ended is checked for
/// having failed.
const TIMEOUT_POLL: Duration = Duration::from_millis(20);

/// Runs a command and returns its stdout on success. None when the binary is
/// missing, the command fails, or the output is not valid UTF-8.
pub(crate) fn command_stdout(bin: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(bin).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

/// Like `command_stdout`, but gives up at `deadline` and reports None, and with
/// `env` added to the child's environment. A deadline rather than a timeout,
/// so that several calls can share one budget: a call that starts after the
/// deadline does not run at all.
///
/// `Command` has no timeout of its own, so the child is polled until the
/// deadline, and its stdout is drained on another thread, both because a
/// command that fills the pipe buffer before it exits would otherwise deadlock
/// and because the pipe can outlive the child: any descendant it started
/// inherits the write end, and the read only ends once every holder closes it.
/// So the deadline bounds the read as well as the exit, and the child runs in a
/// process group of its own that is killed as a whole, rather than leaving a
/// descendant holding the pipe after the child itself is gone.
pub(crate) fn command_stdout_until(
    bin: &str,
    args: &[&str],
    env: &[(&str, &str)],
    deadline: Instant,
) -> Option<String> {
    if Instant::now() >= deadline {
        return None;
    }

    let mut child = Command::new(bin)
        .args(args)
        .envs(env.iter().copied())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .ok()?;

    let mut pipe = child.stdout.take()?;
    let (sender, receiver) = mpsc::channel();
    // Detached on purpose: a descendant that escaped the process group can hold
    // the pipe past the deadline, and the reader is then abandoned rather than
    // waited for.
    std::thread::spawn(move || {
        let mut buffer = Vec::new();
        let read = pipe.read_to_end(&mut buffer);
        let _ = sender.send(read.map(|_| buffer));
    });

    // Waiting on the reader rather than sleeping: a command closes its stdout
    // as it exits, so a quick one is noticed at once instead of a poll later.
    let remaining = || deadline.saturating_duration_since(Instant::now());
    // Some once the reader has seen the end of the output.
    let mut read = None;
    let stdout = loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => {
                break read
                    .or_else(|| receiver.recv_timeout(remaining()).ok())
                    .and_then(Result::ok);
            }
            Ok(None) if Instant::now() < deadline => match read {
                // The output has ended, so the exit is a moment away.
                Some(_) => std::thread::sleep(Duration::from_millis(1)),
                None => match receiver.recv_timeout(TIMEOUT_POLL.min(remaining())) {
                    Ok(result) => read = Some(result),
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break None,
                },
            },
            // Failed, past the deadline, or the wait itself failed.
            _ => break None,
        }
    };

    // Whatever the outcome, nothing the child started may outlive the call:
    // a descendant left running would keep working in the pane's repository.
    kill_process_group(&mut child);
    String::from_utf8(stdout?).ok()
}

/// Kills the process group `child` leads, then reaps the child. Each step is
/// best-effort: the group is usually gone already, since a command that has
/// exited and closed its stdout normally leaves nothing behind.
fn kill_process_group(child: &mut Child) {
    extern "C" {
        fn kill(pid: c_int, signal: c_int) -> c_int;
    }
    const SIGKILL: c_int = 9;

    if let Ok(group) = c_int::try_from(child.id()) {
        // SAFETY: `kill` only sends a signal; a negative pid names the process
        // group the child leads, which `process_group(0)` created for it.
        unsafe {
            kill(-group, SIGKILL);
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

pub(crate) fn sanitize_group_id(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect()
}

pub(crate) fn notification_group_id(pane_id: &str) -> String {
    format!("herdr-{}", sanitize_group_id(pane_id))
}

/// The workspace part of a pane id, e.g. `w1:p3` -> `w1`.
///
/// Herdr pane ids are `workspace:pane`; the workspace is stable while panes
/// are created and destroyed inside it. Used to key per-workspace terminal
/// bindings.
pub(crate) fn workspace_id_from_pane_id(pane_id: &str) -> Option<&str> {
    pane_id.split(':').next().filter(|value| !value.is_empty())
}

pub(crate) fn shell_quote(value: &str) -> String {
    let mut quoted = String::from("'");
    for ch in value.chars() {
        if ch == '\'' {
            quoted.push_str("'\\''");
        } else {
            quoted.push(ch);
        }
    }
    quoted.push('\'');
    quoted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_command_returns_output_when_it_finishes_in_time() {
        let stdout = command_stdout_until(
            "sh",
            &["-c", "printf 'hello'"],
            &[],
            Instant::now() + Duration::from_secs(5),
        );

        assert_eq!(stdout.as_deref(), Some("hello"));
    }

    #[test]
    fn bounded_command_passes_its_environment_through() {
        let stdout = command_stdout_until(
            "sh",
            &["-c", "printf '%s' \"$LC_ALL\""],
            &[("LC_ALL", "C")],
            Instant::now() + Duration::from_secs(5),
        );

        assert_eq!(stdout.as_deref(), Some("C"));
    }

    #[test]
    fn bounded_command_gives_up_and_reports_none_when_it_overruns() {
        let started = Instant::now();
        let stdout = command_stdout_until(
            "sh",
            &["-c", "sleep 30"],
            &[],
            Instant::now() + Duration::from_millis(200),
        );

        assert_eq!(stdout, None);
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn bounded_command_kills_a_descendant_still_holding_its_stdout() {
        // The trailing `wait` keeps the shell from exec-ing into `sleep`, so the
        // shell is the child, `sleep` the descendant, and both hold the pipe.
        let temp_dir = TempDir::new("descendant");
        let pid_file = temp_dir.0.join("sleep.pid");
        let started = Instant::now();
        let stdout = command_stdout_until(
            "sh",
            &[
                "-c",
                "sleep 30 & echo $! > \"$0\"; wait",
                pid_file.to_str().unwrap(),
            ],
            &[],
            Instant::now() + Duration::from_millis(500),
        );

        assert_eq!(stdout, None);
        assert!(started.elapsed() < Duration::from_secs(5));
        assert_process_exits(&std::fs::read_to_string(&pid_file).unwrap());
    }

    #[test]
    fn bounded_command_stops_reading_at_the_deadline_after_the_child_exits() {
        // The shell exits at once, but the backgrounded `sleep` keeps the pipe
        // open: success alone must not wait for an EOF that never comes.
        let temp_dir = TempDir::new("background");
        let pid_file = temp_dir.0.join("sleep.pid");
        let started = Instant::now();
        let stdout = command_stdout_until(
            "sh",
            &[
                "-c",
                "sleep 30 & echo $! > \"$0\"; printf 'partial'",
                pid_file.to_str().unwrap(),
            ],
            &[],
            Instant::now() + Duration::from_millis(500),
        );

        assert_eq!(stdout, None);
        assert!(started.elapsed() < Duration::from_secs(5));
        assert_process_exits(&std::fs::read_to_string(&pid_file).unwrap());
    }

    #[test]
    fn bounded_command_does_not_start_once_the_deadline_has_passed() {
        // Calls sharing one budget: whatever comes after the budget is spent
        // must not run at all.
        let temp_dir = TempDir::new("expired");
        let marker = temp_dir.0.join("ran");
        let stdout = command_stdout_until(
            "sh",
            &["-c", "touch \"$0\"", marker.to_str().unwrap()],
            &[],
            Instant::now(),
        );

        assert_eq!(stdout, None);
        assert!(!marker.exists());
    }

    /// A scratch directory removed when the test ends, pass or fail.
    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "herdr-focus-notify-util-{name}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Waits for `pid` to disappear. A killed descendant is reaped by launchd
    /// once its parent is gone too, which takes a moment.
    fn assert_process_exits(pid: &str) {
        let pid = pid.trim();
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            let alive = Command::new("kill")
                .args(["-0", pid])
                .stderr(Stdio::null())
                .status()
                .unwrap()
                .success();
            if !alive {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("descendant {pid} outlived the bounded command");
    }

    #[test]
    fn extracts_workspace_from_pane_id() {
        assert_eq!(workspace_id_from_pane_id("w1:p3"), Some("w1"));
        assert_eq!(workspace_id_from_pane_id("w2:agent-42"), Some("w2"));
        assert_eq!(workspace_id_from_pane_id("no-colon"), Some("no-colon"));
        assert_eq!(workspace_id_from_pane_id(""), None);
        assert_eq!(workspace_id_from_pane_id(":p1"), None);
    }

    #[test]
    fn shell_quote_handles_apostrophes() {
        assert_eq!(shell_quote("/tmp/it's ok"), "'/tmp/it'\\''s ok'");
    }
}
