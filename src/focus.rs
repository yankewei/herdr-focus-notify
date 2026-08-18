use serde::Deserialize;
use std::process::Command;

use crate::notification::FocusNotification;
use crate::util::sanitize_group_id;

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
        title: "Herdr Focus Notify test".to_string(),
        body: format!("Click to run: {herdr_bin} agent focus {pane_id}"),
        group: format!("herdr-{}", sanitize_group_id(&pane_id)),
        app_icon: None,
    }
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
pub(crate) fn should_clear_notification_on_focus(workspace: &str) -> bool {
    match (
        frontmost_bundle_id(),
        crate::state::remembered_terminal(workspace),
    ) {
        (Some(frontmost), Some(bound)) => frontmost == bound,
        _ => false,
    }
}

/// Learns the currently frontmost app as the terminal bound to `workspace`.
///
/// No whitelist: a `pane.focused` event only fires while the user is
/// operating Herdr inside a terminal, so the frontmost app is trusted to be
/// that terminal. The one spoofable path is a `pane.focused` produced by
/// `herdr agent focus` after a notification click (frontmost is then the
/// browser or notification app); that mis-binding is bounded to this
/// workspace and corrected on the next genuine focus. Best-effort: a failure
/// must never break the pane.focused handling.
pub(crate) fn learn_terminal_from_frontmost(workspace: &str) -> Option<String> {
    let frontmost = frontmost_bundle_id()?;
    // Skip the write when the workspace is already bound to this app.
    if crate::state::remembered_terminal(workspace).as_deref() == Some(frontmost.as_str()) {
        return Some(frontmost);
    }
    crate::state::remember_terminal(workspace, &frontmost).ok()?;
    Some(frontmost)
}

fn pane_is_focused(pane_id: &str, herdr_bin: &str) -> bool {
    let Some(json) = run_herdr(herdr_bin, &["agent", "get", pane_id]) else {
        return false;
    };
    agent_is_focused_from_get_json(&json, pane_id)
        .ok()
        .flatten()
        .unwrap_or(false)
}

/// Runs herdr with the given arguments and returns its stdout on success.
/// None when the binary is missing, the command fails, or the output is not
/// valid UTF-8.
fn run_herdr(herdr_bin: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(herdr_bin).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

fn frontmost_bundle_id() -> Option<String> {
    let output = Command::new("osascript")
        .arg("-e")
        .arg("tell application \"System Events\" to return bundle identifier of first application process whose frontmost is true")
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    String::from_utf8(output.stdout)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn focused_pane_id(herdr_bin: &str) -> Option<String> {
    let json = run_herdr(herdr_bin, &["pane", "list"])?;
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
    let json = run_herdr(herdr_bin, &["pane", "list"])?;
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
}
