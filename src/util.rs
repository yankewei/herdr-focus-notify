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
