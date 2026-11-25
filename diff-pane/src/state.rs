use crate::layout::{CODE_PADDING, GUTTER, Position, THREAD, Tops, cells, columns_for, position};
use crate::rows::{RowRef, Rows, ThreadSlot, Visibility, body_notice, build};
use diffpilot_core::diff::FileDiff;
use diffpilot_core::threads::ThreadId;
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::Arc;

const LOOKAHEAD_FILES: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geometry {
    pub width: f32,
    pub height: f32,
    pub cell: f32,
}

/// The row at the top of the viewport and how far the viewport is into it. `from_header` is the fallback
/// for a row that no longer exists.
#[derive(Clone, Debug, PartialEq)]
struct Anchor {
    path: String,
    row: RowRef,
    in_row: f32,
    from_header: f32,
}

/// Everything the pane knows that does not need a window: files, rows, heights and the scroll offset.
pub struct PaneState {
    pub files: Vec<Arc<FileDiff>>,
    pub rows: Rows,
    pub tops: Tops,
    pub collapsed: HashSet<String>,
    pub viewed: HashSet<String>,
    pub loaded: HashSet<String>,
    pub threads: Vec<ThreadSlot>,
    thread_heights: Vec<f32>,
    wrap: bool,
    geometry: Geometry,
    scroll: f32,
    scroll_x: f32,
    widest: Option<usize>,
    reported: Option<String>,
    index: HashMap<String, usize>,
}

impl PaneState {
    pub fn new(geometry: Geometry) -> Self {
        Self {
            files: Vec::new(),
            rows: Rows::default(),
            tops: Tops::default(),
            collapsed: HashSet::new(),
            viewed: HashSet::new(),
            loaded: HashSet::new(),
            threads: Vec::new(),
            thread_heights: Vec::new(),
            wrap: true,
            geometry,
            scroll: 0.0,
            scroll_x: 0.0,
            widest: None,
            reported: None,
            index: HashMap::new(),
        }
    }

    pub fn wrap(&self) -> bool {
        self.wrap
    }

    pub fn geometry(&self) -> Geometry {
        self.geometry
    }

    pub fn scroll(&self) -> f32 {
        self.scroll
    }

    pub fn wrap_cols(&self) -> Option<usize> {
        self.wrap.then(|| columns_for(self.geometry.width, self.geometry.cell))
    }

    pub fn file_index(&self, path: &str) -> Option<usize> {
        self.index.get(path).copied()
    }

    pub fn max_scroll(&self) -> f32 {
        (self.tops.total() - self.geometry.height).max(0.0)
    }

    pub fn scroll_x(&self) -> f32 {
        self.scroll_x
    }

    pub fn max_scroll_x(&mut self) -> f32 {
        if self.wrap {
            return 0.0;
        }
        let widest = *self.widest.get_or_insert_with(|| {
            self.files
                .iter()
                .flat_map(|file| &file.hunks)
                .flat_map(|hunk| &hunk.lines)
                .map(|line| cells(&line.text))
                .max()
                .unwrap_or(0)
        });
        let code = widest as f32 * self.geometry.cell + CODE_PADDING;
        (code - (self.geometry.width - GUTTER + CODE_PADDING)).max(0.0)
    }

    pub fn scroll_x_by(&mut self, dx: f32) {
        let max = self.max_scroll_x();
        self.scroll_x = (self.scroll_x + dx).clamp(0.0, max);
    }

    pub fn scroll_to(&mut self, y: f32) {
        self.scroll = y.clamp(0.0, self.max_scroll());
    }

    pub fn scroll_by(&mut self, dy: f32) {
        self.scroll_to(self.scroll + dy);
    }

    pub fn position(&self) -> Option<Position> {
        position(&self.rows, &self.tops, self.scroll)
    }

    pub fn visible_path(&self) -> Option<&str> {
        let file = self.position()?.file as usize;
        self.files.get(file).map(|file| file.path.as_str())
    }

    /// The top file when it differs from the last one this returned.
    pub fn take_visible_change(&mut self) -> Option<String> {
        let current = self.visible_path().map(str::to_owned);
        if current == self.reported {
            return None;
        }
        self.reported = current.clone();
        current
    }

