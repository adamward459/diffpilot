//! Follows a line thread to where its code is in the current diff.
//!
//! Only lines the diff shows can be matched, so a thread whose code left the hunks reads as outdated.

use crate::diff::{FileDiff, LineKind};
use crate::threads::{ReviewThread, Side, ThreadAnchor};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    Current,
    Moved { line: u32, end_line: Option<u32> },
    Outdated,
    FileGone,
}

fn side_lines(file: &FileDiff, side: Side) -> Vec<(u32, &str)> {
    let mut lines: Vec<(u32, &str)> = file
        .hunks
        .iter()
        .flat_map(|hunk| &hunk.lines)
        .filter_map(|line| {
            match (side, line.kind) {
                (Side::New, LineKind::Context | LineKind::Added) => line.new,
                (Side::Old, LineKind::Context | LineKind::Deleted) => line.old,
                _ => None,
            }
            .map(|number| (number, line.text.as_str()))
        })
        .collect();
    lines.sort_by_key(|(number, _)| *number);
    lines
}

fn file_of<'a>(files: &'a [FileDiff], path: &str) -> Option<&'a FileDiff> {
    files
        .iter()
        .find(|file| file.path == path)
        .or_else(|| files.iter().find(|file| file.old_path.as_deref() == Some(path)))
}

fn last_line(thread: &ReviewThread, line: u32) -> u32 {
    thread.end_line.filter(|end| *end >= line).unwrap_or(line)
}

/// The text of lines `start..=end`, when the diff shows all of them.
pub fn lines_text(file: &FileDiff, side: Side, start: u32, end: u32) -> Option<String> {
    let lines = side_lines(file, side);
    let texts: Option<Vec<&str>> = (start..=end.max(start))
        .map(|number| {
            lines
                .binary_search_by_key(&number, |(shown, _)| *shown)
                .ok()
                .map(|at| lines[at].1)
        })
        .collect();
    texts.map(|texts| texts.join("\n"))
}

/// The text of the thread's lines, when the diff shows all of them.
pub fn text_of(thread: &ReviewThread, files: &[FileDiff]) -> Option<String> {
    let ThreadAnchor::Line { path, side, line } = &thread.anchor else {
        return None;
    };
    lines_text(file_of(files, path)?, *side, *line, last_line(thread, *line))
}

