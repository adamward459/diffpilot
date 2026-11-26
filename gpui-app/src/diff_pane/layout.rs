use crate::diff_pane::rows::{Row, RowKind, Rows};
use diffpilot_core::diff::{DiffMode, FileDiff};
use diffpilot_core::threads::{Side, ThreadAnchor};
use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;
use unicode_width::UnicodeWidthChar;

pub const FILE_HEADER: f32 = 38.0;
pub const HUNK: f32 = 28.0;
pub const HUD: f32 = 34.0;
pub const LINE: f32 = 20.0;
pub const GAP: f32 = 12.0;
pub const NOTICE: f32 = 96.0;
pub const PREVIEW: f32 = 220.0;
pub const THREAD: f32 = 150.0;
pub const LINE_THREAD_LEFT: f32 = 86.0;
pub const THREAD_RIGHT: f32 = 16.0;
pub const LINE_THREAD_TOP: f32 = 8.0;
pub const LINE_THREAD_BOTTOM: f32 = 10.0;
pub const FILE_THREAD_LEFT: f32 = 16.0;
pub const FILE_THREAD_VERTICAL: f32 = 10.0;
pub const FILE_THREAD_BORDER: f32 = 1.0;
pub const NUMBER_COLUMN: f32 = 40.0;
pub const RIBBON: f32 = 3.0;
pub const CODE_PADDING: f32 = 12.0;
pub const GUTTER: f32 = NUMBER_COLUMN * 2.0 + RIBBON + CODE_PADDING;
pub const SPLIT_MIN_WIDTH: f32 = 900.0;
pub const SPLIT_DIVIDER: f32 = 1.0;
pub const SPLIT_GUTTER: f32 = NUMBER_COLUMN + RIBBON + CODE_PADDING;
pub const SPLIT_THREAD_LEFT: f32 = NUMBER_COLUMN + RIBBON + 3.0;
const TAB_CELLS: usize = 4;

pub fn cell_width(ch: char) -> usize {
    if ch == '\t' { TAB_CELLS } else { ch.width().unwrap_or(1) }
}

pub fn cells(text: &str) -> usize {
    if text.is_ascii() {
        text.len() + text.bytes().filter(|&byte| byte == b'\t').count() * (TAB_CELLS - 1)
    } else {
        text.chars().map(cell_width).sum()
    }
}

/// Splits `text` into pieces of at most `cols` cells, breaking anywhere like CSS `line-break: anywhere`.
pub fn wrap_ranges(text: &str, cols: usize) -> Vec<Range<usize>> {
    let cols = cols.max(1);
    let mut ranges = Vec::new();
    let mut start = 0;
    let mut used = 0;
    for (index, ch) in text.char_indices() {
        let width = cell_width(ch);
        if used + width > cols && used > 0 {
            ranges.push(start..index);
            start = index;
            used = 0;
        }
        used += width;
    }
    if start < text.len() || ranges.is_empty() {
        ranges.push(start..text.len());
    }
    ranges
}

pub fn wrapped_lines(text: &str, cols: usize) -> usize {
    let cols = cols.max(1);
    let total = cells(text);
    if total <= cols {
        return 1;
    }
    if text.is_ascii() && !text.contains('\t') {
        return total.div_ceil(cols);
    }
    wrap_ranges(text, cols).len()
}

pub fn columns_for(width: f32, cell: f32) -> usize {
    if cell <= 0.0 {
        return 1;
    }
    (((width - GUTTER) / cell - 0.01).floor() as isize).max(1) as usize
}

/// Split rows need room for two columns. A narrower pane shows the unified rows instead.
pub fn shown_mode(mode: DiffMode, width: f32) -> DiffMode {
    if width >= SPLIT_MIN_WIDTH {
        mode
    } else {
        DiffMode::Unified
    }
}

pub fn column_width(width: f32) -> f32 {
    ((width - SPLIT_DIVIDER) / 2.0).max(0.0)
}

pub fn split_columns_for(width: f32, cell: f32) -> usize {
    if cell <= 0.0 {
        return 1;
    }
    (((column_width(width) - SPLIT_GUTTER) / cell - 0.01).floor() as isize).max(1) as usize
}

/// Space around a thread card inside its row: left, right, top, bottom. A line thread sits under the code
/// gutter. In split rows it fills the half its side is on. A file or change thread spans the pane and has a
/// rule below it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThreadFrame {
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
    pub rule: bool,
}

