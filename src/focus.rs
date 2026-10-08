use serde::de::DeserializeOwned;
use serde::Deserialize;
use std::env;
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use crate::notification::{FocusNotification, PaneLabels};
use crate::util::{command_stdout, command_stdout_until, sanitize_group_id, workspace_of};

/// How long a notification click waits for Herdr's pane focus response. A real
/// click runs detached, where an unanswered request would leave a stray process
/// behind; `--test` runs in the foreground and would hang the action outright.
const FOCUS_SOCKET_TIMEOUT: Duration = Duration::from_secs(5);

/// The `{"result": ...}` wrapper around every Herdr CLI reply.
#[derive(Debug, Deserialize)]
struct Envelope<T> {
    result: Option<T>,
}

/// The result inside a Herdr CLI reply. None for a reply that cannot be read:
/// every caller treats that like a Herdr that did not answer.
fn herdr_result<T: DeserializeOwned>(json: &str) -> Option<T> {
    serde_json::from_str::<Envelope<T>>(json).ok()?.result
}

#[derive(Debug, Deserialize)]
struct PaneListResult {
    panes: Vec<AgentInfo>,
}

#[derive(Debug, Deserialize)]
struct AgentGetResult {
    agent: Option<AgentInfo>,
}

#[derive(Debug, Deserialize)]
struct AgentInfo {
    focused: bool,
    pane_id: Option<String>,
    cwd: Option<String>,
    tab_id: Option<String>,
    terminal_title_stripped: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TabGetResult {
    tab: Option<TabInfo>,
}

#[derive(Debug, Deserialize)]
struct TabInfo {
    label: Option<String>,
    number: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct WorkspaceListResult {
    workspaces: Vec<WorkspaceInfo>,
}

#[derive(Debug, Deserialize)]
struct WorkspaceInfo {
    workspace_id: String,
    label: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WorktreeListResult {
    #[serde(default)]
    worktrees: Vec<WorktreeInfo>,
}

#[derive(Debug, Deserialize)]
struct WorktreeInfo {
    path: String,
    #[serde(default)]
    is_detached: bool,
    branch: Option<String>,
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
        subtitle: None,
        group: format!("herdr-{}", sanitize_group_id(&pane_id)),
        app_icon: None,
    }
}

/// Everything one `herdr agent get` answers about a pane: whether Herdr calls it
/// the focused one, and the handful of fields a notification names. The default
/// is what an unanswered lookup amounts to: an unfocused pane, so the
/// notification is sent, with nothing to add to it.
#[derive(Default)]
pub(crate) struct PaneDetails {
    pub(crate) focused: bool,
    cwd: Option<String>,
    tab_id: Option<String>,
    terminal_title: Option<String>,
}

/// One `herdr agent get`, so the decision to notify at all and the
/// notification's own row read the pane once. None when Herdr cannot answer by
/// `deadline`, or answers about another pane.
pub(crate) fn pane_details(
    pane_id: &str,
    herdr_bin: &str,
    deadline: Instant,
) -> Option<PaneDetails> {
    let json = command_stdout_until(herdr_bin, &["agent", "get", pane_id], &[], deadline)?;
    pane_details_from_get_json(&json, pane_id)
}

/// The workspace, the tab, the terminal title, and the git state a notification
/// names about its pane, for a notification that is going to be shown.
pub(crate) fn pane_labels(
    pane_id: &str,
    details: PaneDetails,
    herdr_bin: &str,
    deadline: Instant,
) -> PaneLabels {
    let workspace = workspace_label(pane_id, herdr_bin, deadline);
    let tab = details
        .tab_id
        .as_deref()
        .and_then(|tab_id| tab_label(tab_id, herdr_bin, deadline));
    // Without a branch there is nothing to attribute the changes to, so the
    // git probe is not worth its time: a detached `HEAD` or a directory outside
    // a repository contributes nothing rather than a bare `+120/-45` that
    // explains neither where nor what.
    let git = details.cwd.as_deref().and_then(|cwd| {
        let branch = worktree_branch(cwd, herdr_bin, deadline)?;
        Some(crate::git::label(
            &branch,
            crate::git::changes(cwd, deadline),
        ))
    });

    PaneLabels {
        workspace,
        tab,
        terminal_title: details.terminal_title,
        git,
    }
}

/// The label of the workspace that holds the pane. Like every lookup behind a
/// notification's text it is best-effort: None when Herdr cannot answer by
/// `deadline`, and the notification then keeps the event's own copy.
fn workspace_label(pane_id: &str, herdr_bin: &str, deadline: Instant) -> Option<String> {
    let json = command_stdout_until(herdr_bin, &["workspace", "list"], &[], deadline)?;
    workspace_label_from_list_json(&json, workspace_of(pane_id))
}

/// The label of the tab that holds the pane; an unnamed tab is known by its
/// number. Best-effort, like `workspace_label`.
fn tab_label(tab_id: &str, herdr_bin: &str, deadline: Instant) -> Option<String> {
    let json = command_stdout_until(herdr_bin, &["tab", "get", tab_id], &[], deadline)?;
    tab_label_from_get_json(&json)
}

/// The branch the pane's directory is on, from Herdr's own worktree view: it is
/// the source the Agent sidebar uses, and it still names the branch of a
/// repository with no commits yet, where `git status` answers
/// `## No commits yet on main`.
///
/// Best-effort: None outside a worktree, on a detached `HEAD`, or when Herdr
/// cannot answer by `deadline`.
fn worktree_branch(cwd: &str, herdr_bin: &str, deadline: Instant) -> Option<String> {
    let json = command_stdout_until(
        herdr_bin,
        &["worktree", "list", "--cwd", cwd],
        &[],
        deadline,
    )?;
    branch_from_worktree_list_json(&json, cwd)
}

fn branch_from_worktree_list_json(json: &str, cwd: &str) -> Option<String> {
    let result: WorktreeListResult = herdr_result(json)?;
    // The branch belongs to the worktree that owns the directory, matched by
    // path rather than by the checkout Herdr resolved the query to:
    // `source_checkout_path` names the repository's main checkout even when the
    // query came from a linked worktree, so matching it reports `main` for an
    // agent working on its own branch.
    //
    // A directory can sit below its worktree root, and one worktree can live
    // inside another checkout's tree (`<repo>/.tools/worktrees/...`), so the
    // owner is the longest path the directory starts with. Both sides are
    // canonicalized first: the same directory is `/var/...` in one field and
    // `/private/var/...` in the other, and a string comparison drops the branch
    // without a word.
    let cwd = canonicalized(cwd);
    let owner = result
        .worktrees
        .iter()
        .map(|worktree| (canonicalized(&worktree.path), worktree))
        .filter(|(path, _)| cwd == *path || cwd.starts_with(path))
        .max_by_key(|(path, _)| path.as_os_str().len())
        .map(|(_, worktree)| worktree);

    owner
        .filter(|worktree| !worktree.is_detached)
        .and_then(|worktree| worktree.branch.clone())
}

/// A path resolved for comparison, falling back to the value as Herdr spelled
/// it: a path that no longer exists still compares as itself.
fn canonicalized(path: &str) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| PathBuf::from(path))
}