    pub fn visible_rows(&self) -> Range<usize> {
        self.tops.window(self.scroll, self.geometry.height)
    }

    /// Files whose highlights are worth having now, nearest first: the ones in view, then a few below.
    /// Files whose lines are not shown (collapsed, binary, hidden as large or generated) are left out.
    pub fn files_to_highlight(&self) -> Vec<usize> {
        let window = self.visible_rows();
        let (Some(first), Some(last)) = (
            self.rows.rows.get(window.start),
            self.rows.rows.get(window.end.saturating_sub(1)),
        ) else {
            return Vec::new();
        };
        let (first, last) = (first.file as usize, last.file as usize);
        (first..=(last + LOOKAHEAD_FILES).min(self.files.len().saturating_sub(1)))
            .filter(|&index| self.shows_lines(index))
            .collect()
    }

    fn shows_lines(&self, index: usize) -> bool {
        let file = &self.files[index];
        !self.collapsed.contains(&file.path) && body_notice(file, self.loaded.contains(&file.path)).is_none()
    }

    pub fn replace_files(&mut self, files: Vec<FileDiff>) -> Vec<Option<usize>> {
        let anchor = self.anchor();
        let mut reused = Vec::with_capacity(files.len());
        let mut next = Vec::with_capacity(files.len());
        for file in files {
            let old = self
                .index
                .get(&file.path)
                .copied()
                .filter(|&index| *self.files[index] == file);
            reused.push(old);
            next.push(match old {
                Some(index) => Arc::clone(&self.files[index]),
                None => Arc::new(file),
            });
        }
        self.files = next;
        self.widest = None;
        self.index = self
            .files
            .iter()
            .enumerate()
            .map(|(index, file)| (file.path.clone(), index))
            .collect();
        self.relayout(anchor);
        reused
    }

    pub fn set_viewed(&mut self, viewed: HashSet<String>) {
        let anchor = self.anchor();
        for path in viewed.difference(&self.viewed) {
            self.collapsed.insert(path.clone());
        }
        for path in self.viewed.difference(&viewed) {
            self.collapsed.remove(path);
        }
        self.viewed = viewed;
        self.relayout(anchor);
    }

    pub fn toggle_collapsed(&mut self, path: &str) {
        let anchor = self.anchor();
        if !self.collapsed.remove(path) {
            self.collapsed.insert(path.to_owned());
        }
        self.relayout(anchor);
    }

    pub fn load_diff(&mut self, path: &str) {
        let anchor = self.anchor();
        self.loaded.insert(path.to_owned());
        self.relayout(anchor);
    }

    pub fn set_wrap(&mut self, wrap: bool) {
        if wrap != self.wrap {
            let anchor = self.anchor();
            self.wrap = wrap;
            self.scroll_x = 0.0;
            self.retop(anchor);
        }
    }

    pub fn set_geometry(&mut self, geometry: Geometry) {
        if geometry == self.geometry {
            return;
        }
        let anchor = self.anchor();
        let before = self.wrap_cols();
        self.geometry = geometry;
        if self.wrap_cols() == before {
            self.scroll_to(self.scroll);
        } else {
            self.retop(anchor);
        }
        self.scroll_x_by(0.0);
    }

    pub fn reveal(&mut self, path: &str) -> bool {
        let Some(top) = self.header_top(path) else { return false };
        self.scroll_to(top);
        true
    }

    fn header_top(&self, path: &str) -> Option<f32> {
        let file = self.file_index(path)?;
        Some(self.tops.top(self.rows.header_index(file)?))
    }

    fn anchor(&self) -> Option<Anchor> {
        let index = self.tops.row_at(self.scroll)?;
        let path = self.files.get(self.rows.rows[index].file as usize)?.path.clone();
        let header = self.header_top(&path)?;
        Some(Anchor {
            row: self.rows.row_ref(index, &self.threads),
            in_row: self.scroll - self.tops.top(index),
            from_header: (self.scroll - header).max(0.0),
            path,
        })
    }

