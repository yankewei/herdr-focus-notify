use serde::Deserialize;
use std::process::Command;

use crate::config::activate_app;
use crate::notification::FocusNotification;
use crate::util::sanitize_group_id;

#[derive(Debug, Deserialize)]
struct AgentListEnvelope {
    result: Option<AgentListResult>,
}

#[derive(Debug, Deserialize)]
struct AgentListResult {
    agents: Vec<AgentInfo>,
}

#[derive(Debug, Deserialize)]
struct AgentInfo {
    focused: bool,
    pane_id: Option<String>,
}

pub(crate) fn test_notification(herdr_bin: &str) -> FocusNotification {
    let pane_id = focused_pane_id(herdr_bin).unwrap_or_else(|| "test-pane".to_string());
    FocusNotification {
        pane_id: pane_id.clone(),
        status: "blocked".to_string(),
        title: "Herdr Focus Notify test".to_string(),
        body: format!("Click to run: {herdr_bin} agent focus {pane_id}"),
        group: format!("herdr-{}", sanitize_group_id(&pane_id)),
    }
}

pub(crate) fn should_skip_notification(pane_id: &str, herdr_bin: &str) -> bool {
    should_skip_from_focus_and_bundles(
        pane_is_focused(pane_id, herdr_bin),
        herdr_bundle_id(),
        frontmost_bundle_id(),
    )
}

fn pane_is_focused(pane_id: &str, herdr_bin: &str) -> bool {
    focused_pane_id(herdr_bin)
        .map(|focused| focused == pane_id)
        .unwrap_or(false)
}

fn herdr_bundle_id() -> Option<String> {
    bundle_id_from_app(activate_app().as_deref())
}

fn frontmost_bundle_id() -> Option<String> {
    frontmost_bundle_id_via_applescript()
}

fn frontmost_bundle_id_via_applescript() -> Option<String> {
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

fn bundle_id_from_app(app: Option<&str>) -> Option<String> {
    let app = app?;
    let escaped = app.replace('"', "\\\"");
    let script = format!("id of app \"{escaped}\"");

    let output = Command::new("osascript")
        .arg("-e")
        .arg(&script)
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
    let output = Command::new(herdr_bin)
        .arg("agent")
        .arg("list")
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let json = String::from_utf8(output.stdout).ok()?;
    focused_pane_id_from_agent_list_json(&json).ok().flatten()
}

fn focused_pane_id_from_agent_list_json(json: &str) -> Result<Option<String>, String> {
    let envelope: AgentListEnvelope =
        serde_json::from_str(json).map_err(|err| format!("invalid agent list json: {err}"))?;

    Ok(envelope.result.and_then(|result| {
        result.agents.into_iter().find_map(|agent| {
            agent
                .focused
                .then_some(agent.pane_id)
                .flatten()
                .map(|pane_id| pane_id.trim().to_string())
                .filter(|pane_id| !pane_id.is_empty())
        })
    }))
}

fn should_skip_from_focus_and_bundles(
    pane_is_focused: bool,
    herdr_bundle_id: Option<String>,
    frontmost_bundle_id: Option<String>,
) -> bool {
    pane_is_focused
        && matches!((herdr_bundle_id, frontmost_bundle_id), (Some(herdr), Some(frontmost)) if herdr == frontmost)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_focused_pane_from_agent_list_json() {
        let json = r#"{
            "id": "cli:agent:list",
            "result": {
                "agents": [
                    {"agent": "codex", "focused": false, "pane_id": "w1:p1"},
                    {"agent": "kimi", "focused": true, "pane_id": "w1:p2"}
                ]
            }
        }"#;

        assert_eq!(
            focused_pane_id_from_agent_list_json(json).unwrap(),
            Some("w1:p2".to_string())
        );
    }

    #[test]
    fn skips_only_when_same_pane_and_frontmost_app_are_confirmed() {
        assert!(should_skip_from_focus_and_bundles(
            true,
            Some("com.example.Herdr".to_string()),
            Some("com.example.Herdr".to_string())
        ));
        assert!(!should_skip_from_focus_and_bundles(
            true,
            Some("com.example.Herdr".to_string()),
            Some("com.apple.Terminal".to_string())
        ));
        assert!(!should_skip_from_focus_and_bundles(
            true,
            None,
            Some("com.example.Herdr".to_string())
        ));
        assert!(!should_skip_from_focus_and_bundles(
            true,
            Some("com.example.Herdr".to_string()),
            None
        ));
        assert!(!should_skip_from_focus_and_bundles(
            false,
            Some("com.example.Herdr".to_string()),
            Some("com.example.Herdr".to_string())
        ));
    }
}