fn pane_details_from_get_json(json: &str, expected_pane_id: &str) -> Option<PaneDetails> {
    herdr_result::<AgentGetResult>(json)?
        .agent
        // A stale snapshot can describe a pane Herdr has since replaced. None of
        // it applies then: not its focus, and not its directory, tab or title,
        // which would name another pane on a notification that focuses this one.
        .filter(|agent| agent.pane_id.as_deref() == Some(expected_pane_id))
        .map(|agent| PaneDetails {
            focused: agent.focused,
            cwd: agent.cwd,
            tab_id: agent.tab_id,
            terminal_title: agent.terminal_title_stripped,
        })
}

fn tab_label_from_get_json(json: &str) -> Option<String> {
    let tab = herdr_result::<TabGetResult>(json)?.tab?;

    // Herdr already labels an unnamed tab with its number, as the sidebar
    // shows it; the number stands in should a reply ever leave the label out,
    // since it is what tells two unnamed tabs of one workspace apart.
    tab.label
        .filter(|label| !label.trim().is_empty())
        .or_else(|| tab.number.map(|number| number.to_string()))
}

fn workspace_label_from_list_json(json: &str, workspace_id: &str) -> Option<String> {
    herdr_result::<WorkspaceListResult>(json)?
        .workspaces
        .into_iter()
        .find(|workspace| workspace.workspace_id == workspace_id)?
        .label
}

