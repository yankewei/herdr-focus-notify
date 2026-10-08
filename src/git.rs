//! How much the pane's directory has changed versus `HEAD`.
//!
//! The branch next to these counts comes from Herdr, not from here: see
//! `focus::worktree_branch`.

use std::time::Instant;

use crate::util::command_stdout_until;

/// Longest branch the subtitle shows before it is middle-truncated. macOS gives
/// the subtitle a single line and truncates its end, so an untruncated branch
/// would push the line counts out of view, and the numbers are the part that
/// cannot be guessed from the branch name.
const MAX_BRANCH_CHARS: usize = 24;

/// Characters kept from the head of a truncated branch. The head identifies the
/// work (`feature/…`), the tail separates siblings (`…-layout`).
const BRANCH_HEAD_CHARS: usize = 12;

/// Tracked changes versus `HEAD` in a pane's directory: a dirty tree, with the
/// lines it adds and removes.
///
/// Both counts can be zero on a dirty tree. A binary file or an executable bit
/// changes no text lines, so the counts describe the size of the change and
/// never decide whether there is one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Changes {
    pub(crate) inserted: usize,
    pub(crate) deleted: usize,
}

/// The git state a notification shows for a pane's directory, in the short form
/// shell prompts and diffstats share: `main* · +120/-45`.
///
/// A dirty tree whose changes touch no text lines keeps its marker but shows no
/// counts, since `+0/-0` would read as clean.
pub(crate) fn label(branch: &str, changes: Option<Changes>) -> String {
    let branch = display_branch(branch);

    match changes {
        None => branch,
        Some(Changes {
            inserted: 0,
            deleted: 0,
        }) => format!("{branch}*"),
        Some(Changes { inserted, deleted }) => format!("{branch}* · +{inserted}/-{deleted}"),
    }
}

/// Tracked changes versus `HEAD`; None when the tree is clean, or when git
/// cannot answer by `deadline`, which is the same best-effort contract the rest
/// of the enrichment follows.
///
/// Only tracked changes count, so the `*` marker and the numbers always
/// describe the same set of changes, and they are not a branch's total: work
/// the pane has already committed shows no counts.
pub(crate) fn changes(cwd: &str, deadline: Instant) -> Option<Changes> {
    // `--no-optional-locks` keeps the probe from refreshing the index. Git
    // takes `.git/index.lock` to do that, and a notification fires exactly when
    // an agent is likely to be running git in the same repository.
    //
    // `LC_ALL=C` keeps the summary line parseable: git translates it, and a
    // translated `Dateien geändert` carries no `(+)`/`(-)` suffix to key on.
    let shortstat = command_stdout_until(
        "git",
        &[
            "--no-optional-locks",
            "-C",
            cwd,
            "diff",
            "--shortstat",
            "HEAD",
        ],
        &[("LC_ALL", "C")],
        deadline,
    )?;

    changes_from_shortstat(&shortstat)
}

/// ` 3 files changed, 12 insertions(+), 4 deletions(-)`, one clause per kind of
/// count, and either clause is left out when its count is zero. A clean tree
/// prints nothing at all, so any summary line means the tree is dirty, even
/// ` 1 file changed, 0 insertions(+), 0 deletions(-)` for a binary file.
///
/// The counts are keyed on the `(+)`/`(-)` suffixes rather than the words,
/// which are pluralized (`1 insertion(+)`) and translated.
fn changes_from_shortstat(shortstat: &str) -> Option<Changes> {
    if shortstat.trim().is_empty() {
        return None;
    }

    let mut changes = Changes {
        inserted: 0,
        deleted: 0,
    };
    for clause in shortstat.split(',') {
        let clause = clause.trim();
        let Some(count) = clause
            .split_whitespace()
            .next()
            .and_then(|value| value.parse::<usize>().ok())
        else {
            continue;
        };

        if clause.ends_with("(+)") {
            changes.inserted += count;
        } else if clause.ends_with("(-)") {
            changes.deleted += count;
        }
    }

    Some(changes)
}

/// Middle-truncates a branch too long for the subtitle's one line.
///
/// Counted in `chars`, not bytes: slicing a branch with a non-ASCII character
/// at a byte offset would panic.
fn display_branch(branch: &str) -> String {
    let chars: Vec<char> = branch.chars().collect();
    if chars.len() <= MAX_BRANCH_CHARS {
        return branch.to_string();
    }

    // One of the budgeted characters is the ellipsis.
    let tail_chars = MAX_BRANCH_CHARS - BRANCH_HEAD_CHARS - 1;
    let mut truncated: String = chars[..BRANCH_HEAD_CHARS].iter().collect();
    truncated.push('…');
    truncated.extend(&chars[chars.len() - tail_chars..]);
    truncated
}

#[cfg(test)]
mod tests {
    use super::*;

    fn changes(inserted: usize, deleted: usize) -> Option<Changes> {
        Some(Changes { inserted, deleted })
    }

    #[test]
    fn labels_branch_dirty_marker_and_line_counts_like_a_prompt_does() {
        assert_eq!(label("main", changes(120, 45)), "main* · +120/-45");
        assert_eq!(label("feature/api", changes(3, 0)), "feature/api* · +3/-0");
        // Dirty without changed text lines (a binary file, an executable bit):
        // the marker stays, and `+0/-0` is not shown.
        assert_eq!(label("main", changes(0, 0)), "main*");
        // A clean tree, or git not answering, carries neither marker nor counts.
        assert_eq!(label("main", None), "main");
    }

    #[test]
    fn middle_truncates_a_long_branch_to_keep_the_counts_on_the_line() {
        assert_eq!(display_branch("main"), "main");
        assert_eq!(display_branch("feature/JIRA-123-authentication-layout"), {
            let truncated = "feature/JIRA…tion-layout";
            assert_eq!(truncated.chars().count(), MAX_BRANCH_CHARS);
            truncated
        });
        // Counted in chars: a multi-byte branch must not panic on a slice.
        let long_unicode = "feature/ünicode-branch-name-that-is-long";
        assert_eq!(
            display_branch(long_unicode).chars().count(),
            MAX_BRANCH_CHARS
        );
    }

    #[test]
    fn sums_shortstat_by_its_count_suffixes() {
        assert_eq!(
            changes_from_shortstat(" 3 files changed, 12 insertions(+), 4 deletions(-)"),
            changes(12, 4)
        );
        // Pluralization varies, and either clause is omitted at zero.
        assert_eq!(
            changes_from_shortstat(" 1 file changed, 1 insertion(+)"),
            changes(1, 0)
        );
        assert_eq!(
            changes_from_shortstat(" 2 files changed, 5 deletions(-)"),
            changes(0, 5)
        );
        // A clean tree prints nothing at all.
        assert_eq!(changes_from_shortstat(""), None);
        assert_eq!(changes_from_shortstat("\n"), None);
    }

    #[test]
    fn a_summary_without_changed_lines_still_means_a_dirty_tree() {
        // What `git diff --shortstat HEAD` prints for a changed binary file or
        // a changed executable bit.
        assert_eq!(
            changes_from_shortstat(" 1 file changed, 0 insertions(+), 0 deletions(-)\n"),
            changes(0, 0)
        );
        assert_eq!(changes_from_shortstat(" 1 file changed\n"), changes(0, 0));
    }
}