    fn anchored_scroll(&self, anchor: &Anchor) -> Option<f32> {
        let file = self.file_index(&anchor.path);
        let found = match (anchor.row, file) {
            (RowRef::Thread(_), _) => self.rows.find(0, anchor.row, &self.threads),
            (row, Some(file)) => self.rows.find(file, row, &self.threads),
            (_, None) => None,
        };
        if let Some(index) = found {
            return Some(self.tops.top(index) + anchor.in_row.min(self.tops.height(index) - 1.0).max(0.0));
        }
        let range = self.rows.file_range(file?)?;
        let top = self.tops.top(range.start);
        let height = self.tops.top(range.end) - top;
        let offset = if self.collapsed.contains(&anchor.path) {
            0.0
        } else {
            anchor.from_header.min(height - 1.0).max(0.0)
        };
        Some(top + offset)
    }

    fn restore(&mut self, anchor: Option<Anchor>) {
        let target = anchor.and_then(|anchor| self.anchored_scroll(&anchor));
        self.scroll_to(target.unwrap_or(self.scroll));
    }

    fn relayout(&mut self, anchor: Option<Anchor>) {
        self.rows = build(
            &self.files,
            Visibility {
                collapsed: &self.collapsed,
                loaded: &self.loaded,
            },
            &self.threads,
        );
        self.retop(anchor);
    }

    fn retop(&mut self, anchor: Option<Anchor>) {
        self.tops = Tops::with_threads(&self.rows.rows, &self.files, self.wrap_cols(), &self.thread_heights);
        self.restore(anchor);
    }

    /// Replaces the thread rows. A thread that was already placed keeps its height.
    pub fn set_threads(&mut self, threads: Vec<ThreadSlot>) {
        let anchor = self.anchor();
        let known: HashMap<ThreadId, f32> = self
            .threads
            .iter()
            .map(|thread| thread.id)
            .zip(self.thread_heights.iter().copied())
            .collect();
        self.thread_heights = threads
            .iter()
            .map(|thread| known.get(&thread.id).copied().unwrap_or(THREAD))
            .collect();
        self.threads = threads;
        self.relayout(anchor);
    }

    pub fn thread_height(&self, id: ThreadId) -> Option<f32> {
        let slot = self.threads.iter().position(|thread| thread.id == id)?;
        Some(self.thread_heights[slot])
    }

    pub fn thread_top(&self, id: ThreadId) -> Option<f32> {
        let slot = self.threads.iter().position(|thread| thread.id == id)?;
        Some(self.tops.top(self.rows.thread_row(slot as u32)?))
    }

    /// Sets the height of a thread's row. Rows below it move. A row above the viewport keeps the
    /// content in view where it is, and a row under the viewport top grows downward.
    pub fn set_thread_height(&mut self, id: ThreadId, height: f32) -> bool {
        let Some(slot) = self.threads.iter().position(|thread| thread.id == id) else {
            return false;
        };
        if self.thread_heights[slot] == height {
            return false;
        }
        self.thread_heights[slot] = height;
        if let Some(row) = self.rows.thread_row(slot as u32) {
            let (top, old) = (self.tops.top(row), self.tops.height(row));
            self.tops.set_height(row, height);
            if top + old <= self.scroll {
                self.scroll += height - old;
            }
            self.scroll_to(self.scroll);
        }
        true
    }

