use diffpilot_core::changes::FileStatus;
use diffpilot_core::diff::{DiffLine, DiffMode, FileDiff, Hunk, LineKind};
use diffpilot_core::threads::{Side, ThreadAnchor, ThreadId};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub const BIG_LINES: u32 = 1500;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Notice {
    Binary,
    Generated,
    Large { lines: u32 },
    Renamed,
    Empty,
}

impl Notice {
    pub fn can_load(self) -> bool {
        matches!(self, Self::Generated | Self::Large { .. })
    }

    pub fn text(self) -> String {
        match self {
            Self::Binary => "Binary file not shown.".into(),
            Self::Generated => "Generated file. Hidden by default.".into(),
            Self::Large { lines } => format!("Large diff ({} lines). Hidden by default.", group_thousands(lines)),
            Self::Renamed => "Renamed without changes.".into(),
            Self::Empty => "No content changes.".into(),
        }
    }
}

fn group_thousands(value: u32) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowKind {
    FileHeader,
    Notice(Notice),
    Hunk {
        hunk: u32,
    },
    Line {
        hunk: u32,
        line: u32,
    },
    /// A split row. `left` and `right` index the hunk's lines: a deleted or context line on the left,
    /// an added or context line on the right. A context line is on both sides.
    Pair {
        hunk: u32,
        left: Option<u32>,
        right: Option<u32>,
    },
    Thread {
        slot: u32,
    },
    Gap,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    pub file: u32,
    pub kind: RowKind,
}

impl Row {
    pub fn hunk(self) -> Option<u32> {
        match self.kind {
            RowKind::Hunk { hunk } | RowKind::Line { hunk, .. } | RowKind::Pair { hunk, .. } => Some(hunk),
            _ => None,
        }
    }

    pub fn shows_line(self, hunk: u32, line: u32) -> bool {
        match self.kind {
            RowKind::Line { hunk: h, line: l } => (h, l) == (hunk, line),
            RowKind::Pair { hunk: h, left, right } => h == hunk && (left == Some(line) || right == Some(line)),
            _ => false,
        }
    }

    pub fn is_header(self) -> bool {
        self.kind == RowKind::FileHeader
    }
}

/// A review thread the pane places in the row list. The pane never creates these itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThreadSlot {
    pub id: ThreadId,
    pub anchor: ThreadAnchor,
}

/// A row named by what it shows, so it can be found again after the rows are rebuilt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowRef {
    Header,
    Notice,
    Gap,
    Hunk(u32),
    Line(u32, u32),
    Thread(ThreadId),
}

#[derive(Default)]
pub struct Rows {
    pub rows: Vec<Row>,
    pub file_start: Vec<usize>,
    pub thread_rows: Vec<Option<usize>>,
}

impl Rows {
    /// Rows before the first file header: the threads on the whole change.
    pub fn lead(&self) -> usize {
        self.file_start.first().copied().unwrap_or(0)
    }

    pub fn header_index(&self, file: usize) -> Option<usize> {
        (file + 1 < self.file_start.len()).then(|| self.file_start[file])
    }

    pub fn thread_row(&self, slot: u32) -> Option<usize> {
        self.thread_rows.get(slot as usize).copied().flatten()
    }

    pub fn row_ref(&self, index: usize, threads: &[ThreadSlot]) -> RowRef {
        match self.rows[index].kind {
            RowKind::FileHeader => RowRef::Header,
            RowKind::Notice(_) => RowRef::Notice,
            RowKind::Gap => RowRef::Gap,
            RowKind::Hunk { hunk } => RowRef::Hunk(hunk),
            RowKind::Line { hunk, line } => RowRef::Line(hunk, line),
            RowKind::Pair { hunk, left, right } => RowRef::Line(hunk, left.or(right).unwrap_or_default()),
            RowKind::Thread { slot } => RowRef::Thread(threads[slot as usize].id),
        }
    }

    pub fn find(&self, file: usize, target: RowRef, threads: &[ThreadSlot]) -> Option<usize> {
        if let RowRef::Thread(id) = target {
            let slot = threads.iter().position(|thread| thread.id == id)?;
            return self.thread_row(slot as u32);
        }
        self.file_range(file)?.find(|&index| match target {
            RowRef::Line(hunk, line) => self.rows[index].shows_line(hunk, line),
            _ => self.row_ref(index, threads) == target,
        })
    }

