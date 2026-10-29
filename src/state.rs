use std::collections::HashMap;
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use crate::util::notification_group_id;

const GENERATED_SCRIPT_RETENTION: Duration = Duration::from_secs(30 * 24 * 60 * 60);
const TEMP_FILE_RETENTION: Duration = Duration::from_secs(24 * 60 * 60);
const FOCUS_ORIGIN_RETENTION: Duration = Duration::from_secs(15);

type WorkspaceBindings = HashMap<String, String>;

/// Every Herdr session runs its own plugin processes against one shared state
/// directory, and workspace ids are only unique within a session, so bindings
/// are kept per session, keyed by the session's API socket.
#[derive(Debug, Default, Deserialize, Serialize)]
struct TerminalMemory {
    sessions: HashMap<String, WorkspaceBindings>,
}

pub(crate) fn plugin_state_dir() -> PathBuf {
    env::var_os("HERDR_PLUGIN_STATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| env::temp_dir().join("herdr-focus-notify"))
}

/// The most recently used terminal (learned from `pane.focused` events) is
/// persisted here so a click can activate it and skip checks can match it,
/// with no configuration required.
pub(crate) fn remember_terminal(workspace: &str, bundle_id: &str) -> io::Result<()> {
    let state_dir = plugin_state_dir();
    fs::create_dir_all(&state_dir)?;
    remember_terminal_into(&state_dir, &session_key(), workspace, bundle_id)
}

/// The terminal bound to a pane's workspace, learned from `pane.focused`
/// events. `None` until the workspace has been focused at least once or when
/// the saved bundle id is an obvious non-terminal app.
pub(crate) fn remembered_terminal(workspace: &str) -> Option<String> {
    read_terminal_memory_from(&terminal_memory_path())
        .sessions
        .remove(&session_key())?
        .get(workspace)
        .filter(|bundle_id| !is_obvious_non_terminal_bundle(bundle_id))
        .cloned()
}

/// Clears all saved workspace-to-terminal bindings.
pub(crate) fn clear_terminal_bindings() -> io::Result<()> {
    let binding_result = match fs::remove_file(terminal_memory_path()) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    };
    binding_result?;
    clear_focus_origin_markers()
}

/// Marks a focus operation as originating from a notification click. The
/// marker is intentionally short-lived because Herdr delivers the resulting
/// focus event in a separate plugin process.
pub(crate) fn mark_focus_origin(workspace: &str) -> io::Result<()> {
    let state_dir = plugin_state_dir();
    fs::create_dir_all(&state_dir)?;
    fs::write(focus_origin_marker_path(workspace), [])
}

