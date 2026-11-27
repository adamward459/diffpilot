//! The comment history: threads of one or more views, filtered by status and grouped by view and file.

use crate::store::{STAGED_BASE, SessionId, SessionKey, SessionThreads, WORKING_TREE_BASE};
use crate::threads::{ReviewThread, Side, ThreadAnchor, ThreadStatus};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StatusFilter {
    #[default]
    Open,
    Resolved,
    All,
}

impl StatusFilter {
    /// Resolved means every status that is not open, so a dismissed thread counts as resolved.
    pub fn matches(self, status: ThreadStatus) -> bool {
        match self {
            Self::Open => status == ThreadStatus::Open,
            Self::Resolved => status != ThreadStatus::Open,
            Self::All => true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Counts {
    pub open: usize,
    pub resolved: usize,
    pub all: usize,
}

impl Counts {
    pub fn of(self, filter: StatusFilter) -> usize {
        match filter {
            StatusFilter::Open => self.open,
            StatusFilter::Resolved => self.resolved,
            StatusFilter::All => self.all,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub session: SessionId,
    pub thread: ReviewThread,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileGroup {
    /// Empty for the comments on the whole change.
    pub path: String,
    pub rows: Vec<Row>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewGroup {
    pub session: SessionId,
    pub key: SessionKey,
    pub current: bool,
    pub files: Vec<FileGroup>,
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Grouped {
    pub counts: Counts,
    pub views: Vec<ViewGroup>,
}

/// Counts every thread that has a comment, then keeps the ones `filter` accepts. The current view comes first,
/// then the views changed most recently. Files are sorted by path with the whole change first, and the threads
/// of a file by line.
pub fn group_threads(sessions: &[SessionThreads], current: SessionId, filter: StatusFilter) -> Grouped {
    let mut counts = Counts::default();
    let mut views = Vec::new();
    for session in sessions {
        let mut by_file: BTreeMap<&str, Vec<Row>> = BTreeMap::new();
        let mut count = 0;
        for thread in session.threads.iter().filter(|thread| !thread.comments.is_empty()) {
            counts.all += 1;
            if thread.status == ThreadStatus::Open {
                counts.open += 1;
            } else {
                counts.resolved += 1;
            }
            if filter.matches(thread.status) {
                count += 1;
                by_file.entry(thread.anchor.path()).or_default().push(Row {
                    session: session.session,
                    thread: thread.clone(),
                });
            }
        }
        if count == 0 {
            continue;
        }
        let files = by_file
            .into_iter()
            .map(|(path, mut rows)| {
                rows.sort_by_key(|row| (line_of(&row.thread), row.thread.id));
                FileGroup {
                    path: path.to_string(),
                    rows,
                }
            })
            .collect();
        views.push((
            session.latest,
            ViewGroup {
                session: session.session,
                key: session.key.clone(),
                current: session.session == current,
                files,
                count,
            },
        ));
    }
    views.sort_by_key(|(latest, view)| (!view.current, std::cmp::Reverse(*latest), view.session));
    Grouped {
        counts,
        views: views.into_iter().map(|(_, view)| view).collect(),
    }
}

fn line_of(thread: &ReviewThread) -> u32 {
    match thread.anchor {
        ThreadAnchor::Line { line, .. } => line,
        _ => 0,
    }
}

/// `L12` or `L12–18`, with `(old)` for the old side. A thread on a file or on the whole change has none.
pub fn line_label(thread: &ReviewThread) -> Option<String> {
    let ThreadAnchor::Line { side, line, .. } = &thread.anchor else {
        return None;
    };
    let range = match thread.end_line {
        Some(end) if end > *line => format!("L{line}\u{2013}{end}"),
        _ => format!("L{line}"),
    };
    Some(match side {
        Side::Old => format!("{range} (old)"),
        Side::New => range,
    })
}

pub fn view_label(key: &SessionKey) -> String {
    match key.base.as_str() {
        WORKING_TREE_BASE => format!("{} \u{b7} working tree", key.branch),
        STAGED_BASE => format!("{} \u{b7} staged", key.branch),
        base => format!("{} vs {base}", key.branch),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::threads::{CommentSource, ThreadComment, ThreadId};

    fn thread(id: u32, anchor: ThreadAnchor, status: ThreadStatus) -> ReviewThread {
        ReviewThread {
            id: ThreadId(id),
            anchor,
            comments: vec![ThreadComment {
                id: 1,
                author: "You".into(),
                body: format!("comment {id}"),
                is_streaming: false,
                source: CommentSource::User,
                pending: false,
            }],
            finding: None,
            end_line: None,
            anchor_text: None,
            status,
            addressed: None,
        }
    }

    fn session(id: i64, branch: &str, latest: i64, threads: Vec<ReviewThread>) -> SessionThreads {
        SessionThreads {
            session: SessionId(id),
            key: SessionKey::new("/r", branch, "main"),
            latest,
            threads,
        }
    }

    fn ids(view: &ViewGroup) -> Vec<Vec<u32>> {
        view.files
            .iter()
            .map(|file| file.rows.iter().map(|row| row.thread.id.0).collect())
            .collect()
    }

    fn sample() -> Vec<SessionThreads> {
        vec![
            session(
                1,
                "old",
                5,
                vec![thread(1, ThreadAnchor::file("z.rs"), ThreadStatus::Open)],
            ),
            session(
                2,
                "feature",
                1,
                vec![
                    thread(2, ThreadAnchor::line("b.rs", Side::New, 30), ThreadStatus::Open),
                    thread(3, ThreadAnchor::line("b.rs", Side::New, 4), ThreadStatus::Resolved),
                    thread(4, ThreadAnchor::file("a.rs"), ThreadStatus::Dismissed),
                    thread(5, ThreadAnchor::Change, ThreadStatus::Open),
                ],
            ),
            session(
                3,
                "newer",
                9,
                vec![thread(6, ThreadAnchor::file("a.rs"), ThreadStatus::Resolved)],
            ),
        ]
    }

    #[test]
    fn counts_cover_every_thread_and_resolved_includes_dismissed() {
        let grouped = group_threads(&sample(), SessionId(2), StatusFilter::Open);
        assert_eq!(
            grouped.counts,
            Counts {
                open: 3,
                resolved: 3,
                all: 6
            }
        );
        let count_when = |filter| group_threads(&sample(), SessionId(2), filter).counts;
        assert_eq!(count_when(StatusFilter::Resolved), grouped.counts);
        assert_eq!(grouped.counts.of(StatusFilter::Resolved), 3);
    }

    #[test]
    fn the_filter_keeps_only_matching_threads_and_drops_views_left_empty() {
        let open = group_threads(&sample(), SessionId(2), StatusFilter::Open);
        assert_eq!(open.views.iter().map(|view| view.session.0).collect::<Vec<_>>(), [2, 1]);
        assert_eq!(ids(&open.views[0]), [vec![5], vec![2]]);

        let resolved = group_threads(&sample(), SessionId(2), StatusFilter::Resolved);
        assert_eq!(
            resolved.views.iter().map(|view| view.session.0).collect::<Vec<_>>(),
            [2, 3]
        );
        assert_eq!(ids(&resolved.views[0]), [vec![4], vec![3]]);
        assert_eq!(resolved.views[0].count, 2);

        let all = group_threads(&sample(), SessionId(2), StatusFilter::All);
        assert_eq!(all.views.iter().map(|view| view.count).sum::<usize>(), 6);
    }

    #[test]
    fn the_current_view_comes_first_then_the_most_recent() {
        let grouped = group_threads(&sample(), SessionId(2), StatusFilter::All);
        let order: Vec<_> = grouped
            .views
            .iter()
            .map(|view| (view.session.0, view.current))
            .collect();
        assert_eq!(order, [(2, true), (3, false), (1, false)]);
    }

    #[test]
    fn a_view_without_a_current_session_marks_none() {
        let grouped = group_threads(&sample(), SessionId(99), StatusFilter::All);
        assert!(grouped.views.iter().all(|view| !view.current));
        assert_eq!(grouped.views[0].session, SessionId(3));
    }

    #[test]
    fn files_sort_by_path_with_the_whole_change_first_and_threads_by_line() {
        let grouped = group_threads(&sample(), SessionId(2), StatusFilter::All);
        let paths: Vec<_> = grouped.views[0].files.iter().map(|file| file.path.as_str()).collect();
        assert_eq!(paths, ["", "a.rs", "b.rs"]);
        assert_eq!(ids(&grouped.views[0])[2], [3, 2]);
    }

    #[test]
    fn a_thread_without_a_comment_is_not_listed_or_counted() {
        let mut draft = thread(7, ThreadAnchor::file("a.rs"), ThreadStatus::Open);
        draft.comments.clear();
        let grouped = group_threads(
            &[session(2, "feature", 1, vec![draft])],
            SessionId(2),
            StatusFilter::All,
        );
        assert_eq!(grouped, Grouped::default());
    }

    #[test]
    fn line_labels_show_the_range_and_the_old_side() {
        let mut line = thread(1, ThreadAnchor::line("a.rs", Side::New, 12), ThreadStatus::Open);
        assert_eq!(line_label(&line).as_deref(), Some("L12"));
        line.end_line = Some(18);
        assert_eq!(line_label(&line).as_deref(), Some("L12\u{2013}18"));
        line.anchor = ThreadAnchor::line("a.rs", Side::Old, 12);
        assert_eq!(line_label(&line).as_deref(), Some("L12\u{2013}18 (old)"));
        line.end_line = Some(12);
        assert_eq!(line_label(&line).as_deref(), Some("L12 (old)"));
        assert_eq!(
            line_label(&thread(2, ThreadAnchor::file("a.rs"), ThreadStatus::Open)),
            None
        );
    }

    #[test]
    fn a_view_is_named_by_its_branch_and_base() {
        let name = |base: &str| view_label(&SessionKey::new("/r", "feature", base));
        assert_eq!(name("main"), "feature vs main");
        assert_eq!(name(WORKING_TREE_BASE), "feature \u{b7} working tree");
        assert_eq!(name(STAGED_BASE), "feature \u{b7} staged");
    }
}