pub fn place(thread: &ReviewThread, files: &[FileDiff]) -> Placement {
    let (path, side, line) = match &thread.anchor {
        ThreadAnchor::Change => return Placement::Current,
        ThreadAnchor::File { path } => {
            return if file_of(files, path).is_some() {
                Placement::Current
            } else {
                Placement::FileGone
            };
        }
        ThreadAnchor::Line { path, side, line } => (path, *side, *line),
    };
    let Some(file) = file_of(files, path) else {
        return Placement::FileGone;
    };
    let lines = side_lines(file, side);
    let shown = |number: u32| lines.binary_search_by_key(&number, |(shown, _)| *shown).is_ok();
    let Some(text) = thread.anchor_text.as_deref().filter(|text| !text.is_empty()) else {
        let any = (line..=last_line(thread, line)).any(shown);
        return if any { Placement::Current } else { Placement::Outdated };
    };
    let block: Vec<&str> = text.split('\n').collect();
    let matches_at = |start: usize| {
        lines.get(start..start + block.len()).is_some_and(|window| {
            window.iter().zip(&block).all(|((_, shown), wanted)| shown == wanted)
                && window.windows(2).all(|pair| pair[1].0 == pair[0].0 + 1)
        })
    };
    let starts: Vec<u32> = (0..lines.len())
        .filter(|start| matches_at(*start))
        .map(|start| lines[start].0)
        .collect();
    if starts.contains(&line) {
        return Placement::Current;
    }
    match starts.iter().min_by_key(|start| start.abs_diff(line)) {
        Some(&start) => Placement::Moved {
            line: start,
            end_line: thread.end_line.map(|_| start + block.len() as u32 - 1),
        },
        None => Placement::Outdated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::changes::FileStatus;
    use crate::diff::{DiffLine, Hunk};
    use crate::threads::{ThreadId, ThreadStatus};

    fn file(path: &str, lines: &[(LineKind, Option<u32>, Option<u32>, &str)]) -> FileDiff {
        let mut file = FileDiff::new(path, FileStatus::Modified);
        file.hunks.push(Hunk {
            header: "@@".into(),
            lines: lines
                .iter()
                .map(|(kind, old, new, text)| DiffLine {
                    kind: *kind,
                    old: *old,
                    new: *new,
                    text: text.to_string(),
                })
                .collect(),
        });
        file
    }

    fn thread(anchor: ThreadAnchor, end_line: Option<u32>, text: Option<&str>) -> ReviewThread {
        ReviewThread {
            id: ThreadId(1),
            anchor,
            comments: Vec::new(),
            finding: None,
            end_line,
            anchor_text: text.map(String::from),
            status: ThreadStatus::Open,
            addressed: None,
        }
    }

    use LineKind::{Added, Context, Deleted};

    #[test]
    fn a_thread_with_its_code_where_it_was_stays_current() {
        let files = [file(
            "a.rs",
            &[
                (Context, Some(1), Some(1), "fn a() {"),
                (Added, None, Some(2), "  x();"),
            ],
        )];
        let anchor = ThreadAnchor::line("a.rs", Side::New, 2);
        assert_eq!(place(&thread(anchor, None, Some("  x();")), &files), Placement::Current);
    }

    #[test]
    fn a_thread_follows_its_code_after_an_edit_above() {
        let files = [file(
            "a.rs",
            &[
                (Added, None, Some(1), "// new"),
                (Added, None, Some(2), "// new"),
                (Context, Some(1), Some(3), "fn a() {"),
                (Added, None, Some(4), "  x();"),
            ],
        )];
        let anchor = ThreadAnchor::line("a.rs", Side::New, 2);
        assert_eq!(
            place(&thread(anchor, None, Some("  x();")), &files),
            Placement::Moved {
                line: 4,
                end_line: None
            }
        );
    }

    #[test]
    fn a_range_moves_as_one_block() {
        let files = [file(
            "a.rs",
            &[
                (Added, None, Some(10), "one"),
                (Added, None, Some(11), "two"),
                (Added, None, Some(12), "three"),
            ],
        )];
        let anchor = ThreadAnchor::line("a.rs", Side::New, 5);
        assert_eq!(
            place(&thread(anchor, Some(6), Some("two\nthree")), &files),
            Placement::Moved {
                line: 11,
                end_line: Some(12)
            }
        );
    }

    #[test]
    fn the_nearest_copy_wins() {
        let files = [file(
            "a.rs",
            &[
                (Added, None, Some(2), "x"),
                (Context, Some(3), Some(3), "y"),
                (Added, None, Some(40), "x"),
            ],
        )];
        let anchor = ThreadAnchor::line("a.rs", Side::New, 38);
        assert_eq!(
            place(&thread(anchor, None, Some("x")), &files),
            Placement::Moved {
                line: 40,
                end_line: None
            }
        );
    }

    #[test]
    fn a_thread_is_outdated_when_its_line_is_gone() {
        let files = [file("a.rs", &[(Added, None, Some(1), "other")])];
        let anchor = ThreadAnchor::line("a.rs", Side::New, 1);
        assert_eq!(
            place(&thread(anchor.clone(), None, Some("x();")), &files),
            Placement::Outdated
        );
        let far = ThreadAnchor::line("a.rs", Side::New, 99);
        assert_eq!(place(&thread(far, None, None), &files), Placement::Outdated);
        assert_eq!(place(&thread(anchor, None, None), &files), Placement::Current);
    }

    #[test]
    fn the_old_side_reads_deleted_lines() {
        let files = [file(
            "a.rs",
            &[(Deleted, Some(7), None, "gone();"), (Added, None, Some(7), "kept();")],
        )];
        let old = ThreadAnchor::line("a.rs", Side::Old, 7);
        assert_eq!(place(&thread(old, None, Some("gone();")), &files), Placement::Current);
        let new = ThreadAnchor::line("a.rs", Side::New, 7);
        assert_eq!(place(&thread(new, None, Some("gone();")), &files), Placement::Outdated);
    }

    #[test]
    fn a_missing_file_is_gone_and_a_renamed_one_is_found_by_its_old_path() {
        let mut renamed = file("b.rs", &[(Added, None, Some(1), "x")]);
        renamed.old_path = Some("a.rs".into());
        let files = [renamed];
        assert_eq!(
            place(&thread(ThreadAnchor::file("a.rs"), None, None), &files),
            Placement::Current
        );
        assert_eq!(
            place(&thread(ThreadAnchor::file("c.rs"), None, None), &files),
            Placement::FileGone
        );
        assert_eq!(
            place(&thread(ThreadAnchor::line("c.rs", Side::New, 1), None, None), &files),
            Placement::FileGone
        );
        assert_eq!(
            place(&thread(ThreadAnchor::Change, None, None), &files),
            Placement::Current
        );
    }

    #[test]
    fn the_text_of_a_range_needs_every_line_in_the_diff() {
        let files = [file(
            "a.rs",
            &[(Added, None, Some(1), "a"), (Added, None, Some(2), "b")],
        )];
        let range = |end| thread(ThreadAnchor::line("a.rs", Side::New, 1), Some(end), None);
        assert_eq!(text_of(&range(2), &files).as_deref(), Some("a\nb"));
        assert_eq!(text_of(&range(3), &files), None);
    }
}