/// Returns whether a recent notification-originated focus operation is still
/// suppressing terminal learning for this workspace.
pub(crate) fn focus_origin_is_active(workspace: &str) -> bool {
    let path = focus_origin_marker_path(workspace);
    let active = fs::metadata(&path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .is_some_and(|age| age < FOCUS_ORIGIN_RETENTION);

    if !active {
        let _ = fs::remove_file(path);
    }
    active
}

pub(crate) fn clear_focus_origin(workspace: &str) -> io::Result<()> {
    match fs::remove_file(focus_origin_marker_path(workspace)) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

pub(crate) fn focus_origin_marker_path(workspace: &str) -> PathBuf {
    plugin_state_dir().join(format!(
        "focus-origin-{}.marker",
        crate::util::sanitize_group_id(workspace)
    ))
}

fn clear_focus_origin_markers() -> io::Result<()> {
    let state_dir = plugin_state_dir();
    let entries = match fs::read_dir(&state_dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };

    for entry in entries {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name.starts_with("focus-origin-") && name.ends_with(".marker") && path.is_file() {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

/// Removes this session's bindings for workspaces that no longer exist in it.
///
/// Uses `herdr pane list` to learn the live workspace set; when that cannot
/// be trusted (command failure or no panes at all) nothing is pruned, so the
/// memory file never loses bindings against an empty world. Best-effort: a
/// stale binding is harmless, it just gets re-learned on the next focus.
/// The rewrite reuses the same atomic write path as learning.
pub(crate) fn prune_stale_workspace_bindings(herdr_bin: &str) -> io::Result<()> {
    let state_dir = plugin_state_dir();
    let mut memory = read_terminal_memory_from(&terminal_memory_path());
    let session = session_key();
    if !memory.sessions.contains_key(&session) {
        return Ok(());
    }
    let Some(live) = crate::focus::live_workspace_ids(herdr_bin) else {
        return Ok(());
    };

    if !retain_live_workspaces(&mut memory, &session, &live) {
        return Ok(());
    }

    fs::create_dir_all(&state_dir)?;
    write_terminal_memory_into(&state_dir, &memory)
}

/// Drops `session`'s bindings for workspaces missing from `live`, leaving
/// other sessions untouched. Returns whether anything was dropped.
fn retain_live_workspaces(memory: &mut TerminalMemory, session: &str, live: &[String]) -> bool {
    let Some(bindings) = memory.sessions.get_mut(session) else {
        return false;
    };
    let before = bindings.len();
    bindings.retain(|workspace, _| live.iter().any(|id| id == workspace));
    let pruned = bindings.len() != before;
    if bindings.is_empty() {
        memory.sessions.remove(session);
    }
    pruned
}

fn write_terminal_memory_into(dir: &Path, memory: &TerminalMemory) -> io::Result<()> {
    let path = dir.join("terminal-memory.json");
    let temp_path = dir.join(format!(".terminal-memory-{}.tmp", std::process::id()));
    fs::write(
        &temp_path,
        serde_json::to_vec(memory).map_err(io::Error::other)?,
    )?;
    fs::rename(temp_path, &path)
}

fn remember_terminal_into(
    dir: &Path,
    session: &str,
    workspace: &str,
    bundle_id: &str,
) -> io::Result<()> {
    // Read existing bindings so other workspaces and sessions survive the
    // rewrite.
    let mut memory = read_terminal_memory_from(&dir.join("terminal-memory.json"));
    memory
        .sessions
        .entry(session.to_string())
        .or_default()
        .insert(workspace.to_string(), bundle_id.to_string());

    write_terminal_memory_into(dir, &memory)
}

/// Reads `{ "sessions": { <socket>: { <workspace>: <bundle> } } }`. A missing
/// or unreadable file, including the pre-session `{ "workspaces": ... }`
/// format, reads as empty and is replaced on the next write.
fn read_terminal_memory_from(path: &Path) -> TerminalMemory {
    fs::read_to_string(path)
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_default()
}

fn terminal_memory_path() -> PathBuf {
    plugin_state_dir().join("terminal-memory.json")
}

/// The session the plugin process runs for, identified by the API socket
/// Herdr injects. Manual runs without one share the empty key.
fn session_key() -> String {
    env::var("HERDR_SOCKET_PATH").unwrap_or_default()
}

pub(crate) fn is_obvious_non_terminal_bundle(bundle_id: &str) -> bool {
    matches!(
        bundle_id,
        "com.google.Chrome"
            | "com.apple.Safari"
            | "org.mozilla.firefox"
            | "com.microsoft.edgemac"
            | "company.thebrowser.Browser"
            | "com.brave.Browser"
            | "com.apple.finder"
            | "com.apple.notificationcenterui"
    )
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
        FOCUS_ORIGIN_RETENTION,
    )
}

/// The retention period for a generated state file, or None when the file is
/// not managed by cleanup. Cleared markers only gate the gap between a new
/// notification being queued and its script starting (seconds); every send
/// resets the marker first, so any marker old enough to be stale here is
/// inert.
fn retention_for(
    name: &str,
    script_retention: Duration,
    temp_file_retention: Duration,
    focus_origin_retention: Duration,
) -> Option<Duration> {
    if name.starts_with("focus-") && name.ends_with(".sh") {
        Some(script_retention)
    } else if name.starts_with("focus-origin-") && name.ends_with(".marker") {
        Some(focus_origin_retention)
    } else if name.contains(".result.") || name.contains(".status.") || name.ends_with(".cleared") {
        Some(temp_file_retention)
    } else {
        None
    }
}

fn cleanup_stale_state_files_in(
    state_dir: &Path,
    now: SystemTime,
    script_retention: Duration,
    temp_file_retention: Duration,
    focus_origin_retention: Duration,
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

        let Some(retention) = retention_for(
            name,
            script_retention,
            temp_file_retention,
            focus_origin_retention,
        ) else {
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
        fs::write(dir.join("focus-origin-w1.marker"), "old").unwrap();
        fs::write(dir.join("focus-click.log"), "keep").unwrap();

        cleanup_stale_state_files_in(
            &dir,
            SystemTime::now(),
            Duration::ZERO,
            Duration::ZERO,
            Duration::ZERO,
        )
        .unwrap();

        assert!(!dir.join("focus-old.sh").exists());
        assert!(!dir.join("herdr-w1-p1.result.123456").exists());
        assert!(!dir.join("herdr-w1-p1.status.123456").exists());
        assert!(!dir.join("herdr-w1-p1.cleared").exists());
        assert!(!dir.join("focus-origin-w1.marker").exists());
        assert!(dir.join("focus-click.log").exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn bindings_are_kept_per_workspace_and_per_session() {
        let dir = std::env::temp_dir().join(format!(
            "herdr-focus-notify-terminal-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("terminal-memory.json");
        let (a, b) = ("/tmp/a/herdr.sock", "/tmp/b/herdr.sock");
        let binding = |session: &str, workspace: &str| {
            read_terminal_memory_from(&path)
                .sessions
                .get(session)
                .and_then(|bindings| bindings.get(workspace).cloned())
        };

        // A pre-session file reads as empty instead of failing.
        fs::write(&path, r#"{"workspaces":{"w1":"com.googlecode.iterm2"}}"#).unwrap();
        assert!(read_terminal_memory_from(&path).sessions.is_empty());

        // Workspaces bind independently, and the same workspace id in another
        // session is a different binding.
        remember_terminal_into(&dir, a, "w1", "com.googlecode.iterm2").unwrap();
        remember_terminal_into(&dir, a, "w2", "net.kovidgoyal.kitty").unwrap();
        remember_terminal_into(&dir, b, "w1", "app.warp").unwrap();
        assert_eq!(binding(a, "w1").as_deref(), Some("com.googlecode.iterm2"));
        assert_eq!(binding(a, "w2").as_deref(), Some("net.kovidgoyal.kitty"));
        assert_eq!(binding(b, "w1").as_deref(), Some("app.warp"));

        // Overwriting one binding disturbs no other.
        remember_terminal_into(&dir, a, "w1", "com.mitchellh.ghostty").unwrap();
        assert_eq!(binding(a, "w1").as_deref(), Some("com.mitchellh.ghostty"));
        assert_eq!(binding(a, "w2").as_deref(), Some("net.kovidgoyal.kitty"));
        assert_eq!(binding(b, "w1").as_deref(), Some("app.warp"));

        // Pruning against one session's live workspaces leaves the others,
        // and a session whose workspaces are all gone is dropped.
        let mut memory = read_terminal_memory_from(&path);
        assert!(retain_live_workspaces(&mut memory, a, &["w2".to_string()]));
        assert!(!retain_live_workspaces(&mut memory, a, &["w2".to_string()]));
        assert_eq!(memory.sessions[a].len(), 1);
        assert_eq!(memory.sessions[b]["w1"], "app.warp");
        assert!(retain_live_workspaces(&mut memory, b, &[]));
        assert!(!memory.sessions.contains_key(b));

        fs::remove_dir_all(dir).unwrap();
    }
}
