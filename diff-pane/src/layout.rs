use crate::rows::{Row, RowKind, Rows};
use diffpilot_core::diff::FileDiff;
use diffpilot_core::threads::ThreadAnchor;
use std::ops::Range;
use std::sync::Arc;
use unicode_width::UnicodeWidthChar;

pub const FILE_HEADER: f32 = 38.0;
pub const HUNK: f32 = 28.0;
pub const LINE: f32 = 20.0;
pub const GAP: f32 = 12.0;
pub const NOTICE: f32 = 96.0;
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

/// Space around a thread card inside its row: left, right, top, bottom. A line thread sits under the code
/// gutter. A file or change thread spans the pane and has a rule below it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThreadFrame {
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
    pub rule: bool,
}

impl ThreadFrame {
    pub fn of(anchor: &ThreadAnchor) -> Self {
        match anchor {
            ThreadAnchor::Line { .. } => Self {
                left: LINE_THREAD_LEFT,
                right: THREAD_RIGHT,
                top: LINE_THREAD_TOP,
                bottom: LINE_THREAD_BOTTOM,
                rule: false,
            },
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

pub fn row_height(row: Row, files: &[Arc<FileDiff>], wrap: Option<usize>, threads: &[f32]) -> f32 {
    match row.kind {
        RowKind::FileHeader => FILE_HEADER,
        RowKind::Hunk { .. } => HUNK,
        RowKind::Gap => GAP,
        RowKind::Notice(_) => NOTICE,
        RowKind::Thread { slot } => threads.get(slot as usize).copied().unwrap_or(THREAD),
        RowKind::Line { hunk, line } => {
            let Some(cols) = wrap else { return LINE };
            let text = &files[row.file as usize].hunks[hunk as usize].lines[line as usize].text;
            LINE * wrapped_lines(text, cols) as f32
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
        let mut tops = Vec::with_capacity(rows.len() + 1);
        let mut y = 0.0;
        tops.push(y);
        for &row in rows {
            y += row_height(row, files, wrap, threads);
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
    use crate::rows::{ThreadSlot, Visibility, build};
    use crate::testing::{added, diff};
    use diffpilot_core::diff::DiffLine;
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
            },
            &[],
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
            },
            &[],
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
        let threads = [ThreadSlot {
            id: ThreadId(1),
            anchor: ThreadAnchor::file("a.ts"),
        }];
        let none = HashSet::new();
        let rows = build(
            &files,
            Visibility {
                collapsed: &none,
                loaded: &none,
            },
            &threads,
        );
        let row = rows.thread_row(0).unwrap();
        assert_eq!(Tops::new(&rows.rows, &files, None).height(row), THREAD);
        let tops = Tops::with_threads(&rows.rows, &files, None, &[200.0]);
        assert_eq!(tops.height(row), 200.0);
    }

    #[test]
    fn resizing_a_row_in_place_matches_a_rebuild() {
        let files = files();
        let threads = [ThreadSlot {
            id: ThreadId(1),
            anchor: ThreadAnchor::file("a.ts"),
        }];
        let none = HashSet::new();
        let rows = build(
            &files,
            Visibility {
                collapsed: &none,
                loaded: &none,
            },
            &threads,
        );
        let row = rows.thread_row(0).unwrap();
        let mut tops = Tops::with_threads(&rows.rows, &files, None, &[150.0]);
        tops.set_height(row, 260.0);
        let rebuilt = Tops::with_threads(&rows.rows, &files, None, &[260.0]);
        assert_eq!(tops.total(), rebuilt.total());
        assert!((0..tops.len()).all(|index| tops.top(index) == rebuilt.top(index)));
    }

    #[test]
    fn a_thread_frame_insets_the_card_and_adds_its_padding() {
        let line = ThreadFrame::of(&ThreadAnchor::line("a.ts", Side::New, 1));
        assert_eq!(line.card_width(900.0), 900.0 - 86.0 - 16.0);
        assert_eq!(line.row_height(100.0), 118.0);
        let file = ThreadFrame::of(&ThreadAnchor::file("a.ts"));
        assert_eq!(file.card_width(900.0), 868.0);
        assert_eq!(file.row_height(100.0), 121.0);
        assert_eq!(ThreadFrame::of(&ThreadAnchor::Change), file);
    }

    #[test]
    fn the_top_of_the_pane_over_a_change_thread_belongs_to_the_first_file_without_a_pinned_header() {
        let files = files();
        let threads = [ThreadSlot {
            id: ThreadId(1),
            anchor: ThreadAnchor::Change,
        }];
        let none = HashSet::new();
        let rows = build(
            &files,
            Visibility {
                collapsed: &none,
                loaded: &none,
            },
            &threads,
        );
        let tops = Tops::new(&rows.rows, &files, None);
        assert_eq!(position(&rows, &tops, 0.0), Some(Position { file: 0, sticky: None }));
        assert_eq!(
            position(&rows, &tops, THREAD - 1.0),
            Some(Position { file: 0, sticky: None })
        );
        assert_eq!(position(&rows, &tops, THREAD), Some(Position { file: 0, sticky: None }));
    }
}