    pub fn file_range(&self, file: usize) -> Option<std::ops::Range<usize>> {
        self.header_index(file).map(|start| start..self.file_start[file + 1])
    }
}

#[derive(Clone, Copy)]
pub struct Visibility<'a> {
    pub collapsed: &'a HashSet<String>,
    pub loaded: &'a HashSet<String>,
}

pub fn is_generated(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    matches!(
        name,
        "yarn.lock" | "package-lock.json" | "pnpm-lock.yaml" | "Podfile.lock"
    ) || name.ends_with(".pbxproj")
        || name.ends_with(".snap")
}

pub fn body_notice(file: &FileDiff, loaded: bool) -> Option<Notice> {
    if file.is_binary {
        return Some(Notice::Binary);
    }
    if !loaded {
        let generated = is_generated(&file.path);
        let lines = file.additions + file.deletions;
        if generated {
            return Some(Notice::Generated);
        }
        if lines > BIG_LINES {
            return Some(Notice::Large { lines });
        }
    }
    if file.hunks.is_empty() {
        return Some(if file.status == FileStatus::Renamed {
            Notice::Renamed
        } else {
            Notice::Empty
        });
    }
    None
}

pub fn build(files: &[Arc<FileDiff>], visibility: Visibility, threads: &[ThreadSlot], mode: DiffMode) -> Rows {
    let mut by_path: HashMap<&str, Vec<(u32, &ThreadSlot)>> = HashMap::new();
    for (slot, thread) in threads.iter().enumerate() {
        by_path
            .entry(thread.anchor.path())
            .or_default()
            .push((slot as u32, thread));
    }
    let capacity = files
        .iter()
        .map(|file| 2 + file.hunks.iter().map(|hunk| 1 + hunk.lines.len()).sum::<usize>())
        .sum();
    let mut out = Rows {
        rows: Vec::with_capacity(capacity),
        file_start: Vec::with_capacity(files.len() + 1),
        thread_rows: vec![None; threads.len()],
    };
    if !files.is_empty() {
        for (slot, thread) in threads.iter().enumerate() {
            if thread.anchor == ThreadAnchor::Change {
                out.rows.push(Row {
                    file: 0,
                    kind: RowKind::Thread { slot: slot as u32 },
                });
            }
        }
    }
    let no_threads = Vec::new();
    for (index, file) in files.iter().enumerate() {
        let f = index as u32;
        out.file_start.push(out.rows.len());
        out.rows.push(Row {
            file: f,
            kind: RowKind::FileHeader,
        });
        let file_threads = by_path.get(file.path.as_str()).unwrap_or(&no_threads);
        for (slot, thread) in file_threads {
            if matches!(thread.anchor, ThreadAnchor::File { .. }) {
                out.rows.push(Row {
                    file: f,
                    kind: RowKind::Thread { slot: *slot },
                });
            }
        }
        if !visibility.collapsed.contains(&file.path) {
            push_body(
                &mut out.rows,
                f,
                file,
                visibility.loaded.contains(&file.path),
                file_threads,
                mode,
            );
        }
        out.rows.push(Row {
            file: f,
            kind: RowKind::Gap,
        });
    }
    out.file_start.push(out.rows.len());
    for (index, row) in out.rows.iter().enumerate() {
        if let RowKind::Thread { slot } = row.kind {
            out.thread_rows[slot as usize] = Some(index);
        }
    }
    out
}

fn push_body(
    rows: &mut Vec<Row>,
    f: u32,
    file: &FileDiff,
    loaded: bool,
    threads: &[(u32, &ThreadSlot)],
    mode: DiffMode,
) {
    if let Some(notice) = body_notice(file, loaded) {
        rows.push(Row {
            file: f,
            kind: RowKind::Notice(notice),
        });
        return;
    }
    for (h, hunk) in file.hunks.iter().enumerate() {
        let hunk_index = h as u32;
        rows.push(Row {
            file: f,
            kind: RowKind::Hunk { hunk: hunk_index },
        });
        match mode {
            DiffMode::Unified => push_unified_hunk(rows, f, hunk_index, hunk, threads),
            DiffMode::Split => push_split_hunk(rows, f, hunk_index, hunk, threads),
        }
    }
}

