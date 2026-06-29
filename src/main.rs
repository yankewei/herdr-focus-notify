use serde::Deserialize;
use std::collections::{hash_map::DefaultHasher, HashMap};
use std::env;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

static CONFIG_ENV: OnceLock<HashMap<String, String>> = OnceLock::new();

#[derive(Debug, Deserialize)]
struct PluginEvent {
    data: Option<EventData>,
}

#[derive(Debug, Deserialize)]
struct EventData {
    pane_id: Option<String>,
    workspace_id: Option<String>,
    workspace: Option<String>,
    tab: Option<String>,
    agent_status: Option<String>,
    agent: Option<String>,
    display_agent: Option<String>,
    title: Option<String>,
    custom_status: Option<String>,
    state_labels: Option<StateLabels>,
}

#[derive(Debug, Deserialize)]
struct StateLabels {
    error: Option<String>,
    task: Option<String>,
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
struct FocusNotification {
    pane_id: String,
    status: String,
    title: String,
    body: String,
    group: String,
}

fn main() {
    if let Err(err) = run() {
        if is_debug_enabled() {
            eprintln!("herdr-focus-notify: {err}");
        }
    }
}

fn run() -> Result<(), String> {
    if !is_enabled() {
        return Ok(());
    }

    let herdr_bin = resolve_herdr_bin();
    let notifier_bin = resolve_notifier_bin();

    let notification = if env::args().any(|arg| arg == "--test") {
        test_notification(&herdr_bin)
    } else {
        let event_json = match env::var("HERDR_PLUGIN_EVENT_JSON") {
            Ok(value) => value,
            Err(_) => return Ok(()),
        };

        match notification_from_event_json(&event_json)? {
            Some(notification) => notification,
            None => return Ok(()),
        }
    };

    if !status_is_enabled(&notification.status) {
        return Ok(());
    }

    if should_skip_notification(&notification.pane_id, &herdr_bin) {
        return Ok(());
    }

    let script_path = write_focus_script(&notification, &herdr_bin, &notifier_bin)
        .map_err(|err| format!("failed to write focus script: {err}"))?;

    send_notification(&notification, &script_path, &notifier_bin)
        .map_err(|err| format!("failed to send notification: {err}"))?;

    Ok(())
}

fn resolve_herdr_bin() -> String {
    config_var("HERDR_BIN_PATH")
        .or_else(|| find_executable("herdr", herdr_candidate_paths()))
        .unwrap_or_else(|| "herdr".to_string())
}

fn resolve_notifier_bin() -> String {
    config_var("HERDR_FOCUS_NOTIFY_NOTIFIER")
        .or_else(|| find_executable("alerter", alerter_candidate_paths()))
        .or_else(|| find_executable("terminal-notifier", terminal_notifier_candidate_paths()))
        .unwrap_or_else(|| "alerter".to_string())
}

fn notifier_kind(notifier_bin: &str) -> &'static str {
    let stem = Path::new(notifier_bin)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    if stem == "alerter" {
        "alerter"
    } else {
        "terminal-notifier"
    }
}

fn alerter_candidate_paths() -> Vec<PathBuf> {
    let mut paths = vec![
        PathBuf::from("/opt/homebrew/bin/alerter"),
        PathBuf::from("/usr/local/bin/alerter"),
    ];
    if let Some(home) = home_dir() {
        paths.push(home.join(".local/bin/alerter"));
    }
    paths
}

fn herdr_candidate_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(home_dir) = home_dir() {
        paths.push(home_dir.join(".local/bin/herdr"));
    }
    paths.push(PathBuf::from("/opt/homebrew/bin/herdr"));
    paths.push(PathBuf::from("/usr/local/bin/herdr"));
    paths
}

fn terminal_notifier_candidate_paths() -> Vec<PathBuf> {
    vec![
        PathBuf::from("/opt/homebrew/bin/terminal-notifier"),
        PathBuf::from("/usr/local/bin/terminal-notifier"),
    ]
}

fn find_executable(name: &str, candidate_paths: Vec<PathBuf>) -> Option<String> {
    executable_in_path(name)
        .or_else(|| {
            candidate_paths
                .into_iter()
                .find(|path| is_executable_file(path))
        })
        .map(|path| path.to_string_lossy().into_owned())
}

