#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FocusNotification {
    pub(crate) pane_id: String,
    pub(crate) status: String,
    pub(crate) title: String,
    pub(crate) body: String,
    /// Secondary line shown under the message (alerter `--subtitle`): the agent,
    /// then the pane's git state once it is known.
    pub(crate) subtitle: Option<String>,
    pub(crate) group: String,
    pub(crate) app_icon: Option<String>,
}
