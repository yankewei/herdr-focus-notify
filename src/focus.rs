use serde::Deserialize;
use std::env;
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::os::unix::net::UnixStream;
use std::process::Command;
use std::time::Duration;

use crate::notification::FocusNotification;
use crate::util::{command_stdout, sanitize_group_id};

/// How long a notification click waits for Herdr's pane focus response. A real
/// click runs detached, where an unanswered request would leave a stray process
/// behind; `--test` runs in the foreground and would hang the action outright.
const FOCUS_SOCKET_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Deserialize)]
struct PaneListEnvelope {
    result: Option<PaneListResult>,
}

#[derive(Debug, Deserialize)]
struct PaneListResult {
    panes: Vec<AgentInfo>,
}

#[derive(Debug, Deserialize)]
struct AgentGetEnvelope {
    result: Option<AgentGetResult>,
}

#[derive(Debug, Deserialize)]
struct AgentGetResult {
    agent: Option<AgentInfo>,
}

#[derive(Debug, Deserialize)]
struct AgentInfo {
    focused: bool,
    pane_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NotificationDecision {
    Skip,
    Send,
    SendWithVisibilityMonitor,
}

pub(crate) fn test_notification(herdr_bin: &str) -> FocusNotification {
    let pane_id = focused_pane_id(herdr_bin).unwrap_or_else(|| "test-pane".to_string());
    FocusNotification {
        pane_id: pane_id.clone(),
        status: "blocked".to_string(),
        title: "Focus notification test".to_string(),
        body: "Click to return to this Herdr pane.".to_string(),
        group: format!("herdr-{}", sanitize_group_id(&pane_id)),
        app_icon: None,
    }
}

/// Activates the workspace's bound terminal and focuses the target pane.
///
/// Without a terminal binding there is no visible app to activate, so a
/// notification click is intentionally a no-op. A binding is checked again
/// here at click time because generated notification scripts can outlive the
/// state that existed when they were written.
pub(crate) fn focus_pane(pane_id: &str) -> Result<(), String> {
    let workspace = crate::util::workspace_id_from_pane_id(pane_id).unwrap_or("default");
    let Some(bound_terminal) = crate::state::remembered_terminal(workspace) else {
        return Ok(());
    };
    crate::state::mark_focus_origin(workspace)
        .map_err(|err| format!("failed to mark notification focus: {err}"))?;
    let result = (|| -> Result<(), String> {
        // Select the terminal container showing Herdr before activating the
        // terminal, so it brings that container forward. Terminals without an
        // adapter, or any failure, keep plain app activation.
        let socket_path = env::var("HERDR_SOCKET_PATH").ok();
        if let Some(socket_path) = &socket_path {
            let _ = crate::terminal::raise_client_container(&bound_terminal, socket_path);
        }
        activate_terminal(&bound_terminal)?;
        let socket_path = socket_path.ok_or("HERDR_SOCKET_PATH is unavailable")?;
        focus_pane_via_socket(pane_id, &socket_path)
    })();

    if result.is_err() {
        let _ = crate::state::clear_focus_origin(workspace);
    }

    result
}

fn focus_pane_via_socket(pane_id: &str, socket_path: &str) -> Result<(), String> {
    let mut stream = UnixStream::connect(socket_path)
        .map_err(|err| format!("failed to connect to Herdr socket {socket_path}: {err}"))?;
    stream
        .set_read_timeout(Some(FOCUS_SOCKET_TIMEOUT))
        .map_err(|err| format!("failed to configure Herdr socket timeout: {err}"))?;
    let request = serde_json::json!({
        "id": "herdr-focus-notify:focus",
        "method": "pane.focus",
        "params": {"pane_id": pane_id},
    });

    serde_json::to_writer(&mut stream, &request)
        .map_err(|err| format!("failed to encode pane focus request: {err}"))?;
    stream
        .write_all(b"\n")
        .map_err(|err| format!("failed to send pane focus request: {err}"))?;

    let mut response_line = String::new();
    BufReader::new(stream)
        .read_line(&mut response_line)
        .map_err(|err| {
            if matches!(err.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) {
                format!(
                    "timed out after {}s waiting for Herdr's pane focus response",
                    FOCUS_SOCKET_TIMEOUT.as_secs()
                )
            } else {
                format!("failed to read pane focus response: {err}")
            }
        })?;
    if response_line.is_empty() {
        return Err("Herdr closed the socket without a pane focus response".to_string());
    }

    let response: serde_json::Value = serde_json::from_str(&response_line)
        .map_err(|err| format!("invalid pane focus response: {err}"))?;
    if response.get("id").and_then(serde_json::Value::as_str) != Some("herdr-focus-notify:focus") {
        return Err("pane focus response has an unexpected request id".to_string());
    }
    if let Some(error) = response.get("error") {
        let message = error
            .get("message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown Herdr error");
        return Err(format!("failed to focus pane: {message}"));
    }

    let result_type = response
        .pointer("/result/type")
        .and_then(serde_json::Value::as_str);
    if result_type != Some("pane_info") {
        return Err("pane focus response is missing pane_info result".to_string());
    }
    if response
        .pointer("/result/pane/pane_id")
        .and_then(serde_json::Value::as_str)
        != Some(pane_id)
    {
        return Err("pane focus response returned a different pane".to_string());
    }

    Ok(())
}

fn activate_terminal(bundle_id: &str) -> Result<(), String> {
    let output = Command::new("open")
        .args(["-b", bundle_id])
        .output()
        .map_err(|err| format!("failed to activate terminal {bundle_id}: {err}"))?;
    if !output.status.success() {
        return Err(format!(
            "failed to activate terminal {bundle_id}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

pub(crate) fn notification_decision(pane_id: &str, herdr_bin: &str) -> NotificationDecision {
    let workspace = crate::util::workspace_id_from_pane_id(pane_id).unwrap_or("default");
    notification_decision_from_focus_and_bundles(
        pane_is_focused(pane_id, herdr_bin),
        crate::state::remembered_terminal(workspace),
        frontmost_bundle_id(),
    )
}

/// Whether the previously queued notification for the now-focused pane can be
/// removed. The user only sees the pane when the frontmost app is the terminal
/// bound to its workspace, so removal requires the frontmost bundle id to be
/// that terminal (never a random app).
pub(crate) fn should_clear_notification_on_focus(workspace: &str, frontmost: Option<&str>) -> bool {
    match (frontmost, crate::state::remembered_terminal(workspace)) {
        (Some(frontmost), Some(bound)) => frontmost == bound,
        _ => false,
    }
}

/// Learns `frontmost` as the terminal bound to `workspace`.
///
/// The caller passes the frontmost app it observed for this focus event, so
/// learning and the clear decision agree on a single sample instead of reading
/// the frontmost app twice. This matters because both reads describe a focus
/// change that has already happened: the later they run, the greater the chance
/// the user has switched to another app and the workspace goes unbound.
///
/// A focus event produced by a notification click must not overwrite the
/// workspace binding with the browser or notification app that was frontmost
/// when the click happened. Obvious non-terminal apps are also rejected as a
/// defense in depth, while unknown apps remain eligible so real terminals and
/// IDEs with integrated terminals still work without configuration.
pub(crate) fn learn_terminal_from_frontmost(
    workspace: &str,
    frontmost: Option<&str>,
) -> Option<String> {
    let existing = crate::state::remembered_terminal(workspace);
    if crate::state::focus_origin_is_active(workspace) {
        return existing;
    }

    let frontmost = frontmost?;
    if crate::state::is_obvious_non_terminal_bundle(frontmost) {
        return existing;
    }
    // Skip the write when the workspace is already bound to this app.
    if existing.as_deref() == Some(frontmost) {
        return Some(frontmost.to_string());
    }
    crate::state::remember_terminal(workspace, frontmost).ok()?;
    Some(frontmost.to_string())
}

fn pane_is_focused(pane_id: &str, herdr_bin: &str) -> bool {
    let Some(json) = command_stdout(herdr_bin, &["agent", "get", pane_id]) else {
        return false;
    };
    agent_is_focused_from_get_json(&json, pane_id)
        .ok()
        .flatten()
        .unwrap_or(false)
}

/// The bundle identifier of the frontmost macOS app, or None when it cannot be
/// determined.
///
/// `lsappinfo` answers in ~10ms, where the AppleScript/System Events query it
/// replaces took ~170ms and ran twice per `pane.focused` event. The answer
/// describes a focus change that already happened, so a slow read observes the
/// app frontmost well after the fact and can miss the terminal entirely.
pub(crate) fn frontmost_bundle_id() -> Option<String> {
    let asn = command_stdout("lsappinfo", &["front"])?;
    let info = command_stdout("lsappinfo", &["info", "-only", "bundleID", asn.trim()])?;
    bundle_id_from_lsappinfo(&info)
}

/// Reads the bundle identifier from either form of `lsappinfo info` output.
/// An app without a bundle identifier reports `[ NULL ]`, which is not a binding.
fn bundle_id_from_lsappinfo(output: &str) -> Option<String> {
    output
        .lines()
        .map(str::trim)
        .find_map(|line| {
            let (key, value) = line.split_once('=')?;
            match key.trim().trim_matches('"') {
                "bundleID" | "CFBundleIdentifier" => Some(value),
                _ => None,
            }
        })
        .map(|value| value.trim().trim_matches('"').to_string())
        .filter(|value| !value.is_empty() && !value.starts_with('['))
}

fn focused_pane_id(herdr_bin: &str) -> Option<String> {
    let json = command_stdout(herdr_bin, &["pane", "list"])?;
    focused_pane_id_from_pane_list_json(&json).ok().flatten()
}

fn focused_pane_id_from_pane_list_json(json: &str) -> Result<Option<String>, String> {
    let envelope: PaneListEnvelope =
        serde_json::from_str(json).map_err(|err| format!("invalid pane list json: {err}"))?;

    Ok(envelope.result.and_then(|result| {
        result.panes.into_iter().find_map(|agent| {
            if !agent.focused {
                return None;
            }
            agent
                .pane_id
                .map(|pane_id| pane_id.trim().to_string())
                .filter(|pane_id| !pane_id.is_empty())
        })
    }))
}

fn agent_is_focused_from_get_json(
    json: &str,
    expected_pane_id: &str,
) -> Result<Option<bool>, String> {
    let envelope: AgentGetEnvelope =
        serde_json::from_str(json).map_err(|err| format!("invalid agent get json: {err}"))?;

    Ok(envelope.result.and_then(|result| {
        result
            .agent
            .map(|agent| agent.focused && agent.pane_id.as_deref() == Some(expected_pane_id))
    }))
}

/// The workspaces that currently exist, derived from `herdr pane list`.
///
/// Returns `None` when the workspace set cannot be trusted (command failure,
/// empty output, or no panes at all) so callers never prune bindings against
/// an empty world — e.g. right after Herdr itself started.
pub(crate) fn live_workspace_ids(herdr_bin: &str) -> Option<Vec<String>> {
    let json = command_stdout(herdr_bin, &["pane", "list"])?;
    let live = live_workspace_ids_from_pane_list_json(&json)
        .ok()
        .flatten()?;
    if live.is_empty() {
        return None;
    }
    Some(live)
}

fn live_workspace_ids_from_pane_list_json(json: &str) -> Result<Option<Vec<String>>, String> {
    let envelope: PaneListEnvelope =
        serde_json::from_str(json).map_err(|err| format!("invalid pane list json: {err}"))?;

    let mut seen: Vec<String> = Vec::new();
    if let Some(result) = envelope.result {
        for agent in result.panes {
            if let Some(pane_id) = agent.pane_id {
                if let Some(workspace) = crate::util::workspace_id_from_pane_id(pane_id.trim()) {
                    if !seen.iter().any(|value| value == workspace) {
                        seen.push(workspace.to_string());
                    }
                }
            }
        }
    }
    Ok((!seen.is_empty()).then_some(seen))
}

fn notification_decision_from_focus_and_bundles(
    pane_is_focused: bool,
    bound_terminal: Option<String>,
    frontmost: Option<String>,
) -> NotificationDecision {
    if !pane_is_focused {
        return NotificationDecision::Send;
    }

    match (frontmost, bound_terminal) {
        (Some(frontmost), Some(bound)) if frontmost == bound => NotificationDecision::Skip,
        (Some(_), Some(_)) => NotificationDecision::SendWithVisibilityMonitor,
        _ => NotificationDecision::Send,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_focused_pane_from_pane_list_json() {
        let json = r#"{
            "id": "cli:pane:list",
            "result": {
                "panes": [
                    {"agent": "codex", "focused": false, "pane_id": "w1:p1"},
                    {"agent": "kimi", "focused": true, "pane_id": "w1:p2"}
                ]
            }
        }"#;

        assert_eq!(
            focused_pane_id_from_pane_list_json(json).unwrap(),
            Some("w1:p2".to_string())
        );
    }

    #[test]
    fn extracts_live_workspaces_from_pane_list_json() {
        let json = r#"{
            "id": "cli:pane:list",
            "result": {
                "panes": [
                    {"agent": "codex", "focused": false, "pane_id": "w1:p1"},
                    {"agent": "kimi", "focused": true, "pane_id": "w1:p2"},
                    {"agent": "claude", "focused": false, "pane_id": "w2:p1"}
                ]
            }
        }"#;

        assert_eq!(
            live_workspace_ids_from_pane_list_json(json).unwrap(),
            Some(vec!["w1".to_string(), "w2".to_string()])
        );

        // No panes at all is untrustworthy for pruning.
        let empty = r#"{"id":"cli:pane:list","result":{"panes":[]}}"#;
        assert_eq!(live_workspace_ids_from_pane_list_json(empty).unwrap(), None);
    }

    #[test]
    fn reads_focus_from_agent_get_json() {
        let json = r#"{
            "id": "cli:agent:get",
            "result": {
                "agent": {
                    "focused": true,
                    "pane_id": "w1:p2"
                }
            }
        }"#;

        assert_eq!(
            agent_is_focused_from_get_json(json, "w1:p2").unwrap(),
            Some(true)
        );
        assert_eq!(
            agent_is_focused_from_get_json(json, "w1:p3").unwrap(),
            Some(false)
        );
    }

    #[test]
    fn decides_when_to_skip_or_monitor_notifications() {
        // Pane focused + frontmost matches the bound terminal -> skip.
        assert_eq!(
            notification_decision_from_focus_and_bundles(
                true,
                Some("com.example.Herdr".to_string()),
                Some("com.example.Herdr".to_string())
            ),
            NotificationDecision::Skip
        );
        // Pane focused + frontmost matches the workspace binding -> skip.
        assert_eq!(
            notification_decision_from_focus_and_bundles(
                true,
                Some("com.googlecode.iterm2".to_string()),
                Some("com.googlecode.iterm2".to_string())
            ),
            NotificationDecision::Skip
        );
        // Pane focused + frontmost outside the bound terminal -> notify, with
        // a visibility monitor since a terminal is bound.
        assert_eq!(
            notification_decision_from_focus_and_bundles(
                true,
                Some("com.example.Herdr".to_string()),
                Some("com.apple.Terminal".to_string())
            ),
            NotificationDecision::SendWithVisibilityMonitor
        );
        // Pane focused + frontmost is a non-terminal app and a terminal is
        // bound -> notify with a visibility monitor so it auto-dismisses.
        assert_eq!(
            notification_decision_from_focus_and_bundles(
                true,
                Some("com.example.Herdr".to_string()),
                Some("com.google.Chrome".to_string())
            ),
            NotificationDecision::SendWithVisibilityMonitor
        );
        // Pane focused + frontmost with no bound terminal -> notify.
        assert_eq!(
            notification_decision_from_focus_and_bundles(
                true,
                None,
                Some("com.google.Chrome".to_string())
            ),
            NotificationDecision::Send
        );
        // Pane focused + frontmost unknown -> notify (conservative).
        assert_eq!(
            notification_decision_from_focus_and_bundles(
                true,
                Some("com.example.Herdr".to_string()),
                None
            ),
            NotificationDecision::Send
        );
        // Pane not focused -> notify.
        assert_eq!(
            notification_decision_from_focus_and_bundles(
                false,
                Some("com.example.Herdr".to_string()),
                Some("com.example.Herdr".to_string())
            ),
            NotificationDecision::Send
        );
    }

    #[test]
    fn reads_bundle_id_from_lsappinfo_output() {
        let info = "[ NULL ]  ASN:0x0-0x3e03e: (in front)\n    bundleID=\"net.kovidgoyal.kitty\"\n    bundle path=[ NULL ] \n    executable path=[ NULL ] \n";

        assert_eq!(
            bundle_id_from_lsappinfo(info).as_deref(),
            Some("net.kovidgoyal.kitty")
        );
        // `bundle path` and `executable path` are separate fields, and an app
        // without a bundle identifier must not become a binding.
        let current = "\"CFBundleIdentifier\"=\"com.mitchellh.ghostty\"\n";
        assert_eq!(
            bundle_id_from_lsappinfo(current).as_deref(),
            Some("com.mitchellh.ghostty")
        );
        assert_eq!(
            bundle_id_from_lsappinfo("\"CFBundleIdentifier\"=[ NULL ]\n"),
            None
        );
        assert_eq!(bundle_id_from_lsappinfo("    bundleID=[ NULL ] \n"), None);
        assert_eq!(bundle_id_from_lsappinfo(""), None);
    }
}