impl ThreadFrame {
    /// `split_width` is the pane width when the pane shows split rows.
    pub fn of(anchor: &ThreadAnchor, split_width: Option<f32>) -> Self {
        match anchor {
            ThreadAnchor::Line { side, .. } => {
                let (left, right) = match (split_width, side) {
                    (None, _) => (LINE_THREAD_LEFT, THREAD_RIGHT),
                    (Some(width), Side::Old) => (SPLIT_THREAD_LEFT, width - column_width(width) + THREAD_RIGHT),
                    (Some(width), Side::New) => (column_width(width) + SPLIT_DIVIDER + SPLIT_THREAD_LEFT, THREAD_RIGHT),
                };
                Self {
                    left,
                    right,
                    top: LINE_THREAD_TOP,
                    bottom: LINE_THREAD_BOTTOM,
                    rule: false,
                }
            }
            _ => Self {
                left: FILE_THREAD_LEFT,
                right: THREAD_RIGHT,
                top: FILE_THREAD_VERTICAL,
                bottom: FILE_THREAD_VERTICAL,
                rule: true,
            },
        }
    }

    pub fn card_width(self, pane_width: f32) -> f32 {
        (pane_width - self.left - self.right).max(1.0)
    }

    pub fn row_height(self, card: f32) -> f32 {
        self.top + card + self.bottom + if self.rule { FILE_THREAD_BORDER } else { 0.0 }
    }
}

pub fn row_height(
    row: Row,
    files: &[Arc<FileDiff>],
    wrap: Option<usize>,
    threads: &[f32],
    previews: &HashMap<String, f32>,
) -> f32 {
    match row.kind {
        RowKind::FileHeader => FILE_HEADER,
        RowKind::Hunk { .. } => HUNK,
        RowKind::Gap => GAP,
        RowKind::Notice(_) => NOTICE,
        RowKind::Preview => previews.get(&files[row.file as usize].path).copied().unwrap_or(PREVIEW),
        RowKind::Block { slot } => threads.get(slot as usize).copied().unwrap_or(THREAD),
        RowKind::Line { hunk, line } => {
            let Some(cols) = wrap else { return LINE };
            let text = &files[row.file as usize].hunks[hunk as usize].lines[line as usize].text;
            LINE * wrapped_lines(text, cols) as f32
        }
        RowKind::Pair { hunk, left, right } => {
            let Some(cols) = wrap else { return LINE };
            let lines = &files[row.file as usize].hunks[hunk as usize].lines;
            let wrapped = |cell: Option<u32>| cell.map_or(1, |line| wrapped_lines(&lines[line as usize].text, cols));
            let tall = if left == right {
                wrapped(left)
            } else {
                wrapped(left).max(wrapped(right))
            };
            LINE * tall as f32
        }
    }
}

#[derive(Default)]
pub struct Tops(Vec<f32>);

impl Tops {
    pub fn new(rows: &[Row], files: &[Arc<FileDiff>], wrap: Option<usize>) -> Self {
        Self::with_threads(rows, files, wrap, &[])
    }

    /// `threads` holds the row height of each thread slot. A slot without one takes `THREAD`.
    pub fn with_threads(rows: &[Row], files: &[Arc<FileDiff>], wrap: Option<usize>, threads: &[f32]) -> Self {
        Self::with_heights(rows, files, wrap, threads, &HashMap::new())
    }

    /// `previews` holds the row height of each previewed file by path. A file without one takes `PREVIEW`.
    pub fn with_heights(
        rows: &[Row],
        files: &[Arc<FileDiff>],
        wrap: Option<usize>,
        threads: &[f32],
        previews: &HashMap<String, f32>,
    ) -> Self {
        let mut tops = Vec::with_capacity(rows.len() + 1);
        let mut y = 0.0;
        tops.push(y);
        for &row in rows {
            y += row_height(row, files, wrap, threads, previews);
            tops.push(y);
        }
        Self(tops)
    }

    /// Moves every row below `index` by the change in its height.
    pub fn set_height(&mut self, index: usize, height: f32) {
        let delta = height - self.height(index);
        if delta != 0.0 {
            self.0[index + 1..].iter_mut().for_each(|top| *top += delta);
        }
    }