fn push_unified_hunk(rows: &mut Vec<Row>, f: u32, hunk_index: u32, hunk: &Hunk, threads: &[(u32, &ThreadSlot)]) {
    for (l, line) in hunk.lines.iter().enumerate() {
        rows.push(Row {
            file: f,
            kind: RowKind::Line {
                hunk: hunk_index,
                line: l as u32,
            },
        });
        push_line_threads(rows, f, line, threads);
    }
}

/// Each run of changed lines becomes rows that put its deletions on the left and its additions on the right,
/// in order. The shorter side ends early and the other side's cells have no partner.
fn push_split_hunk(rows: &mut Vec<Row>, f: u32, hunk_index: u32, hunk: &Hunk, threads: &[(u32, &ThreadSlot)]) {
    let lines = &hunk.lines;
    let mut push_pair = |left: Option<u32>, right: Option<u32>| {
        rows.push(Row {
            file: f,
            kind: RowKind::Pair {
                hunk: hunk_index,
                left,
                right,
            },
        });
        let own_threads = [
            left.filter(|&index| lines[index as usize].kind == LineKind::Deleted),
            right,
        ];
        for index in own_threads.into_iter().flatten() {
            push_line_threads(rows, f, &lines[index as usize], threads);
        }
    };
    let (mut deleted, mut added) = (Vec::new(), Vec::new());
    let mut at = 0;
    while at < lines.len() {
        if lines[at].kind == LineKind::Context {
            push_pair(Some(at as u32), Some(at as u32));
            at += 1;
            continue;
        }
        deleted.clear();
        added.clear();
        while at < lines.len() && lines[at].kind != LineKind::Context {
            if lines[at].kind == LineKind::Deleted {
                deleted.push(at as u32);
            } else {
                added.push(at as u32);
            }
            at += 1;
        }
        for k in 0..deleted.len().max(added.len()) {
            push_pair(deleted.get(k).copied(), added.get(k).copied());
        }
    }
}

