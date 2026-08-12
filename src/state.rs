use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::util::notification_group_id;

const GENERATED_SCRIPT_RETENTION: Duration = Duration::from_secs(30 * 24 * 60 * 60);
const TEMP_FILE_RETENTION: Duration = Duration::from_secs(24 * 60 * 60);

pub(crate) fn plugin_state_dir() -> PathBuf {
    env::var_os("HERDR_PLUGIN_STATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| env::temp_dir().join("herdr-focus-notify"))
}

pub(crate) fn mark_notification_cleared(pane_id: &str) -> io::Result<()> {
    let state_dir = plugin_state_dir();
    fs::create_dir_all(&state_dir)?;
    fs::write(cleared_notification_marker_path(pane_id), [])
}

pub(crate) fn reset_notification_clearance(pane_id: &str) -> io::Result<()> {
    match fs::remove_file(cleared_notification_marker_path(pane_id)) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

pub(crate) fn cleared_notification_marker_path(pane_id: &str) -> PathBuf {
    plugin_state_dir().join(format!("{}.cleared", notification_group_id(pane_id)))
}

pub(crate) fn cleanup_stale_state_files() -> io::Result<()> {
    cleanup_stale_state_files_in(
        &plugin_state_dir(),
        SystemTime::now(),
        GENERATED_SCRIPT_RETENTION,
        TEMP_FILE_RETENTION,
    )
}

fn cleanup_stale_state_files_in(
    state_dir: &Path,
    now: SystemTime,
    script_retention: Duration,
    temp_file_retention: Duration,
) -> io::Result<()> {
    let entries = match fs::read_dir(state_dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };

        // Cleared markers only gate the gap between a new notification being
        // queued and its script starting (seconds); every send resets the
        // marker first, so any marker old enough to be stale here is inert.
        let retention = if name.starts_with("focus-") && name.ends_with(".sh") {
            Some(script_retention)
        } else if name.contains(".result.")
            || name.contains(".status.")
            || name.ends_with(".cleared")
        {
            Some(temp_file_retention)
        } else {
            None
        };

        let Some(retention) = retention else {
            continue;
        };
        if !path.is_file() {
            continue;
        }

        let is_stale = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age >= retention);

        if is_stale {
            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                Err(err) => return Err(err),
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleanup_removes_only_generated_state_files() {
        let dir = std::env::temp_dir().join(format!(
            "herdr-focus-notify-state-test-{}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("focus-old.sh"), "old").unwrap();
        fs::write(dir.join("herdr-w1-p1.result.123456"), "old").unwrap();
        fs::write(dir.join("herdr-w1-p1.status.123456"), "old").unwrap();
        fs::write(dir.join("herdr-w1-p1.cleared"), "old").unwrap();
        fs::write(dir.join("focus-click.log"), "keep").unwrap();

        cleanup_stale_state_files_in(&dir, SystemTime::now(), Duration::ZERO, Duration::ZERO)
            .unwrap();

        assert!(!dir.join("focus-old.sh").exists());
        assert!(!dir.join("herdr-w1-p1.result.123456").exists());
        assert!(!dir.join("herdr-w1-p1.status.123456").exists());
        assert!(!dir.join("herdr-w1-p1.cleared").exists());
        assert!(dir.join("focus-click.log").exists());
        fs::remove_dir_all(dir).unwrap();
    }
}
