use serde::Deserialize;

use crate::icons::agent_icon_path;
use crate::notification::FocusNotification;
use crate::util::notification_group_id;

#[derive(Debug, Deserialize)]
struct PluginEvent {
    data: Option<EventData>,
}

#[derive(Debug, Deserialize)]
struct EventData {
    pane_id: Option<String>,
    agent_status: Option<String>,
    agent: Option<String>,
    display_agent: Option<String>,
}

/// The agent statuses worth notifying about. `blocked` and `done` are the
/// only ones needing the user's action; everything else is noise.
pub(crate) fn status_is_enabled(status: &str) -> bool {
    matches!(status, "blocked" | "done")
}

pub(crate) fn notification_from_event_json(
    json: &str,
) -> Result<Option<FocusNotification>, String> {
    let event: PluginEvent =
        serde_json::from_str(json).map_err(|err| format!("invalid event json: {err}"))?;
    let Some(data) = event.data else {
        return Ok(None);
    };

    let status = data
        .agent_status
        .as_deref()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();

    // Only blocked and done need user action, so they are the only statuses
    // that notify.
    if !status_is_enabled(&status) {
        return Ok(None);
    }

    let Some(pane_id) = pane_id_from_event_data(&data) else {
        return Ok(None);
    };

    let agent = first_non_empty([data.display_agent.as_deref(), data.agent.as_deref()])
        .unwrap_or("Agent")
        .to_string();
    let app_icon = agent_icon_path(&[data.display_agent.as_deref(), data.agent.as_deref()]);

    let (title, body) = match status.as_str() {
        "blocked" => (
            format!("{agent} needs your input"),
            "Open the pane to review and respond.".to_string(),
        ),
        "done" => (
            format!("{agent} finished"),
            "Open the pane to review the result.".to_string(),
        ),
        _ => unreachable!("status already filtered"),
    };
    let group = notification_group_id(&pane_id);

    Ok(Some(FocusNotification {
        pane_id,
        status,
        title,
        body,
        group,
        app_icon,
    }))
}

pub(crate) fn focused_pane_id_from_event_json(json: &str) -> Result<Option<String>, String> {
    let event: PluginEvent =
        serde_json::from_str(json).map_err(|err| format!("invalid event json: {err}"))?;

    Ok(event.data.as_ref().and_then(pane_id_from_event_data))
}

fn pane_id_from_event_data(data: &EventData) -> Option<String> {
    data.pane_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn first_non_empty<const N: usize>(values: [Option<&str>; N]) -> Option<&str> {
    values
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_blocked_notification_from_event() {
        let json = r#"{
            "event": "pane.agent_status_changed",
            "data": {
                "pane_id": "w1:p3",
                "workspace_id": "herdr",
                "agent_status": "blocked",
                "agent": "codex",
                "display_agent": "Codex",
                "title": "Implement plugin",
                "custom_status": "Needs an answer"
            }
        }"#;

        let notification = notification_from_event_json(json).unwrap().unwrap();

        assert_eq!(notification.pane_id, "w1:p3");
        assert_eq!(notification.status, "blocked");
        assert_eq!(notification.title, "Codex needs your input");
        assert_eq!(notification.body, "Open the pane to review and respond.");
        assert_eq!(notification.group, "herdr-w1-p3");
        assert!(notification
            .app_icon
            .as_deref()
            .unwrap()
            .ends_with("/icons/codex-color.png"));
    }

    #[test]
    fn builds_done_notification_from_title() {
        let json = r#"{
            "data": {
                "pane_id": "p1",
                "agent_status": "done",
                "agent": "Codex",
                "title": "Implement plugin"
            }
        }"#;

        let notification = notification_from_event_json(json).unwrap().unwrap();

        assert_eq!(notification.status, "done");
        assert_eq!(notification.title, "Codex finished");
        assert_eq!(notification.body, "Open the pane to review the result.");
        assert!(notification.app_icon.is_some());
    }

    #[test]
    fn keeps_notification_copy_simple_when_event_has_status_details() {
        let json = r#"{
            "data": {
                "pane_id": "p1",
                "agent_status": "blocked",
                "agent": "Codex",
                "state_labels": {"reason": "Needs an answer"}
            }
        }"#;

        let notification = notification_from_event_json(json).unwrap().unwrap();

        assert_eq!(notification.title, "Codex needs your input");
        assert_eq!(notification.body, "Open the pane to review and respond.");
    }

    #[test]
    fn ignores_other_statuses() {
        let json = r#"{
            "data": {
                "pane_id": "p1",
                "agent_status": "running",
                "agent": "Codex"
            }
        }"#;

        assert!(notification_from_event_json(json).unwrap().is_none());
    }

    #[test]
    fn ignores_missing_pane_id() {
        let json = r#"{
            "data": {
                "agent_status": "blocked",
                "agent": "Codex"
            }
        }"#;

        assert!(notification_from_event_json(json).unwrap().is_none());
    }

    #[test]
    fn extracts_pane_id_from_focus_event() {
        let json = r#"{
            "event": "pane.focused",
            "data": {
                "pane_id": " w1:p2 "
            }
        }"#;

        assert_eq!(
            focused_pane_id_from_event_json(json).unwrap(),
            Some("w1:p2".to_string())
        );
    }
}