    /// Scrolls just enough to show the thread under the pinned header of height `inset`.
    pub fn reveal_thread(&mut self, id: ThreadId, inset: f32) -> bool {
        let Some(slot) = self.threads.iter().position(|thread| thread.id == id) else {
            return false;
        };
        let Some(row) = self.rows.thread_row(slot as u32) else {
            return false;
        };
        let (top, bottom) = (self.tops.top(row), self.tops.top(row + 1));
        let shown = top >= self.scroll + inset && bottom <= self.scroll + self.geometry.height;
        if !shown {
            self.scroll_to(top - inset);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{FILE_HEADER, GAP, HUNK, LINE, NOTICE};
    use crate::testing::{added, diff};
    use diffpilot_core::threads::{Side, ThreadAnchor};

    const GEOMETRY: Geometry = Geometry {
        width: 400.0,
        height: 100.0,
        cell: 7.0,
    };

    fn state(files: Vec<FileDiff>) -> PaneState {
        let mut state = PaneState::new(GEOMETRY);
        state.replace_files(files);
        state
    }

    fn three_files() -> Vec<FileDiff> {
        (0..3)
            .map(|i| diff(&format!("f{i}.ts"), (1..=10).map(|n| added(n, "line")).collect()))
            .collect()
    }

    const FILE_HEIGHT: f32 = FILE_HEADER + HUNK + 10.0 * LINE + GAP;

    fn paths(state: &PaneState) -> Vec<&str> {
        state.files.iter().map(|file| file.path.as_str()).collect()
    }

    #[test]
    fn starts_at_the_first_file() {
        let mut state = state(three_files());
        assert_eq!(state.visible_path(), Some("f0.ts"));
        assert_eq!(state.take_visible_change().as_deref(), Some("f0.ts"));
        assert_eq!(state.take_visible_change(), None);
    }

    #[test]
    fn reveal_puts_the_file_header_at_the_top() {
        let mut state = state(three_files());
        assert!(state.reveal("f1.ts"));
        assert_eq!(state.scroll(), FILE_HEIGHT);
        assert_eq!(state.visible_path(), Some("f1.ts"));
        assert!(!state.reveal("missing.ts"));
        assert_eq!(state.visible_path(), Some("f1.ts"));
    }

    #[test]
    fn reveal_stops_at_the_end_when_the_last_file_is_too_short_to_reach_the_top() {
        let mut state = PaneState::new(Geometry {
            height: 400.0,
            ..GEOMETRY
        });
        state.replace_files(three_files());
        state.reveal("f2.ts");
        assert_eq!(state.scroll(), state.max_scroll());
        assert!(state.scroll() < 2.0 * FILE_HEIGHT);
    }

    #[test]
    fn the_visible_file_follows_the_scroll_offset() {
        let mut state = state(three_files());
        state.scroll_to(FILE_HEIGHT - 1.0);
        assert_eq!(state.visible_path(), Some("f0.ts"));
        state.scroll_to(FILE_HEIGHT);
        assert_eq!(state.visible_path(), Some("f1.ts"));
        assert_eq!(state.take_visible_change().as_deref(), Some("f1.ts"));
        state.scroll_by(-5.0);
        assert_eq!(state.take_visible_change().as_deref(), Some("f0.ts"));
    }

    #[test]
    fn scrolling_stays_inside_the_content() {
        let mut state = state(three_files());
        state.scroll_by(-50.0);
        assert_eq!(state.scroll(), 0.0);
        state.scroll_by(1e9);
        assert_eq!(state.scroll(), 3.0 * FILE_HEIGHT - GEOMETRY.height);
    }

    #[test]
    fn viewed_files_collapse_and_unviewed_files_expand() {
        let mut state = state(three_files());
        let full = state.tops.total();
        state.set_viewed(["f1.ts".to_string()].into());
        assert!(state.collapsed.contains("f1.ts"));
        assert_eq!(state.tops.total(), full - (FILE_HEIGHT - FILE_HEADER - GAP));
        state.set_viewed(HashSet::new());
        assert!(state.collapsed.is_empty());
        assert_eq!(state.tops.total(), full);
    }

    #[test]
    fn collapsing_by_hand_is_independent_of_viewed() {
        let mut state = state(three_files());
        state.toggle_collapsed("f0.ts");
        assert!(state.collapsed.contains("f0.ts"));
        assert!(state.viewed.is_empty());
        state.toggle_collapsed("f0.ts");
        assert!(state.collapsed.is_empty());
    }

    #[test]
    fn collapsing_a_file_above_keeps_the_viewport_on_the_same_content() {
        let mut state = state(three_files());
        state.scroll_to(FILE_HEIGHT + 3.0 * LINE);
        assert_eq!(state.visible_path(), Some("f1.ts"));
        let before = state.scroll() - state.tops.top(state.rows.header_index(1).unwrap());
        state.toggle_collapsed("f0.ts");
        let after = state.scroll() - state.tops.top(state.rows.header_index(1).unwrap());
        assert_eq!(before, after);
    }

    #[test]
    fn collapsing_the_top_file_scrolls_to_its_header() {
        let mut state = state(three_files());
        state.scroll_to(FILE_HEADER + HUNK + 5.0 * LINE);
        state.toggle_collapsed("f0.ts");
        assert_eq!(state.scroll(), 0.0);
        assert_eq!(state.visible_path(), Some("f0.ts"));
    }

    #[test]
    fn reload_keeps_the_scroll_anchor() {
        let mut state = state(three_files());
        state.scroll_to(FILE_HEIGHT + 2.0 * LINE);
        let mut next = three_files();
        next.insert(0, diff("a.ts", vec![added(1, "new first file")]));
        state.replace_files(next);
        assert_eq!(paths(&state), ["a.ts", "f0.ts", "f1.ts", "f2.ts"]);
        assert_eq!(state.visible_path(), Some("f1.ts"));
        let header = state.tops.top(state.rows.header_index(2).unwrap());
        assert_eq!(state.scroll(), header + 2.0 * LINE);
    }

    #[test]
    fn reload_without_the_top_file_stays_in_range() {
        let mut state = state(three_files());
        state.reveal("f2.ts");
        state.replace_files(vec![diff("only.ts", vec![added(1, "x")])]);
        assert!(state.scroll() <= state.max_scroll());
        assert_eq!(state.visible_path(), Some("only.ts"));
    }

    #[test]
    fn reload_shares_unchanged_files_and_reports_them() {
        let mut state = state(three_files());
        let before = Arc::clone(&state.files[1]);
        let mut next = three_files();
        next[2] = diff("f2.ts", vec![added(1, "different")]);
        next.insert(0, diff("a.ts", vec![added(1, "x")]));
        let reused = state.replace_files(next);
        assert_eq!(reused, [None, Some(0), Some(1), None]);
        assert!(Arc::ptr_eq(&before, &state.files[2]));
    }

    #[test]
    fn wrap_makes_long_lines_taller_and_unwrapping_restores_them() {
        let long = "x".repeat(200);
        let mut state = PaneState::new(GEOMETRY);
        state.replace_files(vec![diff("a.ts", vec![added(1, &long)])]);
        let wrapped = state.tops.total();
        assert!(state.wrap());
        assert!(wrapped > FILE_HEADER + HUNK + LINE + GAP);
        state.set_wrap(false);
        assert_eq!(state.tops.total(), FILE_HEADER + HUNK + LINE + GAP);
        state.set_wrap(true);
        assert_eq!(state.tops.total(), wrapped);
    }

    #[test]
    fn narrower_panes_wrap_more() {
        let long = "x".repeat(100);
        let mut state = PaneState::new(GEOMETRY);
        state.replace_files(vec![diff("a.ts", vec![added(1, &long)])]);
        let wide = state.tops.total();
        state.set_geometry(Geometry {
            width: 250.0,
            ..GEOMETRY
        });
        assert!(state.tops.total() > wide);
    }

    #[test]
    fn taller_panes_only_change_the_scroll_limit() {
        let mut state = state(three_files());
        state.scroll_to(1e9);
        state.set_geometry(Geometry {
            height: 400.0,
            ..GEOMETRY
        });
        assert_eq!(state.scroll(), state.max_scroll());
    }

    #[test]
    fn highlights_are_wanted_for_visible_files_and_two_more() {
        let files: Vec<_> = (0..8).map(|i| diff(&format!("f{i}.ts"), vec![added(1, "x")])).collect();
        let mut state = state(files);
        assert_eq!(state.files_to_highlight(), [0, 1, 2, 3]);
        state.reveal("f3.ts");
        assert_eq!(state.files_to_highlight(), [3, 4, 5, 6]);
    }

    #[test]
    fn files_that_hide_their_lines_are_not_highlighted() {
        let mut binary = diff("logo.png", Vec::new());
        binary.is_binary = true;
        let mut lock = diff("pnpm-lock.yaml", vec![added(1, "x")]);
        lock.additions = 1;
        let files = vec![
            diff("a.ts", vec![added(1, "x")]),
            binary,
            lock,
            diff("b.ts", vec![added(1, "x")]),
        ];
        let mut state = state(files);
        assert_eq!(state.files_to_highlight(), [0, 3]);
        state.toggle_collapsed("a.ts");
        assert_eq!(state.files_to_highlight(), [3]);
        state.load_diff("pnpm-lock.yaml");
        assert_eq!(state.files_to_highlight(), [2, 3]);
    }

    #[test]
    fn loading_a_big_diff_adds_its_lines() {
        let mut big = diff("big.ts", vec![added(1, "x")]);
        big.additions = 5000;
        let mut state = state(vec![big]);
        assert_eq!(state.tops.total(), FILE_HEADER + NOTICE + GAP);
        state.load_diff("big.ts");
        assert_eq!(state.tops.total(), FILE_HEADER + HUNK + LINE + GAP);
    }

    #[test]
    fn unwrapped_panes_scroll_sideways_to_the_longest_line() {
        let mut state = PaneState::new(GEOMETRY);
        state.replace_files(vec![diff("a.ts", vec![added(1, &"x".repeat(100)), added(2, "short")])]);
        assert_eq!(state.max_scroll_x(), 0.0, "wrapped text never needs it");
        state.set_wrap(false);
        let max = state.max_scroll_x();
        assert!(max > 0.0);
        assert_eq!(
            max,
            100.0 * GEOMETRY.cell + CODE_PADDING - (GEOMETRY.width - GUTTER + CODE_PADDING)
        );
        state.scroll_x_by(1e9);
        assert_eq!(state.scroll_x(), max);
        state.scroll_x_by(-1e9);
        assert_eq!(state.scroll_x(), 0.0);
        state.scroll_x_by(10.0);
        state.set_wrap(true);
        assert_eq!(state.scroll_x(), 0.0);
    }

    #[test]
    fn an_empty_pane_has_nothing_visible() {
        let mut state = PaneState::new(GEOMETRY);
        assert_eq!(state.visible_path(), None);
        assert_eq!(state.take_visible_change(), None);
        assert!(state.files_to_highlight().is_empty());
        assert!(!state.reveal("x"));
    }

    fn slot(id: u32, anchor: ThreadAnchor) -> ThreadSlot {
        ThreadSlot {
            id: ThreadId(id),
            anchor,
        }
    }

    fn line_thread(id: u32, path: &str, line: u32) -> ThreadSlot {
        slot(id, ThreadAnchor::line(path, Side::New, line))
    }

    fn content_at_top(state: &PaneState) -> (usize, f32) {
        let row = state.tops.row_at(state.scroll()).unwrap();
        (row, state.scroll() - state.tops.top(row))
    }

    #[test]
    fn new_thread_rows_start_at_the_placeholder_and_keep_a_measured_height() {
        let mut state = state(three_files());
        let before = state.tops.total();
        state.set_threads(vec![line_thread(1, "f0.ts", 3)]);
        assert_eq!(state.thread_height(ThreadId(1)), Some(THREAD));
        assert_eq!(state.tops.total(), before + THREAD);
        assert!(state.set_thread_height(ThreadId(1), 220.0));
        assert!(!state.set_thread_height(ThreadId(1), 220.0));
        assert_eq!(state.tops.total(), before + 220.0);
        state.set_threads(vec![line_thread(1, "f0.ts", 3), line_thread(2, "f1.ts", 1)]);
        assert_eq!(state.thread_height(ThreadId(1)), Some(220.0));
        assert_eq!(state.thread_height(ThreadId(2)), Some(THREAD));
        assert_eq!(state.tops.total(), before + 220.0 + THREAD);
        assert!(!state.set_thread_height(ThreadId(9), 10.0));
    }

    #[test]
    fn a_thread_above_the_viewport_growing_keeps_the_content_in_view() {
        let mut state = state(three_files());
        state.set_threads(vec![line_thread(1, "f0.ts", 3)]);
        state.scroll_to(FILE_HEIGHT + 3.0 * LINE);
        let before = content_at_top(&state);
        let at = state.scroll();
        state.set_thread_height(ThreadId(1), 400.0);
        assert_eq!(state.scroll(), at + (400.0 - THREAD));
        assert_eq!(content_at_top(&state), (before.0, before.1));
        state.set_thread_height(ThreadId(1), 100.0);
        assert_eq!(state.scroll(), at - (THREAD - 100.0));
        assert_eq!(content_at_top(&state), (before.0, before.1));
    }

    #[test]
    fn a_thread_under_the_viewport_top_grows_downward_and_one_below_moves_nothing() {
        let mut state = state(three_files());
        state.set_threads(vec![line_thread(1, "f0.ts", 3), line_thread(2, "f2.ts", 3)]);
        let row = state.rows.thread_row(0).unwrap();
        let top = state.tops.top(row);
        state.scroll_to(top + 30.0);
        state.set_thread_height(ThreadId(1), 400.0);
        assert_eq!(state.scroll(), top + 30.0);
        state.set_thread_height(ThreadId(2), 400.0);
        assert_eq!(state.scroll(), top + 30.0);
    }

    #[test]
    fn adding_and_removing_a_thread_above_the_viewport_keeps_the_content_in_view() {
        let mut state = state(three_files());
        state.scroll_to(FILE_HEIGHT + 4.0 * LINE);
        let before = content_at_top(&state);
        let before_row = state.rows.row_ref(before.0, &state.threads);
        state.set_threads(vec![line_thread(1, "f0.ts", 3)]);
        let after = content_at_top(&state);
        assert_eq!(state.rows.row_ref(after.0, &state.threads), before_row);
        assert_eq!(after.1, before.1);
        assert_eq!(state.scroll(), FILE_HEIGHT + 4.0 * LINE + THREAD);
        state.set_threads(Vec::new());
        assert_eq!(state.scroll(), FILE_HEIGHT + 4.0 * LINE);
    }

    #[test]
    fn a_change_thread_sits_above_the_first_file() {
        let mut state = state(three_files());
        let full = state.tops.total();
        state.set_threads(vec![slot(1, ThreadAnchor::Change)]);
        assert_eq!(state.tops.total(), full + THREAD);
        assert_eq!(state.visible_path(), Some("f0.ts"));
        assert_eq!(state.thread_top(ThreadId(1)), Some(0.0));
        state.scroll_to(30.0);
        state.toggle_collapsed("f1.ts");
        assert_eq!(state.scroll(), 30.0);
        state.set_thread_height(ThreadId(1), 300.0);
        assert_eq!(state.scroll(), 30.0);
        assert_eq!(state.visible_path(), Some("f0.ts"));
    }

    #[test]
    fn revealing_a_thread_scrolls_only_when_it_is_not_fully_shown() {
        let mut state = state(three_files());
        state.set_threads(vec![line_thread(1, "f2.ts", 9)]);
        let top = state.thread_top(ThreadId(1)).unwrap();
        assert!(state.reveal_thread(ThreadId(1), 66.0));
        assert_eq!(state.scroll(), (top - 66.0).min(state.max_scroll()));
        let shown = state.scroll();
        assert!(state.reveal_thread(ThreadId(1), 66.0));
        assert_eq!(state.scroll(), shown);
        assert!(!state.reveal_thread(ThreadId(9), 66.0));
    }

    #[test]
    fn collapsing_the_file_that_holds_the_top_thread_scrolls_to_its_header() {
        let mut state = state(three_files());
        state.set_threads(vec![line_thread(1, "f1.ts", 5)]);
        let top = state.thread_top(ThreadId(1)).unwrap();
        state.scroll_to(top + 10.0);
        assert_eq!(state.visible_path(), Some("f1.ts"));
        state.toggle_collapsed("f1.ts");
        let header = state.tops.top(state.rows.header_index(1).unwrap());
        assert_eq!(state.scroll(), header);
    }
}
