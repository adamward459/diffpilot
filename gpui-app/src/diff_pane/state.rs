use crate::diff_pane::layout::{
    CODE_PADDING, GUTTER, Position, SPLIT_GUTTER, THREAD, Tops, cells, column_width, columns_for, position, shown_mode,
    split_columns_for,
};
use crate::diff_pane::rows::{BlockId, BlockSlot, RowKind, RowRef, Rows, Visibility, body_notice, build};
use diffpilot_core::diff::{DiffMode, FileDiff};
use diffpilot_core::preview::preview_kind;
use diffpilot_core::threads::Side;
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
    all: Vec<Arc<FileDiff>>,
    filter: Option<Vec<String>>,
    /// The files the pane shows: all of them in diff order, or the filter's paths in the filter's order.
    pub files: Vec<Arc<FileDiff>>,
    pub rows: Rows,
    pub tops: Tops,
    pub collapsed: HashSet<String>,
    pub viewed: HashSet<String>,
    pub loaded: HashSet<String>,
    previews: HashSet<String>,
    preview_off: HashSet<String>,
    preview_heights: HashMap<String, f32>,
    preview_source: bool,
    pub blocks: Vec<BlockSlot>,
    block_heights: Vec<f32>,
    wrap: bool,
    mode: DiffMode,
    geometry: Geometry,
    scroll: f32,
    scroll_x: f32,
    widest: Option<usize>,
    reported: Option<String>,
    reported_mode: DiffMode,
    index: HashMap<String, usize>,
}