fn push_line_threads(rows: &mut Vec<Row>, f: u32, line: &DiffLine, threads: &[(u32, &ThreadSlot)]) {
    if threads.is_empty() {
        return;
    }
    let (side, number) = match line.kind {
        LineKind::Deleted => (Side::Old, line.old),
        _ => (Side::New, line.new),
    };
    for (slot, thread) in threads {
        if let ThreadAnchor::Line {
            side: anchor_side,
            line: anchor_line,
            ..
        } = &thread.anchor
            && *anchor_side == side
            && Some(*anchor_line) == number
        {
            rows.push(Row {
                file: f,
                kind: RowKind::Thread { slot: *slot },
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{diff_hunks, line, lines_of};

    fn file(path: &str, hunks: Vec<Vec<DiffLine>>) -> Arc<FileDiff> {
        Arc::new(diff_hunks(path, hunks))
    }

    fn sample() -> Vec<Arc<FileDiff>> {
        vec![
            file(
                "a.ts",
                vec![
                    vec![
                        line(LineKind::Context, Some(1), Some(1), "keep"),
                        line(LineKind::Deleted, Some(2), None, "gone"),
                        line(LineKind::Added, None, Some(2), "new"),
                    ],
                    vec![line(LineKind::Added, None, Some(9), "tail")],
                ],
            ),
            file("b.ts", vec![vec![line(LineKind::Added, None, Some(1), "x")]]),
        ]
    }

    fn rows_of(files: &[Arc<FileDiff>], collapsed: &[&str]) -> Rows {
        let collapsed: HashSet<String> = collapsed.iter().map(|path| path.to_string()).collect();
        let loaded = HashSet::new();
        build(
            files,
            Visibility {
                collapsed: &collapsed,
                loaded: &loaded,
            },
            &[],
            DiffMode::Unified,
        )
    }

    fn kinds(rows: &Rows) -> Vec<(u32, RowKind)> {
        rows.rows.iter().map(|row| (row.file, row.kind)).collect()
    }

    #[test]
    fn builds_header_hunks_lines_and_gap_per_file() {
        let rows = rows_of(&sample(), &[]);
        use RowKind::*;
        assert_eq!(
            kinds(&rows),
            [
                (0, FileHeader),
                (0, Hunk { hunk: 0 }),
                (0, Line { hunk: 0, line: 0 }),
                (0, Line { hunk: 0, line: 1 }),
                (0, Line { hunk: 0, line: 2 }),
                (0, Hunk { hunk: 1 }),
                (0, Line { hunk: 1, line: 0 }),
                (0, Gap),
                (1, FileHeader),
                (1, Hunk { hunk: 0 }),
                (1, Line { hunk: 0, line: 0 }),
                (1, Gap),
            ]
        );
        assert_eq!(rows.file_start, [0, 8, 12]);
        assert_eq!(rows.file_range(1), Some(8..12));
        assert_eq!(rows.header_index(2), None);
    }

    #[test]
    fn collapsed_file_keeps_only_header_and_gap() {
        let rows = rows_of(&sample(), &["a.ts"]);
        use RowKind::*;
        assert_eq!(kinds(&rows)[..3], [(0, FileHeader), (0, Gap), (1, FileHeader)]);
        assert_eq!(rows.file_start, [0, 2, 6]);
    }

    #[test]
    fn hunk_of_a_row_is_known_for_hunks_and_lines_only() {
        let rows = rows_of(&sample(), &[]);
        let hunks: Vec<_> = rows.rows.iter().map(|row| row.hunk()).collect();
        assert_eq!(
            hunks[..8],
            [None, Some(0), Some(0), Some(0), Some(0), Some(1), Some(1), None]
        );
    }

    #[test]
    fn binary_file_shows_a_notice_instead_of_hunks() {
        let mut diff = FileDiff::new("logo.png", FileStatus::Added);
        diff.is_binary = true;
        let rows = rows_of(&[Arc::new(diff)], &[]);
        assert_eq!(rows.rows[1].kind, RowKind::Notice(Notice::Binary));
        assert_eq!(rows.rows.len(), 3);
    }

    #[test]
    fn generated_and_large_files_wait_for_a_load_request() {
        let mut lock = (*file("pnpm-lock.yaml", vec![vec![line(LineKind::Added, None, Some(1), "x")]])).clone();
        lock.additions = 1;
        let mut big = (*file("big.ts", vec![vec![line(LineKind::Added, None, Some(1), "x")]])).clone();
        big.additions = 1600;
        let files = vec![Arc::new(lock), Arc::new(big)];
        let none = HashSet::new();
        let rows = build(
            &files,
            Visibility {
                collapsed: &none,
                loaded: &none,
            },
            &[],
            DiffMode::Unified,
        );
        assert_eq!(rows.rows[1].kind, RowKind::Notice(Notice::Generated));
        assert_eq!(rows.rows[4].kind, RowKind::Notice(Notice::Large { lines: 1600 }));
        let loaded: HashSet<String> = ["pnpm-lock.yaml".into(), "big.ts".into()].into();
        let rows = build(
            &files,
            Visibility {
                collapsed: &none,
                loaded: &loaded,
            },
            &[],
            DiffMode::Unified,
        );
        assert_eq!(rows.rows[1].kind, RowKind::Hunk { hunk: 0 });
    }

    #[test]
    fn notice_text_matches_the_web_page() {
        assert_eq!(
            Notice::Large { lines: 12_345 }.text(),
            "Large diff (12,345 lines). Hidden by default."
        );
        assert_eq!(
            Notice::Large { lines: 1_600 }.text(),
            "Large diff (1,600 lines). Hidden by default."
        );
        assert!(Notice::Generated.can_load());
        assert!(!Notice::Binary.can_load());
    }

    #[test]
    fn file_without_hunks_says_why() {
        let mut renamed = FileDiff::new("b.txt", FileStatus::Renamed);
        renamed.old_path = Some("a.txt".into());
        let empty = FileDiff::new("c.txt", FileStatus::Modified);
        let rows = rows_of(&[Arc::new(renamed), Arc::new(empty)], &[]);
        assert_eq!(rows.rows[1].kind, RowKind::Notice(Notice::Renamed));
        assert_eq!(rows.rows[4].kind, RowKind::Notice(Notice::Empty));
    }

    #[test]
    fn detects_generated_files_by_name() {
        assert!(is_generated("pnpm-lock.yaml"));
        assert!(is_generated("app/Podfile.lock"));
        assert!(is_generated("x.xcodeproj/project.pbxproj"));
        assert!(is_generated("tests/__snapshots__/a.snap"));
        assert!(!is_generated("src/lock.ts"));
        assert!(!is_generated("my-package-lock.json.md"));
    }

    #[test]
    fn thread_rows_follow_their_anchor() {
        let files = sample();
        let threads = vec![
            ThreadSlot {
                id: ThreadId(1),
                anchor: ThreadAnchor::file("a.ts"),
            },
            ThreadSlot {
                id: ThreadId(2),
                anchor: ThreadAnchor::line("a.ts", Side::Old, 2),
            },
            ThreadSlot {
                id: ThreadId(3),
                anchor: ThreadAnchor::line("a.ts", Side::New, 2),
            },
            ThreadSlot {
                id: ThreadId(4),
                anchor: ThreadAnchor::line("a.ts", Side::Old, 1),
            },
            ThreadSlot {
                id: ThreadId(5),
                anchor: ThreadAnchor::file("elsewhere.ts"),
            },
        ];
        let none = HashSet::new();
        let rows = build(
            &files,
            Visibility {
                collapsed: &none,
                loaded: &none,
            },
            &threads,
            DiffMode::Unified,
        );
        use RowKind::*;
        assert_eq!(
            kinds(&rows)[..8],
            [
                (0, FileHeader),
                (0, Thread { slot: 0 }),
                (0, Hunk { hunk: 0 }),
                (0, Line { hunk: 0, line: 0 }),
                (0, Line { hunk: 0, line: 1 }),
                (0, Thread { slot: 1 }),
                (0, Line { hunk: 0, line: 2 }),
                (0, Thread { slot: 2 }),
            ]
        );
        assert!(!kinds(&rows).contains(&(0, Thread { slot: 3 })));
    }

    #[test]
    fn change_threads_lead_the_rows_and_every_thread_row_is_indexed() {
        let files = sample();
        let threads = vec![
            ThreadSlot {
                id: ThreadId(7),
                anchor: ThreadAnchor::line("a.ts", Side::New, 2),
            },
            ThreadSlot {
                id: ThreadId(8),
                anchor: ThreadAnchor::Change,
            },
        ];
        let none = HashSet::new();
        let rows = build(
            &files,
            Visibility {
                collapsed: &none,
                loaded: &none,
            },
            &threads,
            DiffMode::Unified,
        );
        assert_eq!(rows.rows[0].kind, RowKind::Thread { slot: 1 });
        assert_eq!(rows.lead(), 1);
        assert_eq!(rows.header_index(0), Some(1));
        assert_eq!(rows.thread_row(1), Some(0));
        let line = rows.thread_row(0).expect("the line thread is placed");
        assert_eq!(rows.rows[line - 1].kind, RowKind::Line { hunk: 0, line: 2 });
        assert_eq!(rows.find(0, RowRef::Thread(ThreadId(7)), &threads), Some(line));
        assert_eq!(rows.find(1, RowRef::Line(0, 0), &threads), Some(rows.file_start[1] + 2));
        assert_eq!(rows.find(1, RowRef::Line(5, 5), &threads), None);
    }

    #[test]
    fn a_pane_without_files_has_no_change_rows() {
        let threads = vec![ThreadSlot {
            id: ThreadId(1),
            anchor: ThreadAnchor::Change,
        }];
        let none = HashSet::new();
        let rows = build(
            &[],
            Visibility {
                collapsed: &none,
                loaded: &none,
            },
            &threads,
            DiffMode::Unified,
        );
        assert!(rows.rows.is_empty());
        assert_eq!(rows.thread_row(0), None);
    }

    fn split_of(files: &[Arc<FileDiff>], threads: &[ThreadSlot]) -> Rows {
        let none = HashSet::new();
        build(
            files,
            Visibility {
                collapsed: &none,
                loaded: &none,
            },
            threads,
            DiffMode::Split,
        )
    }

    type Cells = (Option<u32>, Option<u32>);

    fn pairs_of(spec: &str) -> Vec<Cells> {
        let rows = split_of(&[file("a.ts", vec![lines_of(spec)])], &[]);
        rows.rows
            .iter()
            .filter_map(|row| match row.kind {
                RowKind::Pair { left, right, .. } => Some((left, right)),
                _ => None,
            })
            .collect()
    }

    const NONE: Option<u32> = None;

    fn at(line: u32) -> Option<u32> {
        Some(line)
    }

    #[test]
    fn a_balanced_change_puts_each_deletion_beside_its_addition() {
        assert_eq!(pairs_of("ddaa"), [(at(0), at(2)), (at(1), at(3))]);
    }

    #[test]
    fn extra_deletions_have_nothing_on_the_right() {
        assert_eq!(pairs_of("ddda"), [(at(0), at(3)), (at(1), NONE), (at(2), NONE)]);
    }

    #[test]
    fn extra_additions_have_nothing_on_the_left() {
        assert_eq!(pairs_of("daaa"), [(at(0), at(1)), (NONE, at(2)), (NONE, at(3))]);
    }

    #[test]
    fn context_lines_are_on_both_sides_and_split_the_changes_around_them() {
        assert_eq!(
            pairs_of("cdacaac"),
            [
                (at(0), at(0)),
                (at(1), at(2)),
                (at(3), at(3)),
                (NONE, at(4)),
                (NONE, at(5)),
                (at(6), at(6)),
            ]
        );
    }

    #[test]
    fn interleaved_changes_in_one_run_pair_in_order() {
        assert_eq!(pairs_of("dada"), [(at(0), at(1)), (at(2), at(3))]);
        assert_eq!(pairs_of("ad"), [(at(1), at(0))]);
    }

    #[test]
    fn every_line_is_shown_once_on_the_side_it_belongs_to() {
        for spec in ["", "c", "d", "a", "cc", "dddaa", "adadad", "cdcacdda", "aaaccddd"] {
            let lines = lines_of(spec);
            let pairs = pairs_of(spec);
            for (index, line) in lines.iter().enumerate() {
                let index = Some(index as u32);
                let left = pairs.iter().filter(|(left, _)| *left == index).count();
                let right = pairs.iter().filter(|(_, right)| *right == index).count();
                let want = match line.kind {
                    LineKind::Context => (1, 1),
                    LineKind::Deleted => (1, 0),
                    LineKind::Added => (0, 1),
                };
                assert_eq!((left, right), want, "{spec} line {index:?}");
            }
            assert!(pairs.iter().all(|pair| *pair != (NONE, NONE)), "{spec}");
        }
    }

    #[test]
    fn an_added_file_has_an_empty_left_side_and_a_deleted_file_an_empty_right_side() {
        assert_eq!(pairs_of("aaa"), [(NONE, at(0)), (NONE, at(1)), (NONE, at(2))]);
        assert_eq!(pairs_of("ddd"), [(at(0), NONE), (at(1), NONE), (at(2), NONE)]);
    }

    #[test]
    fn hunk_headers_stay_whole_rows_and_the_other_row_kinds_do_not_change() {
        let rows = split_of(&sample(), &[]);
        use RowKind::*;
        assert_eq!(
            kinds(&rows),
            [
                (0, FileHeader),
                (0, Hunk { hunk: 0 }),
                (
                    0,
                    Pair {
                        hunk: 0,
                        left: at(0),
                        right: at(0)
                    }
                ),
                (
                    0,
                    Pair {
                        hunk: 0,
                        left: at(1),
                        right: at(2)
                    }
                ),
                (0, Hunk { hunk: 1 }),
                (
                    0,
                    Pair {
                        hunk: 1,
                        left: NONE,
                        right: at(0)
                    }
                ),
                (0, Gap),
                (1, FileHeader),
                (1, Hunk { hunk: 0 }),
                (
                    1,
                    Pair {
                        hunk: 0,
                        left: NONE,
                        right: at(0)
                    }
                ),
                (1, Gap),
            ]
        );
        assert_eq!(rows.file_start, [0, 7, 11]);
        assert_eq!(rows.rows[3].hunk(), Some(0));
    }

    #[test]
    fn files_without_a_body_show_the_same_notice_in_both_modes() {
        let mut binary = FileDiff::new("logo.png", FileStatus::Added);
        binary.is_binary = true;
        let mut renamed = FileDiff::new("b.txt", FileStatus::Renamed);
        renamed.old_path = Some("a.txt".into());
        let files = [Arc::new(binary), Arc::new(renamed)];
        let split = split_of(&files, &[]);
        assert_eq!(kinds(&split), kinds(&rows_of(&files, &[])));
        assert_eq!(split.rows[1].kind, RowKind::Notice(Notice::Binary));
        assert_eq!(split.rows[4].kind, RowKind::Notice(Notice::Renamed));
    }

    #[test]
    fn a_rename_with_edits_pairs_its_lines() {
        let mut renamed = diff_hunks("b.ts", vec![lines_of("cda")]);
        renamed.status = FileStatus::Renamed;
        renamed.old_path = Some("a.ts".into());
        let rows = split_of(&[Arc::new(renamed)], &[]);
        assert_eq!(rows.rows.len(), 1 + 1 + 2 + 1);
        assert!(matches!(
            rows.rows[3].kind,
            RowKind::Pair {
                left: Some(1),
                right: Some(2),
                ..
            }
        ));
    }

    #[test]
    fn a_no_newline_marker_adds_no_row_and_does_not_split_a_change() {
        let text = "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1,2 +1,2 @@\n same\n-old\n\\ No newline at end of file\n+new\n\\ No newline at end of file\n";
        let files: Vec<_> = diffpilot_core::diff::parse(text).into_iter().map(Arc::new).collect();
        let rows = split_of(&files, &[]);
        let pairs: Vec<_> = rows
            .rows
            .iter()
            .filter_map(|row| match row.kind {
                RowKind::Pair { left, right, .. } => Some((left, right)),
                _ => None,
            })
            .collect();
        assert_eq!(pairs, [(at(0), at(0)), (at(1), at(2))]);
        assert_eq!(rows.rows.len(), 1 + 1 + 2 + 1);
    }

    #[test]
    fn a_thread_sits_under_the_row_that_shows_its_line_left_side_first() {
        let threads = vec![
            ThreadSlot {
                id: ThreadId(1),
                anchor: ThreadAnchor::line("a.ts", Side::New, 2),
            },
            ThreadSlot {
                id: ThreadId(2),
                anchor: ThreadAnchor::line("a.ts", Side::Old, 2),
            },
            ThreadSlot {
                id: ThreadId(3),
                anchor: ThreadAnchor::line("a.ts", Side::New, 1),
            },
            ThreadSlot {
                id: ThreadId(4),
                anchor: ThreadAnchor::line("a.ts", Side::Old, 1),
            },
        ];
        let rows = split_of(&sample(), &threads);
        let shown: Vec<_> = rows.rows[..7]
            .iter()
            .map(|row| match row.kind {
                RowKind::Thread { slot } => format!("thread {}", slot + 1),
                RowKind::Pair { left, right, .. } => format!("{left:?}|{right:?}"),
                other => format!("{other:?}"),
            })
            .collect();
        assert_eq!(
            shown,
            [
                "FileHeader",
                "Hunk { hunk: 0 }",
                "Some(0)|Some(0)",
                "thread 3",
                "Some(1)|Some(2)",
                "thread 2",
                "thread 1",
            ],
            "an old-side thread on a context line has no row, as in the unified rows"
        );
    }

    #[test]
    fn a_line_is_found_in_either_mode_by_the_row_that_shows_it() {
        let files = sample();
        let threads: [ThreadSlot; 0] = [];
        let unified = rows_of(&files, &[]);
        let split = split_of(&files, &threads);
        assert_eq!(unified.find(0, RowRef::Line(0, 1), &threads), Some(3));
        assert_eq!(split.find(0, RowRef::Line(0, 1), &threads), Some(3));
        assert_eq!(split.find(0, RowRef::Line(0, 2), &threads), Some(3));
        assert_eq!(unified.find(0, RowRef::Line(0, 2), &threads), Some(4));
        assert_eq!(split.find(0, RowRef::Line(0, 3), &threads), None);
        assert_eq!(split.row_ref(3, &threads), RowRef::Line(0, 1));
        assert_eq!(split.row_ref(2, &threads), RowRef::Line(0, 0));
        assert_eq!(split.row_ref(5, &threads), RowRef::Line(1, 0));
        assert!(split.rows[3].shows_line(0, 2));
        assert!(!split.rows[3].shows_line(1, 2));
        assert!(!split.rows[1].shows_line(0, 0), "a hunk header shows no line");
    }
}