fn executable_in_path(name: &str) -> Option<PathBuf> {
    env::var_os("PATH").and_then(|path| {
        env::split_paths(&path)
            .map(|dir| dir.join(name))
            .find(|path| is_executable_file(path))
    })
}

fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME").map(PathBuf::from)
}

#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    path.is_file()
        && path
            .metadata()
            .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}

fn test_notification(herdr_bin: &str) -> FocusNotification {
    let pane_id = focused_pane_id(herdr_bin).unwrap_or_else(|| "test-pane".to_string());
    FocusNotification {
        pane_id: pane_id.clone(),
        status: "blocked".to_string(),
        title: "Herdr Focus Notify test".to_string(),
        body: format!("Click to run: {herdr_bin} agent focus {pane_id}"),
        group: format!("herdr-{}", sanitize_group_id(&pane_id)),
    }
}

fn pane_is_focused(pane_id: &str, herdr_bin: &str) -> bool {
    focused_pane_id(herdr_bin)
        .map(|focused| focused == pane_id)
        .unwrap_or(false)
}

fn should_skip_notification(pane_id: &str, herdr_bin: &str) -> bool {
    if !pane_is_focused(pane_id, herdr_bin) {
        return false;
    }

    match (herdr_bundle_id(), frontmost_bundle_id()) {
        (Some(herdr), Some(frontmost)) => herdr == frontmost,
        _ => true,
    }
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

fn notification_from_event_json(json: &str) -> Result<Option<FocusNotification>, String> {
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

    let title = match status.as_str() {
        "blocked" => format!("{agent} needs attention"),
        "done" => format!("{agent} finished"),
        _ => unreachable!("status already filtered"),
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
    let mut lines = Vec::new();

    if let Some(text) = first_non_empty([
        data.custom_status.as_deref(),
        data.state_labels
            .as_ref()
            .and_then(|labels| labels.error.as_deref()),
        data.state_labels
            .as_ref()
            .and_then(|labels| labels.task.as_deref()),
        data.title.as_deref(),
    ]) {
        lines.push(truncate(text, 220));
    }

    let workspace = first_non_empty([data.workspace.as_deref(), data.workspace_id.as_deref()]);
    let tab = data.tab.as_deref().filter(|value| !value.trim().is_empty());

    if workspace.is_some() || tab.is_some() {
        let mut location = String::from("Location:");
        if let Some(workspace) = workspace {
            location.push(' ');
            location.push_str(workspace);
            if tab.is_some() {
                location.push_str(" / ");
            }
        }
        if let Some(tab) = tab {
            location.push_str(tab);
        }
        lines.push(location);
    }

    if lines.is_empty() {
        "Click to focus this Herdr agent pane.".to_string()
    } else {
        lines.join("\n")
    }
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

fn status_is_enabled(status: &str) -> bool {
    let configured =
        config_var("HERDR_FOCUS_NOTIFY_STATUSES").unwrap_or_else(|| "blocked,done".to_string());

    configured
        .split(',')
        .map(|value| value.trim().to_ascii_lowercase())
        .any(|value| value == status)
}

fn is_enabled() -> bool {
    config_var("HERDR_FOCUS_NOTIFY_ENABLED")
        .map(|value| {
            !matches!(
                value.to_ascii_lowercase().as_str(),
                "0" | "false" | "no" | "off"
            )
        })
        .unwrap_or(true)
}

fn is_debug_enabled() -> bool {
    config_var("HERDR_FOCUS_NOTIFY_DEBUG")
        .map(|value| {
            matches!(
                value.to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

fn alerter_timeout_secs() -> u64 {
    parse_timeout_secs(config_var("HERDR_FOCUS_NOTIFY_TIMEOUT"))
}

fn activate_app() -> Option<String> {
    config_var("HERDR_FOCUS_NOTIFY_ACTIVATE_APP")
}

fn parse_timeout_secs(raw: Option<String>) -> u64 {
    raw.and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(3600)
}

fn config_var(key: &str) -> Option<String> {
    env::var(key)
        .ok()
        .or_else(|| CONFIG_ENV.get_or_init(load_config_env).get(key).cloned())
}

fn load_config_env() -> HashMap<String, String> {
    let Some(config_dir) = env::var_os("HERDR_PLUGIN_CONFIG_DIR") else {
        return HashMap::new();
    };

    let path = PathBuf::from(config_dir).join(".env");
    let Ok(content) = fs::read_to_string(path) else {
        return HashMap::new();
    };

    parse_env_file(&content)
}

fn parse_env_file(content: &str) -> HashMap<String, String> {
    let mut values = HashMap::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let mut parts = trimmed.splitn(2, '=');
        let key = parts.next().unwrap_or("").trim();
        let value = parts.next().unwrap_or("").trim();
        if key.is_empty() {
            continue;
        }

        values.insert(key.to_string(), unquote_env_value(value).to_string());
    }

    values
}

fn unquote_env_value(value: &str) -> &str {
    let bytes = value.as_bytes();
    if bytes.len() >= 2
        && ((bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\''))
    {
        &value[1..value.len() - 1]
    } else {
        value
    }
}

fn write_focus_script(
    notification: &FocusNotification,
    herdr_bin: &str,
    notifier_bin: &str,
) -> io::Result<PathBuf> {
    let state_dir = env::var_os("HERDR_PLUGIN_STATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| env::temp_dir().join("herdr-focus-notify"));
    fs::create_dir_all(&state_dir)?;

    let mut hasher = DefaultHasher::new();
    notification.pane_id.hash(&mut hasher);
    herdr_bin.hash(&mut hasher);
    notifier_bin.hash(&mut hasher);
    activate_app().hash(&mut hasher);
    is_debug_enabled().hash(&mut hasher);

    let script_path = state_dir.join(format!("focus-{:016x}.sh", hasher.finish()));
    let debug_log_path = is_debug_enabled().then(|| state_dir.join("focus-click.log"));
    let script = focus_script_content(
        notification,
        herdr_bin,
        notifier_bin,
        debug_log_path.as_deref(),
    );

    fs::write(&script_path, script)?;
    make_executable(&script_path)?;

    Ok(script_path)
}

fn focus_script_content(
    notification: &FocusNotification,
    herdr_bin: &str,
    notifier_bin: &str,
    debug_log_path: Option<&Path>,
) -> String {
    match notifier_kind(notifier_bin) {
        "alerter" => alerter_focus_script(
            notification,
            herdr_bin,
            notifier_bin,
            alerter_timeout_secs(),
            activation_command().as_deref(),
            debug_log_path,
        ),
        _ => terminal_notifier_focus_script(
            &notification.pane_id,
            herdr_bin,
            activation_command().as_deref(),
            debug_log_path,
        ),
    }
}

fn alerter_focus_script(
    notification: &FocusNotification,
    herdr_bin: &str,
    notifier_bin: &str,
    timeout_secs: u64,
    activate_command: Option<&str>,
    debug_log_path: Option<&Path>,
) -> String {
    let title_q = shell_quote(&notification.title);
    let body_q = shell_quote(&notification.body);
    let group_q = shell_quote(&notification.group);
    let pane_q = shell_quote(&notification.pane_id);
    let herdr_q = shell_quote(herdr_bin);
    let notifier_q = shell_quote(notifier_bin);
    let timeout_args = if timeout_secs > 0 {
        format!(" --timeout {}", timeout_secs)
    } else {
        String::new()
    };

    let mut script = String::from("#!/bin/sh\n");
    script.push_str(&format!(
        "result=$({notifier} --title {title} --message {body} --group {group} --actions {action} --close-label {close_label}{timeout_args} 2>/dev/null)\n",
        notifier = notifier_q,
        title = title_q,
        body = body_q,
        group = group_q,
        action = shell_quote("Focus"),
        close_label = shell_quote("Dismiss"),
        timeout_args = timeout_args,
    ));

    match debug_log_path {
        Some(log_path) => {
            let log_q = shell_quote(log_path.to_string_lossy().as_ref());
            script.push_str(&format!(
                "printf '%s alerter result=%s\\n' \"$(date -u '+%Y-%m-%dT%H:%M:%SZ')\" \"$result\" >> {log} 2>&1\n",
                log = log_q,
            ));
            script.push_str("status=0\n");
            script.push_str("case \"$result\" in\n");
            script.push_str(&format!(
                "  Focus|@ACTIONCLICKED|@CONTENTCLICKED)\n{activate}    {herdr} agent focus {pane} >> {log} 2>&1\n    status=$?\n    printf '%s focus exited %s\\n' \"$(date -u '+%Y-%m-%dT%H:%M:%SZ')\" \"$status\" >> {log} 2>&1\n    ;;\n",
                activate = activation_script(activate_command, Some(log_q.as_str())),
                herdr = herdr_q,
                pane = pane_q,
                log = log_q,
            ));
            script.push_str("esac\n");
            script.push_str("exit \"$status\"\n");
        }
        None => {
            script.push_str("case \"$result\" in\n");
            script.push_str(&format!(
                "  Focus|@ACTIONCLICKED|@CONTENTCLICKED)\n{activate}    exec {herdr} agent focus {pane}\n    ;;\n",
                activate = activation_script(activate_command, None),
                herdr = herdr_q,
                pane = pane_q,
            ));
            script.push_str("esac\n");
        }
    }

    script
}

fn activation_script(activate_command: Option<&str>, log_q: Option<&str>) -> String {
    let Some(command) = activate_command else {
        return String::new();
    };

    match log_q {
        Some(log_q) => format!(
            "    {command} >> {log} 2>&1\n    activate_status=$?\n    printf '%s activate exited %s\\n' \"$(date -u '+%Y-%m-%dT%H:%M:%SZ')\" \"$activate_status\" >> {log} 2>&1\n",
            command = command,
            log = log_q,
        ),
        None => format!("    {command} >/dev/null 2>&1\n", command = command),
    }
}

fn activation_command() -> Option<String> {
    activate_app().map(activation_command_from)
}

fn activation_command_from(app: String) -> String {
    if app.contains('/') {
        format!("open {}", shell_quote(&app))
    } else {
        format!("open -a {}", shell_quote(&app))
    }
}

fn terminal_notifier_focus_script(
    pane_id: &str,
    herdr_bin: &str,
    activate_command: Option<&str>,
    debug_log_path: Option<&Path>,
) -> String {
    let mut script = String::from("#!/bin/sh\n");

    if let Some(debug_log_path) = debug_log_path {
        let message = format!("focus notification clicked: pane_id={pane_id}");
        script.push_str(&format!(
            "printf '%s %s\\n' \"$(date -u '+%Y-%m-%dT%H:%M:%SZ')\" {} >> {} 2>&1\n",
            shell_quote(&message),
            shell_quote(debug_log_path.to_string_lossy().as_ref())
        ));
        let log_q = shell_quote(debug_log_path.to_string_lossy().as_ref());
        script.push_str(&activation_script(activate_command, Some(log_q.as_str())));
        script.push_str(&format!(
            "{} agent focus {} >> {} 2>&1\n",
            shell_quote(herdr_bin),
            shell_quote(pane_id),
            shell_quote(debug_log_path.to_string_lossy().as_ref())
        ));
        script.push_str("status=$?\n");
        script.push_str(&format!(
            "printf '%s focus command exited with %s\\n' \"$(date -u '+%Y-%m-%dT%H:%M:%SZ')\" \"$status\" >> {} 2>&1\n",
            shell_quote(debug_log_path.to_string_lossy().as_ref())
        ));
        script.push_str("exit \"$status\"\n");
        return script;
    }

    script.push_str(&activation_script(activate_command, None));
    script.push_str(&format!(
        "exec {} agent focus {}\n",
        shell_quote(herdr_bin),
        shell_quote(pane_id)
    ));
    script
}

#[cfg(unix)]
fn make_executable(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(path, permissions)
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> io::Result<()> {
    Ok(())
}

fn send_notification(
    notification: &FocusNotification,
    script_path: &Path,
    notifier_bin: &str,
) -> io::Result<()> {
    match notifier_kind(notifier_bin) {
        "alerter" => spawn_detached_script(script_path),
        _ => send_via_terminal_notifier(notification, script_path, notifier_bin),
    }
}

fn spawn_detached_script(script_path: &Path) -> io::Result<()> {
    let script_str = script_path.to_string_lossy();
    let cmd = format!(
        "nohup sh {} >/dev/null 2>&1 &",
        shell_quote(script_str.as_ref())
    );

    Command::new("sh")
        .arg("-c")
        .arg(&cmd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
}

fn send_via_terminal_notifier(
    notification: &FocusNotification,
    script_path: &Path,
    notifier_bin: &str,
) -> io::Result<()> {
    let script_path_string = script_path.to_string_lossy();
    let execute = format!("sh {}", shell_quote(script_path_string.as_ref()));

    let result = Command::new(notifier_bin)
        .arg("-title")
        .arg(&notification.title)
        .arg("-message")
        .arg(&notification.body)
        .arg("-group")
        .arg(&notification.group)
        .arg("-execute")
        .arg(execute)
        .status();

    match result {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(io::Error::other(format!(
            "terminal-notifier exited with {status}"
        ))),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Err(io::Error::other(
            "no clickable notifier backend found; for reliable click-to-focus install alerter: brew install vjeantet/tap/alerter",
        )),
        Err(err) => Err(err),
    }
}

fn sanitize_group_id(value: &str) -> String {
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

fn shell_quote(value: &str) -> String {
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
    fn builds_blocked_notification_from_event() {
        let json = r#"{
            "event": "pane.agent_status_changed",
            "data": {
                "pane_id": "w1:p3",
                "workspace_id": "herdr",
                "agent_status": "blocked",
                "agent": "codex",
                "display_agent": "Codex",
                "custom_status": "Needs an answer"
            }
        }"#;

        let notification = notification_from_event_json(json).unwrap().unwrap();

        assert_eq!(notification.pane_id, "w1:p3");
        assert_eq!(notification.status, "blocked");
        assert_eq!(notification.title, "Codex needs attention");
        assert!(notification.body.contains("Needs an answer"));
        assert!(notification.body.contains("Location: herdr"));
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
    fn shell_quote_handles_apostrophes() {
        assert_eq!(shell_quote("/tmp/it's ok"), "'/tmp/it'\\''s ok'");
    }

    #[test]
    fn focus_script_can_include_debug_click_log() {
        let notification = FocusNotification {
            pane_id: "pane ' one".to_string(),
            status: "blocked".to_string(),
            title: "x".to_string(),
            body: "y".to_string(),
            group: "g".to_string(),
        };
        let script = focus_script_content(
            &notification,
            "/tmp/herdr bin",
            "/usr/local/bin/terminal-notifier",
            Some(Path::new("/tmp/focus clicks.log")),
        );

        assert!(script.contains("focus notification clicked: pane_id=pane '\\'' one"));
        assert!(script.contains(">> '/tmp/focus clicks.log' 2>&1"));
        assert!(script.contains("'/tmp/herdr bin' agent focus 'pane '\\'' one'"));
        assert!(script.contains("focus command exited with %s"));
        assert!(script.contains("exit \"$status\""));
    }

    fn sample_notification() -> FocusNotification {
        FocusNotification {
            pane_id: "w1:p3".to_string(),
            status: "blocked".to_string(),
            title: "Codex needs attention".to_string(),
            body: "Needs an answer".to_string(),
            group: "herdr-w1-p3".to_string(),
        }
    }

    #[test]
    fn alerter_script_invokes_alerter_and_runs_focus_on_click() {
        let script = focus_script_content(
            &sample_notification(),
            "/usr/local/bin/herdr",
            "/opt/homebrew/bin/alerter",
            None,
        );

        assert!(script.starts_with("#!/bin/sh\n"));
        assert!(script.contains("'/opt/homebrew/bin/alerter' --title 'Codex needs attention'"));
        assert!(script.contains("--message 'Needs an answer'"));
        assert!(script.contains("--group 'herdr-w1-p3'"));
        assert!(script.contains("--actions 'Focus'"));
        assert!(script.contains("--close-label 'Dismiss'"));
        assert!(script.contains("Focus|@ACTIONCLICKED|@CONTENTCLICKED)"));
        assert!(script.contains("exec '/usr/local/bin/herdr' agent focus 'w1:p3'"));
    }

    #[test]
    fn alerter_script_includes_timeout_when_configured() {
        let script = alerter_focus_script(
            &sample_notification(),
            "/usr/local/bin/herdr",
            "/opt/homebrew/bin/alerter",
            120,
            None,
            None,
        );

        assert!(script.contains("--timeout 120"));
    }

    #[test]
    fn alerter_script_omits_timeout_when_zero() {
        let script = alerter_focus_script(
            &sample_notification(),
            "/usr/local/bin/herdr",
            "/opt/homebrew/bin/alerter",
            0,
            None,
            None,
        );

        assert!(!script.contains("--timeout"));
    }

    #[test]
    fn alerter_debug_script_logs_result() {
        let script = alerter_focus_script(
            &sample_notification(),
            "/usr/local/bin/herdr",
            "/opt/homebrew/bin/alerter",
            1800,
            None,
            Some(Path::new("/tmp/click.log")),
        );

        assert!(script.contains("alerter result="));
        assert!(script.contains(">> '/tmp/click.log' 2>&1"));
        assert!(script.contains("status=0\n"));
        assert!(script.contains("focus exited %s"));
        assert!(script.contains("Focus|@ACTIONCLICKED|@CONTENTCLICKED)"));
        assert!(!script.contains("content click ignored"));
    }

    #[test]
    fn alerter_script_includes_activation_when_configured() {
        let script = alerter_focus_script(
            &sample_notification(),
            "/usr/local/bin/herdr",
            "/opt/homebrew/bin/alerter",
            3600,
            Some("open -a 'kitty'"),
            None,
        );

        assert!(script.contains("open -a 'kitty' >/dev/null 2>&1"));
        assert!(script.contains("exec '/usr/local/bin/herdr' agent focus 'w1:p3'"));
    }

    #[test]
    fn terminal_notifier_script_includes_activation_when_configured() {
        let script = terminal_notifier_focus_script(
            "w1:p3",
            "/usr/local/bin/herdr",
            Some("open -a 'kitty'"),
            None,
        );

        assert!(script.contains("open -a 'kitty' >/dev/null 2>&1"));
        assert!(script.contains("exec '/usr/local/bin/herdr' agent focus 'w1:p3'"));
    }

    #[test]
    fn terminal_notifier_debug_script_includes_activation_when_configured() {
        let script = terminal_notifier_focus_script(
            "w1:p3",
            "/usr/local/bin/herdr",
            Some("open -a 'kitty'"),
            Some(Path::new("/tmp/click.log")),
        );

        assert!(script.contains("open -a 'kitty' >> '/tmp/click.log' 2>&1"));
        assert!(script.contains("activate exited %s"));
        assert!(script.contains("'/usr/local/bin/herdr' agent focus 'w1:p3'"));
    }

    #[test]
    fn activation_command_opens_app_names_and_paths() {
        assert_eq!(
            activation_command_from("kitty".to_string()),
            "open -a 'kitty'".to_string()
        );
        assert_eq!(
            activation_command_from("/Applications/kitty.app".to_string()),
            "open '/Applications/kitty.app'".to_string()
        );
    }

    #[test]
    fn notifier_kind_detects_alerter() {
        assert_eq!(notifier_kind("/opt/homebrew/bin/alerter"), "alerter");
        assert_eq!(notifier_kind("alerter"), "alerter");
        assert_eq!(
            notifier_kind("/opt/homebrew/bin/terminal-notifier"),
            "terminal-notifier"
        );
        assert_eq!(notifier_kind("terminal-notifier"), "terminal-notifier");
    }

    #[test]
    fn parse_timeout_secs_defaults_and_overrides() {
        assert_eq!(parse_timeout_secs(None), 3600);
        assert_eq!(parse_timeout_secs(Some("abc".to_string())), 3600);
        assert_eq!(parse_timeout_secs(Some("0".to_string())), 0);
        assert_eq!(parse_timeout_secs(Some("120".to_string())), 120);
    }

    #[test]
    fn parses_plugin_env_file() {
        let values = parse_env_file(
            r#"
            # comment
            HERDR_FOCUS_NOTIFY_ENABLED=1
            HERDR_FOCUS_NOTIFY_NOTIFIER="/opt/homebrew/bin/terminal-notifier"
            HERDR_FOCUS_NOTIFY_STATUSES='blocked,done'
            "#,
        );

        assert_eq!(
            values.get("HERDR_FOCUS_NOTIFY_NOTIFIER").unwrap(),
            "/opt/homebrew/bin/terminal-notifier"
        );
        assert_eq!(
            values.get("HERDR_FOCUS_NOTIFY_STATUSES").unwrap(),
            "blocked,done"
        );
    }
}