    pub fn len(&self) -> usize {
        self.0.len().saturating_sub(1)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn total(&self) -> f32 {
        self.0.last().copied().unwrap_or(0.0)
    }

    pub fn top(&self, index: usize) -> f32 {
        self.0[index]
    }

    pub fn height(&self, index: usize) -> f32 {
        self.0[index + 1] - self.0[index]
    }

    pub fn row_at(&self, y: f32) -> Option<usize> {
        if self.is_empty() {
            return None;
        }
        Some(
            self.0
                .partition_point(|&top| top <= y)
                .saturating_sub(1)
                .min(self.len() - 1),
        )
    }

    /// Rows that touch `top..top + height`.
    pub fn window(&self, top: f32, height: f32) -> Range<usize> {
        let Some(first) = self.row_at(top) else { return 0..0 };
        let end = self.0.partition_point(|&row_top| row_top < top + height);
        first..end.clamp(first + 1, self.len())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sticky {
    pub file: u32,
    pub hunk: Option<u32>,
    pub push: f32,
}

impl Sticky {
    pub fn height(self) -> f32 {
        FILE_HEADER + if self.hunk.is_some() { HUNK } else { 0.0 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Position {
    pub file: u32,
    pub sticky: Option<Sticky>,
}

/// Where the viewport top sits: the file it is in, and the pinned header to draw over it.
pub fn position(rows: &Rows, tops: &Tops, scroll: f32) -> Option<Position> {
    let current = tops.row_at(scroll)?;
    if current < rows.lead() {
        return Some(Position { file: 0, sticky: None });
    }
    let row = rows.rows[current];
    let file = row.file;
    if row.is_header() && tops.top(current) >= scroll - 1.0 {
        return Some(Position { file, sticky: None });
    }
    let under = tops
        .row_at(scroll + FILE_HEADER)
        .map_or(current, |index| index.max(current));
    let under_row = rows.rows[under];
    let hunk = if under_row.file == file { under_row.hunk() } else { None };
    let mut sticky = Sticky { file, hunk, push: 0.0 };
    if let Some(next) = rows.header_index(file as usize + 1) {
        sticky.push = (tops.top(next) - scroll - sticky.height()).min(0.0);
    }
    Some(Position {
        file,
        sticky: Some(sticky),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff_pane::rows::{BlockId, BlockSlot, Visibility, build};
    use crate::diff_pane::testing::{added, diff, line, lines_with};
    use diffpilot_core::diff::{DiffLine, DiffMode, LineKind};
    use diffpilot_core::threads::{Side, ThreadId};
    use std::collections::HashSet;

    fn file(path: &str, lines: Vec<DiffLine>) -> Arc<FileDiff> {
        Arc::new(diff(path, lines))
    }

    fn files() -> Vec<Arc<FileDiff>> {
        vec![
            file("a.ts", vec![added(1, "one"), added(2, "two"), added(3, "three")]),
            file("b.ts", vec![added(1, "x")]),
        ]
    }

    fn model(wrap: Option<usize>) -> (Vec<Arc<FileDiff>>, Rows, Tops) {
        let files = files();
        let none = HashSet::new();
        let rows = build(
            &files,
            Visibility {
                collapsed: &none,
                loaded: &none,
                previews: &none,
            },
            &[],
            DiffMode::Unified,
        );
        let tops = Tops::new(&rows.rows, &files, wrap);
        (files, rows, tops)
    }

    #[test]
    fn cells_count_tabs_as_four_and_wide_characters_as_two() {
        assert_eq!(cells("abc"), 3);
        assert_eq!(cells("\tabc"), 7);
        assert_eq!(cells("a\u{4e2d}"), 3);
    }

    #[test]
    fn wraps_ascii_like_the_web_page() {
        assert_eq!(wrapped_lines("", 10), 1);
        assert_eq!(wrapped_lines("0123456789", 10), 1);
        assert_eq!(wrapped_lines("01234567890", 10), 2);
        assert_eq!(wrapped_lines(&"x".repeat(35), 10), 4);
        assert_eq!(wrapped_lines("\t\t\t", 10), 2);
    }

    #[test]
    fn wrap_ranges_agree_with_the_line_count() {
        for text in [
            "",
            "abc",
            "0123456789",
            "01234567890",
            "\t\tab",
            "\u{4e2d}\u{4e2d}\u{4e2d}a",
            "e\u{301}e\u{301}e\u{301}",
        ] {
            for cols in 1..=7 {
                assert_eq!(
                    wrap_ranges(text, cols).len(),
                    wrapped_lines(text, cols),
                    "{text:?} at {cols}"
                );
            }
        }
    }

    #[test]
    fn wrap_ranges_cover_the_text_without_gaps() {
        let text = "ab\u{4e2d}cd\tef";
        let ranges = wrap_ranges(text, 4);
        assert_eq!(ranges.first().map(|r| r.start), Some(0));
        assert_eq!(ranges.last().map(|r| r.end), Some(text.len()));
        for pair in ranges.windows(2) {
            assert_eq!(pair[0].end, pair[1].start);
        }
        let joined: String = ranges.iter().map(|r| &text[r.clone()]).collect();
        assert_eq!(joined, text);
    }

    #[test]
    fn columns_leave_room_for_the_gutter() {
        assert_eq!(columns_for(GUTTER + 10.0 * 7.0, 7.0), 9);
        assert_eq!(columns_for(10.0, 7.0), 1);
    }

    #[test]
    fn heights_follow_row_kinds() {
        let (_, _, tops) = model(None);
        let heights: Vec<_> = (0..tops.len()).map(|i| tops.height(i)).collect();
        assert_eq!(
            heights,
            [FILE_HEADER, HUNK, LINE, LINE, LINE, GAP, FILE_HEADER, HUNK, LINE, GAP]
        );
        assert_eq!(tops.total(), 38.0 + 28.0 + 60.0 + 12.0 + 38.0 + 28.0 + 20.0 + 12.0);
    }

    #[test]
    fn wrapped_lines_grow_their_rows() {
        let files = vec![file("a.ts", vec![added(1, &"x".repeat(25)), added(2, "y")])];
        let none = HashSet::new();
        let rows = build(
            &files,
            Visibility {
                collapsed: &none,
                loaded: &none,
                previews: &none,
            },
            &[],
            DiffMode::Unified,
        );
        let tops = Tops::new(&rows.rows, &files, Some(10));
        assert_eq!(tops.height(2), 3.0 * LINE);
        assert_eq!(tops.height(3), LINE);
        let flat = Tops::new(&rows.rows, &files, None);
        assert_eq!(flat.height(2), LINE);
    }

    #[test]
    fn finds_the_row_at_an_offset() {
        let (_, _, tops) = model(None);
        assert_eq!(tops.row_at(0.0), Some(0));
        assert_eq!(tops.row_at(37.9), Some(0));
        assert_eq!(tops.row_at(38.0), Some(1));
        assert_eq!(tops.row_at(tops.total() + 100.0), Some(tops.len() - 1));
        assert_eq!(Tops::default().row_at(5.0), None);
    }

    #[test]
    fn window_covers_every_touched_row() {
        let (_, _, tops) = model(None);
        assert_eq!(tops.window(0.0, 40.0), 0..2);
        assert_eq!(tops.window(50.0, 10.0), 1..2);
        assert_eq!(tops.window(0.0, 10_000.0), 0..tops.len());
    }

    #[test]
    fn header_at_the_top_needs_no_pinned_copy() {
        let (_, rows, tops) = model(None);
        assert_eq!(position(&rows, &tops, 0.0), Some(Position { file: 0, sticky: None }));
    }

    #[test]
    fn pins_the_header_and_the_hunk_under_it() {
        let (_, rows, tops) = model(None);
        let at = tops.top(2) + 5.0;
        let pos = position(&rows, &tops, at).unwrap();
        assert_eq!(pos.file, 0);
        let sticky = pos.sticky.unwrap();
        assert_eq!(sticky.file, 0);
        assert_eq!(sticky.hunk, Some(0));
        assert_eq!(sticky.push, 0.0);
    }

    #[test]
    fn next_header_pushes_the_pinned_one_up() {
        let (_, rows, tops) = model(None);
        let next = tops.top(rows.header_index(1).unwrap());
        assert_eq!(next, 138.0);
        let with_hunk = position(&rows, &tops, 78.0).unwrap().sticky.unwrap();
        assert_eq!(with_hunk.hunk, Some(0));
        assert_eq!(with_hunk.push, -6.0);
        let header_only = position(&rows, &tops, 133.0).unwrap().sticky.unwrap();
        assert_eq!(header_only.hunk, None);
        assert_eq!(header_only.push, -33.0);
        let clear = position(&rows, &tops, 66.0).unwrap().sticky.unwrap();
        assert_eq!(clear.push, 0.0);
    }

    #[test]
    fn scrolling_into_the_gap_reports_the_same_file() {
        let (_, rows, tops) = model(None);
        let gap = tops.top(rows.file_start[1] - 1) + 1.0;
        let pos = position(&rows, &tops, gap).unwrap();
        assert_eq!(pos.file, 0);
    }

    #[test]
    fn the_second_file_is_current_once_its_header_reaches_the_top() {
        let (_, rows, tops) = model(None);
        let header = tops.top(rows.header_index(1).unwrap());
        assert_eq!(position(&rows, &tops, header).unwrap().file, 1);
        assert_eq!(position(&rows, &tops, header - 0.5).unwrap().file, 0);
    }

    #[test]
    fn empty_pane_has_no_position() {
        assert_eq!(position(&Rows::default(), &Tops::default(), 0.0), None);
    }

    #[test]
    fn a_thread_row_takes_its_slot_height_or_the_placeholder() {
        let files = files();
        let threads = [BlockSlot {
            id: BlockId::Thread(ThreadId(1)),
            anchor: ThreadAnchor::file("a.ts"),
        }];
        let none = HashSet::new();
        let rows = build(
            &files,
            Visibility {
                collapsed: &none,
                loaded: &none,
                previews: &none,
            },
            &threads,
            DiffMode::Unified,
        );
        let row = rows.block_row(0).unwrap();
        assert_eq!(Tops::new(&rows.rows, &files, None).height(row), THREAD);
        let tops = Tops::with_threads(&rows.rows, &files, None, &[200.0]);
        assert_eq!(tops.height(row), 200.0);
    }

    #[test]
    fn resizing_a_row_in_place_matches_a_rebuild() {
        let files = files();
        let threads = [BlockSlot {
            id: BlockId::Thread(ThreadId(1)),
            anchor: ThreadAnchor::file("a.ts"),
        }];
        let none = HashSet::new();
        let rows = build(
            &files,
            Visibility {
                collapsed: &none,
                loaded: &none,
                previews: &none,
            },
            &threads,
            DiffMode::Unified,
        );
        let row = rows.block_row(0).unwrap();
        let mut tops = Tops::with_threads(&rows.rows, &files, None, &[150.0]);
        tops.set_height(row, 260.0);
        let rebuilt = Tops::with_threads(&rows.rows, &files, None, &[260.0]);
        assert_eq!(tops.total(), rebuilt.total());
        assert!((0..tops.len()).all(|index| tops.top(index) == rebuilt.top(index)));
    }

    #[test]
    fn a_thread_frame_insets_the_card_and_adds_its_padding() {
        let line = ThreadFrame::of(&ThreadAnchor::line("a.ts", Side::New, 1), None);
        assert_eq!(line.card_width(900.0), 900.0 - 86.0 - 16.0);
        assert_eq!(line.row_height(100.0), 118.0);
        let file = ThreadFrame::of(&ThreadAnchor::file("a.ts"), None);
        assert_eq!(file.card_width(900.0), 868.0);
        assert_eq!(file.row_height(100.0), 121.0);
        assert_eq!(ThreadFrame::of(&ThreadAnchor::Change, None), file);
    }

    #[test]
    fn the_top_of_the_pane_over_a_change_thread_belongs_to_the_first_file_without_a_pinned_header() {
        let files = files();
        let threads = [BlockSlot {
            id: BlockId::Thread(ThreadId(1)),
            anchor: ThreadAnchor::Change,
        }];
        let none = HashSet::new();
        let rows = build(
            &files,
            Visibility {
                collapsed: &none,
                loaded: &none,
                previews: &none,
            },
            &threads,
            DiffMode::Unified,
        );
        let tops = Tops::new(&rows.rows, &files, None);
        assert_eq!(position(&rows, &tops, 0.0), Some(Position { file: 0, sticky: None }));
        assert_eq!(
            position(&rows, &tops, THREAD - 1.0),
            Some(Position { file: 0, sticky: None })
        );
        assert_eq!(position(&rows, &tops, THREAD), Some(Position { file: 0, sticky: None }));
    }

    fn split_model(spec: &str, text: &str, wrap: Option<usize>) -> (Rows, Tops) {
        let files = vec![Arc::new(diff("a.ts", lines_with(spec, text)))];
        let none = HashSet::new();
        let rows = build(
            &files,
            Visibility {
                collapsed: &none,
                loaded: &none,
                previews: &none,
            },
            &[],
            DiffMode::Split,
        );
        let tops = Tops::new(&rows.rows, &files, wrap);
        (rows, tops)
    }

    #[test]
    fn split_rows_are_one_line_tall_without_wrap() {
        let (rows, tops) = split_model("cddaaa", &"x".repeat(500), None);
        let pairs = rows.rows.len() - 3;
        assert_eq!(pairs, 4);
        assert_eq!(tops.total(), FILE_HEADER + HUNK + pairs as f32 * LINE + GAP);
    }

    #[test]
    fn a_split_row_is_as_tall_as_its_taller_side() {
        let files = vec![Arc::new(diff(
            "a.ts",
            vec![
                line(LineKind::Deleted, Some(1), None, &"d".repeat(25)),
                line(LineKind::Added, None, Some(1), "short"),
                line(LineKind::Deleted, Some(2), None, "short"),
                line(LineKind::Added, None, Some(2), &"a".repeat(45)),
                line(LineKind::Context, Some(3), Some(3), &"c".repeat(35)),
                line(LineKind::Added, None, Some(4), &"n".repeat(11)),
            ],
        ))];
        let none = HashSet::new();
        let rows = build(
            &files,
            Visibility {
                collapsed: &none,
                loaded: &none,
                previews: &none,
            },
            &[],
            DiffMode::Split,
        );
        let tops = Tops::new(&rows.rows, &files, Some(10));
        let heights: Vec<f32> = (2..rows.rows.len() - 1).map(|index| tops.height(index)).collect();
        assert_eq!(heights, [3.0 * LINE, 5.0 * LINE, 4.0 * LINE, 2.0 * LINE]);
    }

    #[test]
    fn a_split_row_is_as_tall_as_the_chunks_it_paints() {
        let texts = [
            "",
            "abc",
            "0123456789",
            "01234567890",
            "\t\tab",
            "\u{4e2d}\u{4e2d}\u{4e2d}a",
        ];
        for cols in [1, 3, 10] {
            for left in texts {
                for right in texts {
                    let lines = vec![
                        line(LineKind::Deleted, Some(1), None, left),
                        line(LineKind::Added, None, Some(1), right),
                    ];
                    let files = vec![Arc::new(diff("a.ts", lines))];
                    let pair = Row {
                        file: 0,
                        kind: RowKind::Pair {
                            hunk: 0,
                            left: Some(0),
                            right: Some(1),
                        },
                    };
                    let painted = wrap_ranges(left, cols).len().max(wrap_ranges(right, cols).len());
                    assert_eq!(
                        row_height(pair, &files, Some(cols), &[], &HashMap::new()),
                        painted as f32 * LINE,
                        "{left:?} | {right:?} at {cols}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_pair_row_with_one_side_wraps_like_a_unified_line() {
        let (rows, tops) = split_model("aa", &"x".repeat(25), Some(10));
        assert_eq!(rows.rows.len(), 5);
        assert_eq!(tops.height(2), 3.0 * LINE);
        assert_eq!(tops.height(3), 3.0 * LINE);
    }

    #[test]
    fn two_split_columns_share_the_pane_around_a_divider() {
        assert_eq!(column_width(1001.0), 500.0);
        assert_eq!(column_width(901.0) * 2.0 + SPLIT_DIVIDER, 901.0);
        assert_eq!(split_columns_for(1001.0, 7.0), 63);
        assert_eq!(split_columns_for(10.0, 7.0), 1);
        assert!(split_columns_for(1200.0, 7.2) < columns_for(1200.0, 7.2));
    }

    #[test]
    fn split_needs_a_wide_pane() {
        assert_eq!(shown_mode(DiffMode::Split, SPLIT_MIN_WIDTH), DiffMode::Split);
        assert_eq!(shown_mode(DiffMode::Split, SPLIT_MIN_WIDTH - 0.5), DiffMode::Unified);
        assert_eq!(shown_mode(DiffMode::Unified, 2000.0), DiffMode::Unified);
    }

    #[test]
    fn a_split_thread_fills_the_half_of_its_side() {
        let width = 1001.0;
        let old = ThreadFrame::of(&ThreadAnchor::line("a.ts", Side::Old, 1), Some(width));
        let new = ThreadFrame::of(&ThreadAnchor::line("a.ts", Side::New, 1), Some(width));
        let column = column_width(width);
        assert_eq!(old.left, SPLIT_THREAD_LEFT);
        assert_eq!(old.left + old.card_width(width), column - THREAD_RIGHT);
        assert_eq!(new.left, column + SPLIT_DIVIDER + SPLIT_THREAD_LEFT);
        assert_eq!(new.left + new.card_width(width), width - THREAD_RIGHT);
        assert_eq!(old.card_width(width), new.card_width(width));
        assert_eq!(old.row_height(100.0), 118.0);
        let file = ThreadFrame::of(&ThreadAnchor::file("a.ts"), Some(width));
        assert_eq!(file, ThreadFrame::of(&ThreadAnchor::file("a.ts"), None));
    }
}