/// Activates the workspace's bound terminal and focuses the target pane.
///
/// Without a terminal binding there is no visible app to activate, so a
/// notification click is intentionally a no-op. A binding is checked again
/// here at click time because generated notification scripts can outlive the
/// state that existed when they were written.
pub(crate) fn focus_pane(pane_id: &str) -> Result<(), String> {
    let workspace = workspace_of(pane_id);
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

/// Whether a notification for this pane is worth showing at all.
///
/// `focused` comes from the caller's `pane_details`, so one `herdr agent get`
/// serves both this decision and the notification's own row.
pub(crate) fn notification_decision(pane_id: &str, focused: bool) -> NotificationDecision {
    // An unfocused pane always sends, so the binding and the frontmost app
    // are not worth looking up.
    if !focused {
        return NotificationDecision::Send;
    }
    notification_decision_from_bundles(
        crate::state::remembered_terminal(workspace_of(pane_id)),
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
    focused_pane_id_from_pane_list_json(&json)
}

fn focused_pane_id_from_pane_list_json(json: &str) -> Option<String> {
    herdr_result::<PaneListResult>(json)?
        .panes
        .into_iter()
        .find_map(|agent| {
            if !agent.focused {
                return None;
            }
            agent
                .pane_id
                .map(|pane_id| pane_id.trim().to_string())
                .filter(|pane_id| !pane_id.is_empty())
        })
}

/// The workspaces that currently exist, derived from `herdr pane list`.
///
/// Returns `None` when the workspace set cannot be trusted (command failure,
/// empty output, or no panes at all) so callers never prune bindings against
/// an empty world — e.g. right after Herdr itself started.
pub(crate) fn live_workspace_ids(herdr_bin: &str) -> Option<Vec<String>> {
    let json = command_stdout(herdr_bin, &["pane", "list"])?;
    live_workspace_ids_from_pane_list_json(&json)
}

fn live_workspace_ids_from_pane_list_json(json: &str) -> Option<Vec<String>> {
    let mut seen: Vec<String> = Vec::new();
    for agent in herdr_result::<PaneListResult>(json)?.panes {
        if let Some(pane_id) = agent.pane_id {
            if let Some(workspace) = crate::util::workspace_id_from_pane_id(pane_id.trim()) {
                if !seen.iter().any(|value| value == workspace) {
                    seen.push(workspace.to_string());
                }
            }
        }
    }
    (!seen.is_empty()).then_some(seen)
}

/// The decision for a pane Herdr calls focused: whether the user can see it
/// depends on the frontmost app being the terminal bound to its workspace.
fn notification_decision_from_bundles(
    bound_terminal: Option<String>,
    frontmost: Option<String>,
) -> NotificationDecision {
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
    fn reads_pane_details_from_agent_get_json() {
        let json = r#"{"result":{"agent":{"focused":true,"pane_id":"w1:p3",
            "cwd":"/tmp/repo","tab_id":"w1:t7","terminal_title_stripped":"Tidy up the parser tests"}}}"#;

        let details = pane_details_from_get_json(json, "w1:p3").unwrap();

        assert!(details.focused);
        assert_eq!(details.cwd.as_deref(), Some("/tmp/repo"));
        assert_eq!(details.tab_id.as_deref(), Some("w1:t7"));
        assert_eq!(
            details.terminal_title.as_deref(),
            Some("Tidy up the parser tests")
        );
        // The report has to describe the pane that was asked about, or none of
        // it is used.
        assert!(pane_details_from_get_json(json, "w1:p9").is_none());
        let without_pane_id = r#"{"result":{"agent":{"focused":true,"cwd":"/tmp/repo"}}}"#;
        assert!(pane_details_from_get_json(without_pane_id, "w1:p3").is_none());
        assert!(pane_details_from_get_json("not json", "w1:p3").is_none());
    }

    #[test]
    fn reads_the_branch_of_the_worktree_that_owns_the_directory() {
        // Mirrors a real reply: the source checkout is the main checkout even
        // though the query came from a linked worktree, and one worktree lives
        // inside the main checkout's own tree.
        let json = r#"{
            "id": "cli:worktree:list",
            "result": {
                "source": {"repo_root": "/repo", "source_checkout_path": "/repo"},
                "worktrees": [
                    {"branch": "main", "is_detached": false, "path": "/repo"},
                    {"branch": "feature/api", "is_detached": false, "path": "/worktrees/api"},
                    {
                        "branch": null,
                        "is_detached": true,
                        "path": "/repo/.tools/worktrees/spike"
                    }
                ]
            }
        }"#;

        assert_eq!(
            branch_from_worktree_list_json(json, "/repo").as_deref(),
            Some("main")
        );
        // A directory below the worktree root still belongs to that worktree.
        assert_eq!(
            branch_from_worktree_list_json(json, "/repo/src/deep").as_deref(),
            Some("main")
        );
        // A linked worktree reports its own branch, not the main checkout's.
        assert_eq!(
            branch_from_worktree_list_json(json, "/worktrees/api").as_deref(),
            Some("feature/api")
        );
        // The nested worktree wins over the checkout it sits inside, and it is
        // detached, so there is no branch to show.
        assert_eq!(
            branch_from_worktree_list_json(json, "/repo/.tools/worktrees/spike"),
            None
        );
        // A directory outside every worktree has no branch either.
        assert_eq!(branch_from_worktree_list_json(json, "/elsewhere"), None);
    }

    #[test]
    fn reads_the_tab_label_from_tab_get_json() {
        let json = r#"{"result":{"tab":{"label":"status","number":7}}}"#;

        assert_eq!(tab_label_from_get_json(json).as_deref(), Some("status"));
        // An unnamed tab is known by its number.
        assert_eq!(
            tab_label_from_get_json(r#"{"result":{"tab":{"label":"7","number":7}}}"#).as_deref(),
            Some("7")
        );
        assert_eq!(
            tab_label_from_get_json(r#"{"result":{"tab":{"number":7}}}"#).as_deref(),
            Some("7")
        );
        assert_eq!(
            tab_label_from_get_json(r#"{"result":{"tab":{"label":" ","number":7}}}"#).as_deref(),
            Some("7")
        );
        assert_eq!(tab_label_from_get_json(r#"{"result":{"tab":{}}}"#), None);
    }

    #[test]
    fn reads_the_workspace_label_from_workspace_list_json() {
        let json = r#"{"result":{"workspaces":[
            {"workspace_id":"w1","label":"sample-repo"},
            {"workspace_id":"w2"}]}}"#;

        assert_eq!(
            workspace_label_from_list_json(json, "w1").as_deref(),
            Some("sample-repo")
        );
        assert_eq!(workspace_label_from_list_json(json, "w2"), None);
        assert_eq!(workspace_label_from_list_json(json, "w9"), None);
    }

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
            focused_pane_id_from_pane_list_json(json),
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
            live_workspace_ids_from_pane_list_json(json),
            Some(vec!["w1".to_string(), "w2".to_string()])
        );

        // No panes at all is untrustworthy for pruning.
        let empty = r#"{"id":"cli:pane:list","result":{"panes":[]}}"#;
        assert_eq!(live_workspace_ids_from_pane_list_json(empty), None);
    }

    #[test]
    fn decides_when_to_skip_or_monitor_notifications() {
        // Pane focused + frontmost matches the bound terminal -> skip.
        assert_eq!(
            notification_decision_from_bundles(
                Some("com.example.Herdr".to_string()),
                Some("com.example.Herdr".to_string())
            ),
            NotificationDecision::Skip
        );
        // Pane focused + frontmost matches the workspace binding -> skip.
        assert_eq!(
            notification_decision_from_bundles(
                Some("com.googlecode.iterm2".to_string()),
                Some("com.googlecode.iterm2".to_string())
            ),
            NotificationDecision::Skip
        );
        // Pane focused + frontmost outside the bound terminal -> notify, with
        // a visibility monitor since a terminal is bound.
        assert_eq!(
            notification_decision_from_bundles(
                Some("com.example.Herdr".to_string()),
                Some("com.apple.Terminal".to_string())
            ),
            NotificationDecision::SendWithVisibilityMonitor
        );
        // Pane focused + frontmost is a non-terminal app and a terminal is
        // bound -> notify with a visibility monitor so it auto-dismisses.
        assert_eq!(
            notification_decision_from_bundles(
                Some("com.example.Herdr".to_string()),
                Some("com.google.Chrome".to_string())
            ),
            NotificationDecision::SendWithVisibilityMonitor
        );
        // Pane focused + frontmost with no bound terminal -> notify.
        assert_eq!(
            notification_decision_from_bundles(None, Some("com.google.Chrome".to_string())),
            NotificationDecision::Send
        );
        // Pane focused + frontmost unknown -> notify (conservative).
        assert_eq!(
            notification_decision_from_bundles(Some("com.example.Herdr".to_string()), None),
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