impl PaneState {
    pub fn new(geometry: Geometry) -> Self {
        Self {
            all: Vec::new(),
            filter: None,
            files: Vec::new(),
            rows: Rows::default(),
            tops: Tops::default(),
            collapsed: HashSet::new(),
            viewed: HashSet::new(),
            loaded: HashSet::new(),
            previews: HashSet::new(),
            preview_off: HashSet::new(),
            preview_heights: HashMap::new(),
            preview_source: false,
            blocks: Vec::new(),
            block_heights: Vec::new(),
            wrap: true,
            mode: DiffMode::Unified,
            geometry,
            scroll: 0.0,
            scroll_x: 0.0,
            widest: None,
            reported: None,
            reported_mode: DiffMode::Unified,
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

    /// The mode that was asked for. A pane too narrow for two columns draws unified rows anyway.
    pub fn mode(&self) -> DiffMode {
        self.mode
    }

    pub fn shown_mode(&self) -> DiffMode {
        shown_mode(self.mode, self.geometry.width)
    }

    pub fn split_width(&self) -> Option<f32> {
        (self.shown_mode() == DiffMode::Split).then_some(self.geometry.width)
    }

    pub fn take_shown_change(&mut self) -> Option<DiffMode> {
        let current = self.shown_mode();
        (current != std::mem::replace(&mut self.reported_mode, current)).then_some(current)
    }

    pub fn wrap_cols(&self) -> Option<usize> {
        let Geometry { width, cell, .. } = self.geometry;
        self.wrap.then(|| match self.shown_mode() {
            DiffMode::Unified => columns_for(width, cell),
            DiffMode::Split => split_columns_for(width, cell),
        })
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
        let room = match self.split_width() {
            Some(width) => column_width(width) - SPLIT_GUTTER,
            None => self.geometry.width - GUTTER,
        };
        (code - (room + CODE_PADDING)).max(0.0)
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

    fn nearby_files(&self) -> Range<usize> {
        let window = self.visible_rows();
        let (Some(first), Some(last)) = (
            self.rows.rows.get(window.start),
            self.rows.rows.get(window.end.saturating_sub(1)),
        ) else {
            return 0..0;
        };
        let (first, last) = (first.file as usize, last.file as usize);
        first..(last + LOOKAHEAD_FILES + 1).min(self.files.len())
    }

    /// Files whose highlights are worth having now, nearest first: the ones in view, then a few below.
    /// Files whose lines are not shown (collapsed, binary, hidden as large or generated) are left out.
    pub fn files_to_highlight(&self) -> Vec<usize> {
        self.nearby_files().filter(|&index| self.shows_lines(index)).collect()
    }

    pub fn previews_to_load(&self) -> Vec<usize> {
        self.nearby_files()
            .filter(|&index| {
                let path = &self.files[index].path;
                self.previews.contains(path) && !self.collapsed.contains(path)
            })
            .collect()
    }

    fn shows_lines(&self, index: usize) -> bool {
        let file = &self.files[index];
        !self.collapsed.contains(&file.path)
            && !self.previews.contains(&file.path)
            && body_notice(file, self.loaded.contains(&file.path)).is_none()
    }

    pub fn all_files(&self) -> &[Arc<FileDiff>] {
        &self.all
    }

    pub fn replace_files(&mut self, files: Vec<FileDiff>) -> Vec<Option<usize>> {
        let anchor = self.anchor();
        let known: HashMap<&str, &Arc<FileDiff>> = self.all.iter().map(|file| (file.path.as_str(), file)).collect();
        self.all = files
            .into_iter()
            .map(|file| match known.get(file.path.as_str()) {
                Some(old) if ***old == file => Arc::clone(old),
                _ => Arc::new(file),
            })
            .collect();
        self.refilter(anchor)
    }

    pub fn set_filter(&mut self, filter: Option<Vec<String>>) -> Vec<Option<usize>> {
        if filter == self.filter {
            return (0..self.files.len()).map(Some).collect();
        }
        let anchor = self.anchor();
        self.filter = filter;
        self.refilter(anchor)
    }

    pub fn filter(&self) -> Option<&[String]> {
        self.filter.as_deref()
    }

    /// For each file now shown, its index among the files shown before when the same diff was.
    fn refilter(&mut self, anchor: Option<Anchor>) -> Vec<Option<usize>> {
        let next: Vec<Arc<FileDiff>> = match &self.filter {
            None => self.all.clone(),
            Some(paths) => {
                let by_path: HashMap<&str, &Arc<FileDiff>> =
                    self.all.iter().map(|file| (file.path.as_str(), file)).collect();
                paths
                    .iter()
                    .filter_map(|path| by_path.get(path.as_str()).map(|file| Arc::clone(file)))
                    .collect()
            }
        };
        let reused = next
            .iter()
            .map(|file| {
                self.index
                    .get(&file.path)
                    .copied()
                    .filter(|&index| Arc::ptr_eq(&self.files[index], file))
            })
            .collect();
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

    pub fn set_preview_source(&mut self, on: bool) {
        if on == self.preview_source {
            return;
        }
        let anchor = self.anchor();
        self.preview_source = on;
        if !on {
            self.preview_heights.clear();
        }
        self.relayout(anchor);
    }

    pub fn previewable(&self, path: &str) -> bool {
        self.preview_source && preview_kind(path).is_some()
    }

    pub fn shows_preview(&self, path: &str) -> bool {
        self.previews.contains(path)
    }

    pub fn previews(&self) -> &HashSet<String> {
        &self.previews
    }

    pub fn toggle_preview(&mut self, path: &str) {
        if !self.previewable(path) {
            return;
        }
        let anchor = self.anchor();
        if !self.preview_off.remove(path) {
            self.preview_off.insert(path.to_owned());
        }
        self.relayout(anchor);
    }

    /// A row above the viewport keeps the content in view where it is.
    pub fn set_preview_height(&mut self, path: &str, height: f32) -> bool {
        if self.preview_heights.get(path) == Some(&height) {
            return false;
        }
        self.preview_heights.insert(path.to_owned(), height);
        let row = self.file_index(path).and_then(|file| {
            let range = self.rows.file_range(file)?;
            range
                .into_iter()
                .find(|&index| self.rows.rows[index].kind == RowKind::Preview)
        });
        if let Some(row) = row {
            let (top, old) = (self.tops.top(row), self.tops.height(row));
            self.tops.set_height(row, height);
            if top + old <= self.scroll {
                self.scroll += height - old;
            }
            self.scroll_to(self.scroll);
        }
        true
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

    pub fn set_mode(&mut self, mode: DiffMode) {
        if mode == self.mode {
            return;
        }
        let (anchor, shown) = (self.anchor(), self.shown_mode());
        self.mode = mode;
        if self.shown_mode() != shown {
            self.relayout(anchor);
            self.scroll_x_by(0.0);
        }
    }

    pub fn set_geometry(&mut self, geometry: Geometry) {
        if geometry == self.geometry {
            return;
        }
        let (anchor, shown, before) = (self.anchor(), self.shown_mode(), self.wrap_cols());
        self.geometry = geometry;
        if self.shown_mode() != shown {
            self.relayout(anchor);
        } else if self.wrap_cols() == before {
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

    /// Scrolls so the row that shows `line` of `side` sits under the pinned header of height `inset`. A line the
    /// pane does not show leaves the file's header at the top instead.
    pub fn reveal_line(&mut self, path: &str, side: Side, line: u32, inset: f32) -> bool {
        let Some(file) = self.file_index(path) else {
            return false;
        };
        let Some(range) = self.rows.file_range(file) else {
            return false;
        };
        let diff = &self.files[file];
        let number = |hunk: u32, index: u32| {
            let line = &diff.hunks[hunk as usize].lines[index as usize];
            match side {
                Side::Old => line.old,
                Side::New => line.new,
            }
        };
        let row = range.into_iter().find(|&index| match self.rows.rows[index].kind {
            RowKind::Line { hunk, line: at } => number(hunk, at) == Some(line),
            RowKind::Pair { hunk, left, right } => [left, right]
                .into_iter()
                .flatten()
                .any(|at| number(hunk, at) == Some(line)),
            _ => false,
        });
        match row {
            Some(row) => {
                self.scroll_to(self.tops.top(row) - inset);
                true
            }
            None => self.reveal(path),
        }
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
            row: self.rows.row_ref(index, &self.blocks),
            in_row: self.scroll - self.tops.top(index),
            from_header: (self.scroll - header).max(0.0),
            path,
        })
    }

    fn anchored_scroll(&self, anchor: &Anchor) -> Option<f32> {
        let file = self.file_index(&anchor.path);
        let found = match (anchor.row, file) {
            (RowRef::Block(_), _) => self.rows.find(0, anchor.row, &self.blocks),
            (row, Some(file)) => self.rows.find(file, row, &self.blocks),
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
        self.refresh_previews();
        self.rows = build(
            &self.files,
            Visibility {
                collapsed: &self.collapsed,
                loaded: &self.loaded,
                previews: &self.previews,
            },
            &self.blocks,
            self.shown_mode(),
        );
        self.retop(anchor);
    }

    fn refresh_previews(&mut self) {
        self.previews = if self.preview_source {
            self.files
                .iter()
                .filter(|file| preview_kind(&file.path).is_some() && !self.preview_off.contains(&file.path))
                .map(|file| file.path.clone())
                .collect()
        } else {
            HashSet::new()
        };
    }

    fn retop(&mut self, anchor: Option<Anchor>) {
        self.tops = Tops::with_heights(
            &self.rows.rows,
            &self.files,
            self.wrap_cols(),
            &self.block_heights,
            &self.preview_heights,
        );
        self.restore(anchor);
    }

    /// Replaces the block rows. A block that was already placed keeps its height.
    pub fn set_blocks(&mut self, blocks: Vec<BlockSlot>) {
        let anchor = self.anchor();
        let known: HashMap<BlockId, f32> = self
            .blocks
            .iter()
            .map(|block| block.id)
            .zip(self.block_heights.iter().copied())
            .collect();
        self.block_heights = blocks
            .iter()
            .map(|block| known.get(&block.id).copied().unwrap_or(THREAD))
            .collect();
        self.blocks = blocks;
        self.relayout(anchor);
    }

    pub fn block_height(&self, id: BlockId) -> Option<f32> {
        let slot = self.blocks.iter().position(|block| block.id == id)?;
        Some(self.block_heights[slot])
    }

    pub fn block_top(&self, id: BlockId) -> Option<f32> {
        let slot = self.blocks.iter().position(|block| block.id == id)?;
        Some(self.tops.top(self.rows.block_row(slot as u32)?))
    }

    /// Sets the height of a block's row. Rows below it move. A row above the viewport keeps the
    /// content in view where it is, and a row under the viewport top grows downward.
    pub fn set_block_height(&mut self, id: BlockId, height: f32) -> bool {
        let Some(slot) = self.blocks.iter().position(|block| block.id == id) else {
            return false;
        };
        if self.block_heights[slot] == height {
            return false;
        }
        self.block_heights[slot] = height;
        if let Some(row) = self.rows.block_row(slot as u32) {
            let (top, old) = (self.tops.top(row), self.tops.height(row));
            self.tops.set_height(row, height);
            if top + old <= self.scroll {
                self.scroll += height - old;
            }
            self.scroll_to(self.scroll);
        }
        true
    }

    /// Scrolls just enough to show the block under the pinned header of height `inset`.
    pub fn reveal_block(&mut self, id: BlockId, inset: f32) -> bool {
        let Some(slot) = self.blocks.iter().position(|block| block.id == id) else {
            return false;
        };
        let Some(row) = self.rows.block_row(slot as u32) else {
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
    use crate::diff_pane::layout::{FILE_HEADER, GAP, HUNK, LINE, NOTICE, SPLIT_MIN_WIDTH};
    use crate::diff_pane::testing::{added, diff, lines_with};
    use diffpilot_core::threads::{Side, ThreadAnchor, ThreadId};

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

    fn slot(id: u32, anchor: ThreadAnchor) -> BlockSlot {
        BlockSlot {
            id: BlockId::Thread(ThreadId(id)),
            anchor,
        }
    }

    fn line_thread(id: u32, path: &str, line: u32) -> BlockSlot {
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
        state.set_blocks(vec![line_thread(1, "f0.ts", 3)]);
        assert_eq!(state.block_height(BlockId::Thread(ThreadId(1))), Some(THREAD));
        assert_eq!(state.tops.total(), before + THREAD);
        assert!(state.set_block_height(BlockId::Thread(ThreadId(1)), 220.0));
        assert!(!state.set_block_height(BlockId::Thread(ThreadId(1)), 220.0));
        assert_eq!(state.tops.total(), before + 220.0);
        state.set_blocks(vec![line_thread(1, "f0.ts", 3), line_thread(2, "f1.ts", 1)]);
        assert_eq!(state.block_height(BlockId::Thread(ThreadId(1))), Some(220.0));
        assert_eq!(state.block_height(BlockId::Thread(ThreadId(2))), Some(THREAD));
        assert_eq!(state.tops.total(), before + 220.0 + THREAD);
        assert!(!state.set_block_height(BlockId::Thread(ThreadId(9)), 10.0));
    }

    #[test]
    fn a_thread_above_the_viewport_growing_keeps_the_content_in_view() {
        let mut state = state(three_files());
        state.set_blocks(vec![line_thread(1, "f0.ts", 3)]);
        state.scroll_to(FILE_HEIGHT + 3.0 * LINE);
        let before = content_at_top(&state);
        let at = state.scroll();
        state.set_block_height(BlockId::Thread(ThreadId(1)), 400.0);
        assert_eq!(state.scroll(), at + (400.0 - THREAD));
        assert_eq!(content_at_top(&state), (before.0, before.1));
        state.set_block_height(BlockId::Thread(ThreadId(1)), 100.0);
        assert_eq!(state.scroll(), at - (THREAD - 100.0));
        assert_eq!(content_at_top(&state), (before.0, before.1));
    }

    #[test]
    fn a_thread_under_the_viewport_top_grows_downward_and_one_below_moves_nothing() {
        let mut state = state(three_files());
        state.set_blocks(vec![line_thread(1, "f0.ts", 3), line_thread(2, "f2.ts", 3)]);
        let row = state.rows.block_row(0).unwrap();
        let top = state.tops.top(row);
        state.scroll_to(top + 30.0);
        state.set_block_height(BlockId::Thread(ThreadId(1)), 400.0);
        assert_eq!(state.scroll(), top + 30.0);
        state.set_block_height(BlockId::Thread(ThreadId(2)), 400.0);
        assert_eq!(state.scroll(), top + 30.0);
    }

    #[test]
    fn adding_and_removing_a_thread_above_the_viewport_keeps_the_content_in_view() {
        let mut state = state(three_files());
        state.scroll_to(FILE_HEIGHT + 4.0 * LINE);
        let before = content_at_top(&state);
        let before_row = state.rows.row_ref(before.0, &state.blocks);
        state.set_blocks(vec![line_thread(1, "f0.ts", 3)]);
        let after = content_at_top(&state);
        assert_eq!(state.rows.row_ref(after.0, &state.blocks), before_row);
        assert_eq!(after.1, before.1);
        assert_eq!(state.scroll(), FILE_HEIGHT + 4.0 * LINE + THREAD);
        state.set_blocks(Vec::new());
        assert_eq!(state.scroll(), FILE_HEIGHT + 4.0 * LINE);
    }

    #[test]
    fn a_change_thread_sits_above_the_first_file() {
        let mut state = state(three_files());
        let full = state.tops.total();
        state.set_blocks(vec![slot(1, ThreadAnchor::Change)]);
        assert_eq!(state.tops.total(), full + THREAD);
        assert_eq!(state.visible_path(), Some("f0.ts"));
        assert_eq!(state.block_top(BlockId::Thread(ThreadId(1))), Some(0.0));
        state.scroll_to(30.0);
        state.toggle_collapsed("f1.ts");
        assert_eq!(state.scroll(), 30.0);
        state.set_block_height(BlockId::Thread(ThreadId(1)), 300.0);
        assert_eq!(state.scroll(), 30.0);
        assert_eq!(state.visible_path(), Some("f0.ts"));
    }

    #[test]
    fn revealing_a_thread_scrolls_only_when_it_is_not_fully_shown() {
        let mut state = state(three_files());
        state.set_blocks(vec![line_thread(1, "f2.ts", 9)]);
        let top = state.block_top(BlockId::Thread(ThreadId(1))).unwrap();
        assert!(state.reveal_block(BlockId::Thread(ThreadId(1)), 66.0));
        assert_eq!(state.scroll(), (top - 66.0).min(state.max_scroll()));
        let shown = state.scroll();
        assert!(state.reveal_block(BlockId::Thread(ThreadId(1)), 66.0));
        assert_eq!(state.scroll(), shown);
        assert!(!state.reveal_block(BlockId::Thread(ThreadId(9)), 66.0));
    }

    #[test]
    fn collapsing_the_file_that_holds_the_top_thread_scrolls_to_its_header() {
        let mut state = state(three_files());
        state.set_blocks(vec![line_thread(1, "f1.ts", 5)]);
        let top = state.block_top(BlockId::Thread(ThreadId(1))).unwrap();
        state.scroll_to(top + 10.0);
        assert_eq!(state.visible_path(), Some("f1.ts"));
        state.toggle_collapsed("f1.ts");
        let header = state.tops.top(state.rows.header_index(1).unwrap());
        assert_eq!(state.scroll(), header);
    }

    const WIDE: Geometry = Geometry {
        width: 1000.0,
        height: 100.0,
        cell: 7.0,
    };

    fn changes(path: &str, spec: &str, text: &str) -> FileDiff {
        diff(path, lines_with(spec, text))
    }

    fn split_state(files: Vec<FileDiff>) -> PaneState {
        let mut state = PaneState::new(WIDE);
        state.replace_files(files);
        state.set_mode(DiffMode::Split);
        state
    }

    fn top_shows(state: &PaneState, hunk: u32, line: u32) -> bool {
        let (row, _) = content_at_top(state);
        state.rows.rows[row].shows_line(hunk, line)
    }

    fn two_changed_files() -> Vec<FileDiff> {
        ["a.ts", "b.ts", "c.ts"]
            .iter()
            .map(|path| changes(path, "cdddddaaaaaccc", "x"))
            .collect()
    }

    #[test]
    fn split_rows_replace_the_line_rows_and_unified_brings_them_back() {
        let mut state = PaneState::new(WIDE);
        state.replace_files(vec![changes("a.ts", "cddaa", "x")]);
        let unified = state.tops.total();
        assert_eq!(state.rows.rows.len(), 1 + 1 + 5 + 1);
        state.set_mode(DiffMode::Split);
        assert_eq!(state.mode(), DiffMode::Split);
        assert_eq!(state.rows.rows.len(), 1 + 1 + 3 + 1);
        assert_eq!(state.tops.total(), unified - 2.0 * LINE);
        state.set_mode(DiffMode::Unified);
        assert_eq!(state.rows.rows.len(), 8);
        assert_eq!(state.tops.total(), unified);
    }

    #[test]
    fn a_narrow_pane_keeps_unified_rows_until_it_is_wide_enough() {
        let mut state = state(vec![changes("a.ts", "cddaa", "x")]);
        state.set_mode(DiffMode::Split);
        assert_eq!((state.mode(), state.shown_mode()), (DiffMode::Split, DiffMode::Unified));
        assert_eq!(state.rows.rows.len(), 8);
        assert_eq!(state.split_width(), None);
        state.set_geometry(Geometry {
            width: SPLIT_MIN_WIDTH,
            ..GEOMETRY
        });
        assert_eq!(state.shown_mode(), DiffMode::Split);
        assert_eq!(state.rows.rows.len(), 6);
        assert_eq!(state.split_width(), Some(SPLIT_MIN_WIDTH));
        state.set_geometry(Geometry {
            width: SPLIT_MIN_WIDTH - 1.0,
            ..GEOMETRY
        });
        assert_eq!(state.shown_mode(), DiffMode::Unified);
        assert_eq!(state.rows.rows.len(), 8);
    }

    #[test]
    fn the_shown_mode_is_reported_once_per_change() {
        let mut state = state(vec![changes("a.ts", "cddaa", "x")]);
        assert_eq!(state.take_shown_change(), None);
        state.set_mode(DiffMode::Split);
        assert_eq!(state.take_shown_change(), None, "too narrow to show it");
        state.set_geometry(WIDE);
        assert_eq!(state.take_shown_change(), Some(DiffMode::Split));
        assert_eq!(state.take_shown_change(), None);
        state.set_mode(DiffMode::Unified);
        assert_eq!(state.take_shown_change(), Some(DiffMode::Unified));
    }

    #[test]
    fn toggling_keeps_the_line_at_the_top_and_how_far_into_it_the_view_is() {
        let mut state = PaneState::new(WIDE);
        state.replace_files(two_changed_files());
        let third_added = state.rows.find(0, RowRef::Line(0, 8), &state.blocks).unwrap();
        state.scroll_to(state.tops.top(third_added) + 5.0);
        assert!(top_shows(&state, 0, 8));
        state.set_mode(DiffMode::Split);
        assert!(top_shows(&state, 0, 8), "the added line is on the right of its row");
        assert_eq!(content_at_top(&state).1, 5.0);
        assert_eq!(state.visible_path(), Some("a.ts"));
        state.set_mode(DiffMode::Unified);
        assert!(
            top_shows(&state, 0, 3),
            "its row also holds the deleted line that pairs with it"
        );
    }

    #[test]
    fn toggling_twice_returns_to_a_context_line_exactly() {
        let mut state = PaneState::new(WIDE);
        state.replace_files(two_changed_files());
        let context = state.rows.find(1, RowRef::Line(0, 12), &state.blocks).unwrap();
        state.scroll_to(state.tops.top(context) + 7.0);
        let before = state.scroll();
        state.set_mode(DiffMode::Split);
        assert!(top_shows(&state, 0, 12));
        assert_eq!(state.visible_path(), Some("b.ts"));
        assert_ne!(state.scroll(), before, "the file above got shorter");
        state.set_mode(DiffMode::Unified);
        assert_eq!(state.scroll(), before);
    }

    #[test]
    fn toggling_keeps_a_header_and_a_thread_where_they_are() {
        let mut state = PaneState::new(WIDE);
        state.replace_files(two_changed_files());
        state.set_blocks(vec![line_thread(1, "a.ts", 4)]);
        state.reveal("b.ts");
        let header = state.rows.header_index(1).unwrap();
        state.set_mode(DiffMode::Split);
        assert_eq!(state.scroll(), state.tops.top(state.rows.header_index(1).unwrap()));
        assert_eq!(state.visible_path(), Some("b.ts"));
        assert_ne!(header, state.rows.header_index(1).unwrap());

        let top = state.block_top(BlockId::Thread(ThreadId(1))).unwrap();
        state.scroll_to(top + 30.0);
        state.set_mode(DiffMode::Unified);
        let (row, offset) = content_at_top(&state);
        assert_eq!(
            state.rows.row_ref(row, &state.blocks),
            RowRef::Block(BlockId::Thread(ThreadId(1)))
        );
        assert_eq!(offset, 30.0);
    }

    #[test]
    fn a_line_thread_follows_its_line_into_the_split_rows() {
        let mut state = PaneState::new(WIDE);
        state.replace_files(vec![changes("a.ts", "cddaa", "x")]);
        state.set_blocks(vec![
            slot(1, ThreadAnchor::line("a.ts", Side::New, 3)),
            slot(2, ThreadAnchor::line("a.ts", Side::Old, 2)),
        ]);
        let body = FILE_HEADER + HUNK;
        assert_eq!(
            state.block_top(BlockId::Thread(ThreadId(1))),
            Some(body + 5.0 * LINE + THREAD)
        );
        assert_eq!(state.block_top(BlockId::Thread(ThreadId(2))), Some(body + 2.0 * LINE));
        state.set_mode(DiffMode::Split);
        assert_eq!(
            state.block_top(BlockId::Thread(ThreadId(1))),
            Some(body + 3.0 * LINE + THREAD)
        );
        assert_eq!(state.block_top(BlockId::Thread(ThreadId(2))), Some(body + 2.0 * LINE));
    }

    #[test]
    fn wrapped_split_rows_use_the_width_of_one_column() {
        let long = "x".repeat(100);
        let mut state = PaneState::new(WIDE);
        state.replace_files(vec![changes("a.ts", "da", &long)]);
        assert_eq!(state.tops.height(2), LINE, "125 columns hold it in unified rows");
        state.set_mode(DiffMode::Split);
        assert_eq!(state.wrap_cols(), Some(63));
        assert_eq!(state.tops.height(2), 2.0 * LINE);
        state.set_wrap(false);
        assert_eq!(state.tops.height(2), LINE);
        state.set_wrap(true);
        assert_eq!(state.tops.height(2), 2.0 * LINE);
    }

    #[test]
    fn split_columns_scroll_sideways_to_the_longest_line() {
        let mut state = PaneState::new(WIDE);
        state.replace_files(vec![changes("a.ts", "da", &"x".repeat(200))]);
        state.set_wrap(false);
        let unified = state.max_scroll_x();
        assert_eq!(unified, 200.0 * 7.0 + CODE_PADDING - (1000.0 - GUTTER + CODE_PADDING));
        state.set_mode(DiffMode::Split);
        let column = crate::diff_pane::layout::column_width(1000.0);
        let split = 200.0 * 7.0 + CODE_PADDING - (column - SPLIT_GUTTER + CODE_PADDING);
        assert_eq!(state.max_scroll_x(), split);
        state.scroll_x_by(1e9);
        assert_eq!(state.scroll_x(), split);
        state.set_mode(DiffMode::Unified);
        assert_eq!(state.scroll_x(), unified, "a smaller limit pulls the offset back");
    }

    #[test]
    fn the_pinned_header_and_hunk_follow_split_rows() {
        let mut state = split_state(two_changed_files());
        state.scroll_to(state.tops.top(3) + 5.0);
        let sticky = state.position().and_then(|position| position.sticky).unwrap();
        assert_eq!((sticky.file, sticky.hunk), (0, Some(0)));
        assert_eq!(state.visible_path(), Some("a.ts"));
    }

    #[test]
    fn reveal_collapse_and_viewed_work_on_split_rows() {
        let mut binary = FileDiff::new("logo.png", diffpilot_core::changes::FileStatus::Added);
        binary.is_binary = true;
        let files = vec![
            changes("a.ts", "cdddddaaaaaccc", "x"),
            changes("b.ts", "cdddddaaaaaccc", "x"),
            binary,
            changes("c.ts", "aaa", "x"),
        ];
        let mut state = split_state(files);
        assert!(state.reveal("c.ts"));
        assert_eq!(state.visible_path(), Some("c.ts"));
        state.scroll_to(0.0);
        state.toggle_collapsed("a.ts");
        assert_eq!(state.rows.file_range(0).map(|range| range.len()), Some(2));
        state.set_viewed(["b.ts".to_string()].into());
        assert!(state.collapsed.contains("b.ts"));
        assert_eq!(state.rows.file_range(1).map(|range| range.len()), Some(2));
        assert_eq!(state.files_to_highlight(), [3]);
        let binary_rows = state.rows.file_range(2).unwrap();
        assert!(matches!(
            state.rows.rows[binary_rows.start + 1].kind,
            crate::diff_pane::rows::RowKind::Notice(_)
        ));
    }

    #[test]
    fn a_filter_shows_only_its_files_in_its_order_and_clearing_it_restores_the_diff() {
        let mut state = state(three_files());
        let reused = state.set_filter(Some(vec!["f2.ts".into(), "f0.ts".into()]));
        assert_eq!(paths(&state), ["f2.ts", "f0.ts"]);
        assert_eq!(reused, [Some(2), Some(0)]);
        assert_eq!(state.all_files().len(), 3);
        assert!(!state.reveal("f1.ts"));
        state.set_filter(None);
        assert_eq!(paths(&state), ["f0.ts", "f1.ts", "f2.ts"]);
    }

    #[test]
    fn new_files_keep_the_filter() {
        let mut state = state(three_files());
        state.set_filter(Some(vec!["f1.ts".into()]));
        state.replace_files(three_files());
        assert_eq!(paths(&state), ["f1.ts"]);
    }

    #[test]
    fn a_note_row_sits_under_its_line_and_keeps_its_height() {
        let mut state = state(three_files());
        let note = |index| BlockId::Note(index);
        state.set_blocks(vec![BlockSlot {
            id: note(0),
            anchor: ThreadAnchor::line("f1.ts", Side::New, 4),
        }]);
        assert_eq!(state.block_height(note(0)), Some(THREAD));
        assert!(state.set_block_height(note(0), 60.0));
        let row = state.rows.block_row(0).unwrap();
        assert_eq!(state.rows.rows[row - 1].file, 1);
        assert_eq!(state.tops.height(row), 60.0);
    }

    #[test]
    fn reveal_line_puts_the_line_under_the_pinned_header() {
        let mut state = state(three_files());
        let inset = FILE_HEADER + HUNK;
        assert!(state.reveal_line("f1.ts", Side::New, 5, inset));
        let line = state.rows.find(1, RowRef::Line(0, 4), &state.blocks).unwrap();
        assert_eq!(state.scroll(), state.tops.top(line) - inset);
        assert!(
            state.reveal_line("f2.ts", Side::New, 99, inset),
            "a missing line shows the file header"
        );
        assert_eq!(state.visible_path(), Some("f2.ts"));
        assert!(!state.reveal_line("none.ts", Side::New, 1, inset));
    }

    fn with_preview_source(files: Vec<FileDiff>) -> PaneState {
        let mut state = state(files);
        state.set_preview_source(true);
        state
    }

    #[test]
    fn previews_need_a_source_and_can_be_switched_off_per_file() {
        let files = || vec![diff("a.md", vec![added(1, "x")]), diff("b.ts", vec![added(1, "x")])];
        let state = state(files());
        assert!(!state.previewable("a.md"));
        assert!(!state.shows_preview("a.md"));

        let mut state = with_preview_source(files());
        assert!(state.shows_preview("a.md"));
        assert!(!state.shows_preview("b.ts"), "only images and Markdown preview");
        assert_eq!(state.tops.height(1), crate::diff_pane::layout::PREVIEW);

        state.toggle_preview("a.md");
        assert!(!state.shows_preview("a.md"));
        assert_eq!(state.rows.rows[1].kind, RowKind::Hunk { hunk: 0 });

        state.replace_files(files());
        assert!(!state.shows_preview("a.md"), "new files keep the choice");
        state.toggle_preview("a.md");
        assert!(state.shows_preview("a.md"));
    }

    #[test]
    fn a_measured_preview_sets_its_row_height_and_moves_the_rows_below() {
        let mut state = with_preview_source(vec![
            diff("a.md", vec![added(1, "x")]),
            diff("b.ts", vec![added(1, "x")]),
        ]);
        let below = state.tops.top(3);
        assert!(state.set_preview_height("a.md", 301.0));
        assert_eq!(state.tops.height(1), 301.0);
        assert_eq!(state.tops.top(3), below + 301.0 - crate::diff_pane::layout::PREVIEW);
        assert!(!state.set_preview_height("a.md", 301.0));
        state.toggle_preview("a.md");
        state.toggle_preview("a.md");
        assert_eq!(state.tops.height(1), 301.0, "the height survives a switch");
    }
}
