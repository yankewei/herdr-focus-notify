use serde::Deserialize;

use crate::notification::FocusNotification;
use crate::util::sanitize_group_id;

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
    title: Option<String>,
    custom_status: Option<String>,
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

    if status != "blocked" && status != "done" {
        return Ok(None);
    }

    let Some(pane_id) = data
        .pane_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
    else {
        return Ok(None);
    };

    let agent = first_non_empty([data.display_agent.as_deref(), data.agent.as_deref()])
        .unwrap_or("Agent")
        .to_string();

    let base_title = match status.as_str() {
        "blocked" => format!("{agent} needs attention"),
        "done" => format!("{agent} finished"),
        _ => unreachable!("status already filtered"),
    };
    let title = if let Some(custom_status) = first_non_empty([data.custom_status.as_deref()]) {
        format!("{base_title}: {custom_status}")
    } else {
        base_title
    };

    let body = notification_body(&data);
    let group = format!("herdr-{}", sanitize_group_id(&pane_id));

    Ok(Some(FocusNotification {
        pane_id,
        status,
        title,
        body,
        group,
    }))
}

fn notification_body(data: &EventData) -> String {
    first_non_empty([data.title.as_deref()])
        .map(|text| truncate(text, 220))
        .unwrap_or_else(|| "Click to focus this Herdr agent pane.".to_string())
}

fn first_non_empty<const N: usize>(values: [Option<&str>; N]) -> Option<&str> {
    values
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|value| !value.is_empty())
}

fn truncate(value: &str, max_chars: usize) -> String {
    let trimmed = value.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }

    let mut output: String = trimmed.chars().take(max_chars.saturating_sub(3)).collect();
    output.push_str("...");
    output
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
        assert_eq!(notification.title, "Codex needs attention: Needs an answer");
        assert_eq!(notification.body, "Implement plugin");
        assert_eq!(notification.group, "herdr-w1-p3");
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
        assert_eq!(notification.body, "Implement plugin");
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
}
