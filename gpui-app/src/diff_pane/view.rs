use crate::diff_pane::chrome;
use crate::diff_pane::highlight::{self, FileTokens};
use crate::diff_pane::layout::{
    CODE_PADDING, FILE_HEADER, HUD, HUNK, LINE, NOTICE, NUMBER_COLUMN, RIBBON, SPLIT_DIVIDER, Sticky, ThreadFrame,
    column_width,
};
use crate::diff_pane::lines::{
    self, CODE_SIZE, Column, Fonts, Frame, LineCache, LineKey, LineRequest, PaintFiller, Style, Thumb,
};
use crate::diff_pane::preview::{self, ImageMode, PreviewCard, PreviewEvent};
use crate::diff_pane::rows::{BlockId, BlockSlot, Notice, RowKind};
use crate::diff_pane::selection::{Extent, Selection, Spot, column_line, word_at};
use crate::diff_pane::stage_lines;
use crate::diff_pane::state::{Geometry, PaneState, ThreadSteps, ThumbDrag};
use crate::guide::note::{NoteCard, NoteEvent};
use crate::guide::text::CodeRef;
use crate::review::{CardEvent, ReviewEvent, ReviewSession, ThreadCard, ThreadChange, card_key};
use diffpilot_core::changes::FileStatus;
use diffpilot_core::comparison::Comparison;
use diffpilot_core::diff::{DiffMode, FileDiff, LineKind};
use diffpilot_core::file_kind::file_kind;
use diffpilot_core::stage::Action;
use diffpilot_core::threads::{Side, ThreadAnchor, ThreadId};
use diffpilot_ui_kit::Glyph;
use diffpilot_ui_kit::Theme;
use diffpilot_ui_kit::components::menu::{MenuAt, MenuItem, MenuSection, menu_at, menu_button};
use diffpilot_ui_kit::components::{
    BadgeTone, Button, ChipTone, Look, State, bar_button, chip_label, glass_panel, glyph_icon, label_switch,
    status_badge, tag, viewed_toggle,
};
use diffpilot_ui_kit::regions::region;
use gpui::{
    AnyView, App, AppContext as _, ClickEvent, Context, DispatchPhase, Div, Entity, EventEmitter, FontWeight, Hsla,
    InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    ParentElement as _, Pixels, Render, ScrollWheelEvent, SharedString, Stateful, StatefulInteractiveElement as _,
    StyleRefinement, Styled as _, Subscription, Window, canvas, div, font, px,
};
use gpui::{ClipboardItem, FocusHandle, Point, actions};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

const MAX_RUNNING_HIGHLIGHTS: usize = 3;
const KEY_CONTEXT: &str = "DiffPane";
const SCROLL_LANE: f32 = 14.0;

actions!(diff_pane, [CopySelection, ClearSelection]);

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        gpui::KeyBinding::new("cmd-c", CopySelection, Some(KEY_CONTEXT)),
        gpui::KeyBinding::new("escape", ClearSelection, Some(KEY_CONTEXT)),
    ]);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiffPaneEvent {
    VisibleFile(String),
    ToggleViewed(String),
    FirstPaint,
    OpenInEditor {
        path: String,
        line: Option<u32>,
    },
    /// The mode the rows are drawn in changed. A split pane that is too narrow draws unified rows.
    ShownMode(DiffMode),
    ThreadSteps,
    OpenRef(CodeRef),
    RevealInFinder(String),
    /// A stage, unstage or discard changed the repository, so the diff is stale.
    Changed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaneNote {
    pub path: String,
    pub side: Side,
    pub line: Option<u32>,
    pub text: String,
    pub critical: bool,
}

struct Note {
    view: Entity<NoteCard>,
    anchor: ThreadAnchor,
    _opened: Subscription,
}

struct LineMenu {
    at: Point<Pixels>,
    path: String,
    line: Option<u32>,
}

#[derive(Clone)]
enum Highlight {
    Missing,
    Running,
    Plain,
    Ready(Arc<FileTokens>),
}

enum FirstPaint {
    Waiting,
    Scheduled,
    Sent,
}

#[derive(Default)]
pub struct Probe {
    pub enabled: bool,
    pub frames_ms: Vec<f32>,
    pub lines_ms: Vec<f32>,
    started: Option<Instant>,
}

impl Probe {
    fn begin(&mut self) {
        if self.enabled {
            self.started = Some(Instant::now());
        }
    }

    fn end(&mut self) {
        if let Some(started) = self.started.take() {
            self.frames_ms.push(started.elapsed().as_secs_f32() * 1000.0);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Hovered {
    row: usize,
    file: usize,
    column: Column,
    hunk: u32,
    line: u32,
}

struct Card {
    view: Entity<ThreadCard>,
    _resized: Subscription,
}

#[derive(Clone, PartialEq)]
struct PreviewSource {
    repo: PathBuf,
    comparison: Comparison,
}

struct Preview {
    file: Arc<FileDiff>,
    card: Option<Entity<PreviewCard>>,
    measured: Option<(f32, ImageMode)>,
    _resized: Option<Subscription>,
}

struct Review {
    session: Entity<ReviewSession>,
    cards: HashMap<ThreadId, Card>,
    _session: Subscription,
}

pub struct DiffPane {
    state: PaneState,
    review: Option<Review>,
    notes: Vec<Note>,
    note_data: Vec<PaneNote>,
    pending_focus: Option<ThreadId>,
    mouse: Option<(Pixels, Pixels)>,
    origin: (Pixels, Pixels),
    remeasure: bool,
    fonts: Fonts,
    theme: Theme,
    highlights: Vec<Highlight>,
    parked: HashMap<String, (Arc<FileDiff>, Highlight)>,
    running: usize,
    lines: LineCache,
    first_paint: FirstPaint,
    uncommitted: HashSet<String>,
    last_scroll: Option<Instant>,
    lane_hovered: bool,
    thumb_drag: Option<ThumbDrag>,
    line_menu: Option<LineMenu>,
    selection: Option<Selection>,
    confirming: bool,
    stage_error: Option<String>,
    focus: FocusHandle,
    preview_source: Option<PreviewSource>,
    previews: HashMap<String, Preview>,
    pub probe: Probe,
}

impl EventEmitter<DiffPaneEvent> for DiffPane {}

fn load_fonts(window: &Window) -> Fonts {
    let system = window.text_system();
    let advance = |mono: &gpui::Font, ch: char| {
        system
            .advance(system.resolve_font(mono), px(CODE_SIZE), ch)
            .map_or(0.0, |size| f32::from(size.width))
    };
    let mono = [font(".AppleSystemUIFontMonospaced"), font("Menlo")]
        .into_iter()
        .find(|mono| advance(mono, 'i') > 0.0 && advance(mono, 'i') == advance(mono, 'W'))
        .unwrap_or_else(|| font("Menlo"));
    let cell = match advance(&mono, '0') {
        0.0 => 7.2,
        width => width,
    };
    Fonts {
        ui: font(".SystemUIFont"),
        mono,
        cell,
    }
}

impl DiffPane {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let fonts = load_fonts(window);
        let viewport = window.viewport_size();
        let geometry = Geometry {
            width: f32::from(viewport.width),
            height: f32::from(viewport.height),
            cell: fonts.cell,
        };
        cx.background_spawn(async {
            highlight::engine();
        })
        .detach();
        cx.observe_global::<Theme>(|_, cx| cx.notify()).detach();
        Self {
            state: PaneState::new(geometry),
            review: None,
            notes: Vec::new(),
            note_data: Vec::new(),
            pending_focus: None,
            mouse: None,
            origin: (px(0.0), px(0.0)),
            remeasure: false,
            fonts,
            theme: Theme::new(false),
            highlights: Vec::new(),
            parked: HashMap::new(),
            running: 0,
            lines: LineCache::default(),
            first_paint: FirstPaint::Waiting,
            uncommitted: HashSet::new(),
            last_scroll: None,
            lane_hovered: false,
            thumb_drag: None,
            line_menu: None,
            selection: None,
            confirming: false,
            stage_error: None,
            focus: cx.focus_handle(),
            preview_source: None,
            previews: HashMap::new(),
            probe: Probe::default(),
        }
    }

    pub fn set_files(&mut self, files: Vec<FileDiff>, cx: &mut Context<Self>) {
        let before = self.state.files.clone();
        let reused = self.state.replace_files(files);
        self.carry_highlights(before, &reused);
        self.lines.clear();
        self.share_files(cx);
        self.sync(cx);
    }

    /// Without a source every file shows its diff.
    pub fn set_preview_source(&mut self, source: Option<(PathBuf, Comparison)>, cx: &mut Context<Self>) {
        let source = source.map(|(repo, comparison)| PreviewSource { repo, comparison });
        if source == self.preview_source {
            return;
        }
        self.state.set_preview_source(source.is_some());
        self.preview_source = source;
        self.previews.clear();
        self.lines.clear();
        self.sync(cx);
    }

    pub fn set_preview(&mut self, path: &str, on: bool, cx: &mut Context<Self>) {
        if self.state.shows_preview(path) != on {
            self.state.toggle_preview(path);
            self.lines.clear();
            self.sync(cx);
        }
    }

    pub fn set_filter(&mut self, filter: Option<Vec<String>>, cx: &mut Context<Self>) {
        if self.state.filter() == filter.as_deref() {
            return;
        }
        let before = self.state.files.clone();
        let reused = self.state.set_filter(filter);
        self.carry_highlights(before, &reused);
        self.lines.clear();
        self.state.scroll_to(0.0);
        self.remeasure = true;
        self.sync(cx);
    }

    fn carry_highlights(&mut self, before: Vec<Arc<FileDiff>>, reused: &[Option<usize>]) {
        self.selection = self.selection.and_then(|selection| {
            let file = reused.iter().position(|from| *from == Some(selection.file))?;
            Some(Selection { file, ..selection })
        });
        let old = std::mem::take(&mut self.highlights);
        let mut kept = vec![false; old.len()];
        for index in reused.iter().flatten() {
            kept[*index] = true;
        }
        for (index, file) in before.into_iter().enumerate() {
            if let (false, Some(highlight @ (Highlight::Plain | Highlight::Ready(_)))) = (kept[index], old.get(index)) {
                self.parked.insert(file.path.clone(), (file, highlight.clone()));
            }
        }
        self.highlights = self
            .state
            .files
            .iter()
            .zip(reused)
            .map(|(file, from)| match from {
                Some(index) => old[*index].clone(),
                None => match self.parked.remove(&file.path) {
                    Some((parked, highlight)) if Arc::ptr_eq(&parked, file) => highlight,
                    _ => Highlight::Missing,
                },
            })
            .collect();
        let all = self.state.all_files();
        self.parked.retain(|path, (file, _)| {
            all.iter()
                .any(|current| current.path == *path && Arc::ptr_eq(current, file))
        });
    }

    pub fn set_notes(&mut self, notes: Vec<PaneNote>, window: &mut Window, cx: &mut Context<Self>) {
        if notes == self.note_data {
            return;
        }
        self.notes = notes
            .iter()
            .map(|note| {
                let view = cx.new(|_| NoteCard::new(&note.text, note.critical));
                let opened = cx.subscribe_in(&view, window, |_, _, NoteEvent::Open(code): &NoteEvent, _, cx| {
                    cx.emit(DiffPaneEvent::OpenRef(code.clone()));
                });
                let anchor = match note.line {
                    Some(line) => ThreadAnchor::line(&note.path, note.side, line),
                    None => ThreadAnchor::file(&note.path),
                };
                Note {
                    view,
                    anchor,
                    _opened: opened,
                }
            })
            .collect();
        self.note_data = notes;
        self.rebuild_blocks(window, cx);
    }

    pub fn reveal_line(&mut self, path: &str, side: Side, line: u32, cx: &mut Context<Self>) {
        if self.state.reveal_line(path, side, line, FILE_HEADER + HUNK) {
            self.sync(cx);
        }
    }

    pub fn set_uncommitted(&mut self, paths: HashSet<String>, cx: &mut Context<Self>) {
        if self.uncommitted != paths {
            self.uncommitted = paths;
            cx.notify();
        }
    }

    pub fn set_viewed(&mut self, viewed: HashSet<String>, cx: &mut Context<Self>) {
        self.state.set_viewed(viewed);
        self.sync(cx);
    }

    pub fn reveal(&mut self, path: &str, cx: &mut Context<Self>) {
        if self.state.reveal(path) {
            self.sync(cx);
        }
    }

    pub fn set_wrap(&mut self, wrap: bool, cx: &mut Context<Self>) {
        self.state.set_wrap(wrap);
        self.sync(cx);
    }

    pub fn set_mode(&mut self, mode: DiffMode, cx: &mut Context<Self>) {
        self.selection = None;
        self.state.set_mode(mode);
        self.remeasure = true;
        self.sync(cx);
    }

    pub fn scroll_by(&mut self, dy: f32, dx: f32, cx: &mut Context<Self>) {
        self.state.scroll_by(dy);
        self.state.scroll_x_by(dx);
        self.last_scroll = Some(Instant::now());
        self.sync(cx);
    }

    pub fn step_thread(&mut self, forward: bool, cx: &mut Context<Self>) {
        if self.state.step_thread(forward, FILE_HEADER + HUNK) {
            self.last_scroll = Some(Instant::now());
            self.sync(cx);
        }
    }

    pub fn thread_steps(&self) -> ThreadSteps {
        self.state.thread_steps(FILE_HEADER + HUNK)
    }

    pub fn scroll_to(&mut self, y: f32, cx: &mut Context<Self>) {
        self.state.scroll_to(y);
        self.last_scroll = Some(Instant::now());
        self.sync(cx);
    }

    pub fn state(&self) -> &PaneState {
        &self.state
    }

    /// Shows the session's threads in the pane and keeps it told which files are open and which one is on top.
    pub fn set_review(&mut self, session: Entity<ReviewSession>, window: &mut Window, cx: &mut Context<Self>) {
        let subscription = cx.subscribe_in(&session, window, |this, _, event: &ReviewEvent, window, cx| {
            if let ReviewEvent::ThreadsChanged(change) = event {
                this.threads_changed(*change, window, cx);
            }
        });
        self.review = Some(Review {
            session,
            cards: HashMap::new(),
            _session: subscription,
        });
        self.share_files(cx);
        self.share_visible(self.state.visible_path().map(str::to_owned), cx);
        self.rebuild_blocks(window, cx);
    }

    pub fn review(&self) -> Option<&Entity<ReviewSession>> {
        Some(&self.review.as_ref()?.session)
    }

    pub fn card(&self, id: ThreadId) -> Option<&Entity<ThreadCard>> {
        Some(&self.review.as_ref()?.cards.get(&id)?.view)
    }

    /// Scrolls the thread into view and puts the cursor in its composer.
    pub fn focus_thread(&mut self, id: ThreadId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(card) = self.card(id).cloned() else {
            self.pending_focus = Some(id);
            return;
        };
        card.update(cx, |card, cx| card.focus_composer(window, cx));
        self.measure(BlockId::Thread(id), window, cx);
        self.state.reveal_block(BlockId::Thread(id), FILE_HEADER + HUNK);
        self.sync(cx);
    }

    fn share_files(&mut self, cx: &mut Context<Self>) {
        let Some(review) = &self.review else { return };
        let files: Arc<[Arc<FileDiff>]> = self.state.all_files().into();
        review.session.update(cx, |session, cx| session.set_files(files, cx));
        if self.state.all_files().is_empty() {
            self.share_visible(None, cx);
        }
    }

    fn share_visible(&mut self, path: Option<String>, cx: &mut Context<Self>) {
        if let Some(review) = &self.review {
            review
                .session
                .update(cx, |session, cx| session.set_visible_path(path, cx));
        }
    }

    fn threads_changed(&mut self, change: ThreadChange, window: &mut Window, cx: &mut Context<Self>) {
        match change {
            ThreadChange::Added(_) | ThreadChange::Removed(_) => self.rebuild_blocks(window, cx),
            ThreadChange::Updated(id) => {
                self.measure(BlockId::Thread(id), window, cx);
                self.sync(cx);
            }
        }
    }

    fn rebuild_blocks(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut slots: Vec<BlockSlot> = Vec::new();
        let mut added = Vec::new();
        if let Some(review) = &self.review {
            let session = review.session.clone();
            let threads: Vec<(ThreadId, ThreadAnchor)> = session
                .read(cx)
                .threads()
                .map(|thread| (thread.id, thread.anchor.clone()))
                .collect();
            slots.extend(threads.iter().map(|(id, anchor)| BlockSlot {
                id: BlockId::Thread(*id),
                anchor: anchor.clone(),
            }));
            let mut cards = std::mem::take(&mut self.review.as_mut().expect("review is set").cards);
            cards.retain(|id, _| threads.iter().any(|(thread, _)| thread == id));
            for (id, _) in &threads {
                let id = *id;
                if cards.contains_key(&id) {
                    continue;
                }
                let view = cx.new(|cx| ThreadCard::new(session.clone(), id, cx));
                let resized = cx.subscribe_in(&view, window, move |this, _, CardEvent::Resized, window, cx| {
                    this.measure(BlockId::Thread(id), window, cx);
                    this.sync(cx);
                });
                cards.insert(
                    id,
                    Card {
                        view,
                        _resized: resized,
                    },
                );
                added.push(BlockId::Thread(id));
            }
            self.review.as_mut().expect("review is set").cards = cards;
        }
        for (index, note) in self.notes.iter().enumerate() {
            slots.push(BlockSlot {
                id: BlockId::Note(index as u32),
                anchor: note.anchor.clone(),
            });
            added.push(BlockId::Note(index as u32));
        }
        self.state.set_blocks(slots);
        for id in added {
            self.measure(id, window, cx);
        }
        if let Some(id) = self.pending_focus
            && self.card(id).is_some()
        {
            self.pending_focus = None;
            self.focus_thread(id, window, cx);
        }
        self.sync(cx);
    }

    fn card_height(&self, id: BlockId, width: Pixels, window: &Window, cx: &App) -> Option<f32> {
        let height = match id {
            BlockId::Thread(id) => self.card(id)?.read(cx).height(width, window, cx),
            BlockId::Note(index) => self.notes.get(index as usize)?.view.read(cx).height(width, window, cx),
        };
        Some(f32::from(height))
    }

    fn measure(&mut self, id: BlockId, window: &Window, cx: &mut Context<Self>) {
        let Some(slot) = self.state.blocks.iter().find(|slot| slot.id == id) else {
            return;
        };
        let frame = ThreadFrame::of(&slot.anchor, self.state.split_width());
        let width = px(frame.card_width(self.state.geometry().width));
        let Some(height) = self.card_height(id, width, window, cx) else {
            return;
        };
        self.state.set_block_height(id, frame.row_height(height));
    }

    fn measure_all(&mut self, window: &Window, cx: &mut Context<Self>) {
        let ids: Vec<BlockId> = self.state.blocks.iter().map(|slot| slot.id).collect();
        for id in ids {
            self.measure(id, window, cx);
        }
    }

    fn hovered(&self) -> Option<Hovered> {
        self.review.as_ref()?;
        let (x, y) = self.mouse?;
        self.line_at(x, y)
    }

    fn line_at(&self, x: Pixels, y: Pixels) -> Option<Hovered> {
        let row = self.state.tops.row_at(self.state.scroll() + f32::from(y))?;
        let file = self.state.rows.rows[row].file as usize;
        match self.state.rows.rows[row].kind {
            RowKind::Line { hunk, line } => Some(Hovered {
                row,
                file,
                column: Column::Unified,
                hunk,
                line,
            }),
            RowKind::Pair { hunk, left, right } => {
                let on_left = f32::from(x) < column_width(self.state.geometry().width);
                let (column, line) = if on_left {
                    (Column::Left, left?)
                } else {
                    (Column::Right, right?)
                };
                Some(Hovered {
                    row,
                    file,
                    column,
                    hunk,
                    line,
                })
            }
            _ => None,
        }
    }

    fn under_sticky(&self, y: Pixels) -> bool {
        let Some(sticky) = self.state.position().and_then(|position| position.sticky) else {
            return false;
        };
        let height = FILE_HEADER + sticky.hunk.map_or(0.0, |_| HUNK);
        let y = f32::from(y);
        y >= sticky.push && y < sticky.push + height
    }

    fn open_line(&self, file: usize, (hunk, line): (u32, u32)) -> Option<(String, Option<u32>)> {
        let file = &self.state.files[file];
        if file.status == FileStatus::Deleted {
            return None;
        }
        let line = file.hunks[hunk as usize].new_line_near(line as usize);
        Some((file.path.clone(), line))
    }

    fn open_menu(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) {
        let (x, y) = (event.position.x - self.origin.0, event.position.y - self.origin.1);
        if self.under_sticky(y) {
            return;
        }
        let target = self
            .line_at(x, y)
            .and_then(|hovered| self.open_line(hovered.file, (hovered.hunk, hovered.line)));
        self.line_menu = target.map(|(path, line)| LineMenu {
            at: event.position,
            path,
            line,
        });
        cx.notify();
    }

    fn open_from_gutter(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) {
        let (x, y) = (event.position.x - self.origin.0, event.position.y - self.origin.1);
        if self.under_sticky(y) {
            return;
        }
        let Some(hovered) = self.line_at(x, y) else { return };
        let numbers = if hovered.column == Column::Unified { 2.0 } else { 1.0 };
        let left = self.column_span(hovered.column).0;
        if f32::from(x) - left >= NUMBER_COLUMN * numbers {
            return;
        }
        if let Some((path, line)) = self.open_line(hovered.file, (hovered.hunk, hovered.line)) {
            cx.emit(DiffPaneEvent::OpenInEditor { path, line });
        }
    }

    fn code_left(&self, column: Column) -> f32 {
        let numbers = if column == Column::Unified { 2.0 } else { 1.0 };
        self.column_span(column).0 + NUMBER_COLUMN * numbers + RIBBON + CODE_PADDING
    }

    fn line_key(&self, file: usize, column: Column, (hunk, line): (u32, u32)) -> LineKey {
        let kind = self.state.files[file].hunks[hunk as usize].lines[line as usize].kind;
        let tokens = tokens_of(self.highlights.get(file));
        LineKey {
            file: file as u32,
            hunk,
            line,
            side: side_of(tokens, column, kind, hunk, line),
        }
    }

    fn spot_at(&self, file: usize, column: Column, at: (u32, u32), row: usize, x: Pixels, y: Pixels) -> Spot {
        let text = &self.state.files[file].hunks[at.0 as usize].lines[at.1 as usize].text;
        let within = (self.state.scroll() + f32::from(y) - self.state.tops.top(row)).max(0.0);
        let x = x - px(self.code_left(column) - self.state.scroll_x());
        let key = self.line_key(file, column, at);
        let byte = self.lines.byte_at(key, text, (within / LINE) as usize, x).unwrap_or(0);
        Spot::at(at.0, at.1, byte)
    }

    fn start_selection(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let (x, y) = (event.position.x - self.origin.0, event.position.y - self.origin.1);
        let cleared = self.selection.take().is_some();
        self.confirming = false;
        self.stage_error = None;
        self.selection = self.selection_at(event.click_count, x, y);
        if self.selection.is_some() {
            window.focus(&self.focus);
        }
        if cleared || self.selection.is_some() {
            cx.notify();
        }
    }

    fn selection_at(&self, click_count: usize, x: Pixels, y: Pixels) -> Option<Selection> {
        if self.under_sticky(y) {
            let sticky = self.state.position()?.sticky?;
            let hunk = sticky.hunk.filter(|_| f32::from(y) >= sticky.push + FILE_HEADER)?;
            return Some(Selection::hunk(sticky.file as usize, hunk));
        }
        let row = self.state.tops.row_at(self.state.scroll() + f32::from(y))?;
        if let RowKind::Hunk { hunk } = self.state.rows.rows[row].kind {
            return Some(Selection::hunk(self.state.rows.rows[row].file as usize, hunk));
        }
        let hovered = self.line_at(x, y)?;
        let (file, column, at) = (hovered.file, hovered.column, (hovered.hunk, hovered.line));
        let numbers = if column == Column::Unified { 2.0 } else { 1.0 };
        if f32::from(x) - self.column_span(column).0 < NUMBER_COLUMN * numbers + RIBBON {
            return Some(Selection {
                dragging: true,
                ..Selection::line(file, column, at)
            });
        }
        let spot = self.spot_at(file, column, at, row, x, y);
        match click_count {
            1 => Some(Selection::caret(file, column, spot)),
            2 => {
                let text = &self.state.files[file].hunks[at.0 as usize].lines[at.1 as usize].text;
                let word = word_at(text, spot.byte);
                if word.is_empty() {
                    return Some(Selection::line(file, column, at));
                }
                Some(Selection {
                    head: Spot::at(at.0, at.1, word.end),
                    dragging: false,
                    ..Selection::caret(file, column, Spot::at(at.0, at.1, word.start))
                })
            }
            _ => Some(Selection::line(file, column, at)),
        }
    }

    fn drag_selection(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some(selection) = self.selection.filter(|selection| selection.dragging) else {
            return;
        };
        if !event.dragging() {
            self.end_selection(cx);
            return;
        }
        let (x, y) = (event.position.x - self.origin.0, event.position.y - self.origin.1);
        let Some(index) = self.state.tops.row_at(self.state.scroll() + f32::from(y)) else {
            return;
        };
        let row = self.state.rows.rows[index];
        let Some(at) = column_line(row.kind, selection.column).filter(|_| row.file as usize == selection.file) else {
            return;
        };
        let head = match selection.extent {
            Extent::Chars => self.spot_at(selection.file, selection.column, at, index, x, y),
            _ => Spot::at(at.0, at.1, 0),
        };
        if head != selection.head {
            self.selection = Some(Selection { head, ..selection });
            cx.notify();
        }
    }

    fn end_selection(&mut self, cx: &mut Context<Self>) {
        let Some(selection) = self.selection.filter(|selection| selection.dragging) else {
            return;
        };
        self.selection = Some(if selection.is_caret() {
            Selection::line(
                selection.file,
                selection.column,
                (selection.anchor.hunk, selection.anchor.line),
            )
        } else {
            Selection {
                dragging: false,
                ..selection
            }
        });
        cx.notify();
    }

    fn selection_text(&self, selection: Selection) -> String {
        let file = &self.state.files[selection.file];
        match selection.extent {
            Extent::Hunk => hunk_patch(&file.hunks[selection.anchor.hunk as usize]),
            _ => selection.text(file),
        }
    }

    fn copy_selection(&mut self, cx: &mut Context<Self>) {
        if let Some(selection) = self
            .selection
            .filter(|selection| !selection.dragging && !selection.is_caret())
        {
            cx.write_to_clipboard(ClipboardItem::new_string(self.selection_text(selection)));
        }
    }

    fn act_on_selection(&mut self, action: Action, cx: &mut Context<Self>) {
        let (Some(selection), Some(source)) = (self.selection, self.preview_source.clone()) else {
            return;
        };
        self.confirming = false;
        let file = self.state.files[selection.file].clone();
        let selected = selection.lines(&file);
        let task =
            cx.background_spawn(
                async move { stage_lines::run(&source.repo, &source.comparison, action, &file, &selected) },
            );
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(()) => {
                        this.selection = None;
                        this.stage_error = None;
                        cx.emit(DiffPaneEvent::Changed);
                    }
                    Err(message) => this.stage_error = Some(message),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn hud(&self) -> Div {
        let theme = &self.theme;
        glass_panel(theme)
            .h(px(HUD))
            .flex()
            .items_center()
            .gap(px(2.0))
            .px(px(4.0))
            .font(self.fonts.ui.clone())
    }

    fn hud_separator(&self) -> Div {
        div().w(px(1.0)).h(px(16.0)).mx(px(2.0)).bg(self.theme.ui.line)
    }

    fn selection_hud(&self, selection: Selection, y: f32, cx: &mut Context<Self>) -> Stateful<Div> {
        let file = &self.state.files[selection.file];
        let lines = selection.lines(file);
        let (Some(&first), Some(&last)) = (lines.first(), lines.last()) else {
            return div().id("selection-hud");
        };
        let changed = lines
            .iter()
            .filter(|&&(hunk, line)| file.hunks[hunk as usize].lines[line as usize].kind != LineKind::Context)
            .count();
        let open = self.open_line(selection.file, first);
        let can_comment = self.review.is_some() && {
            let line = &file.hunks[last.0 as usize].lines[last.1 as usize];
            line.new.is_some() || line.kind == LineKind::Deleted
        };
        let blocked = match &self.preview_source {
            Some(source) if source.comparison.includes_uncommitted() => {
                (changed == 0).then_some("Select a changed line first")
            }
            _ => Some("This comparison has no uncommitted changes"),
        };
        let button = |id: &'static str, glyph: Glyph, label: &'static str| {
            Button::new(id)
                .label(label)
                .look(Look::Ghost)
                .icon(glyph, 12.0)
                .h(px(26.0))
        };
        let stage_button = |id: &'static str, glyph: Glyph, label: &'static str, tip: &'static str| {
            button(id, glyph, label)
                .disabled(blocked.is_some())
                .tooltip(blocked.unwrap_or(tip))
        };
        let (left, width) = self.column_span(selection.column);
        let right = self.state.geometry().width - left - width;
        let count = lines.len();
        let mut bar = tag(self.hud().id("selection-hud"), || {
            format!("pane:selection-hud:{}:{count}", file.path)
        })
        .absolute()
        .top(px(y))
        .right(px(right + 8.0))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
        if let Some(message) = &self.stage_error {
            bar = bar.child(
                div()
                    .px(px(6.0))
                    .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SMALL))
                    .text_color(self.theme.danger)
                    .child(message.clone()),
            );
        }
        if self.confirming {
            let noun = if changed == 1 { "line" } else { "lines" };
            return bar
                .child(
                    div()
                        .px(px(6.0))
                        .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SECONDARY))
                        .child(format!("Discard {changed} changed {noun}?")),
                )
                .child(tag(
                    button("selection-discard-confirm", Glyph::Undo2, "Discard").on_click(
                        cx.listener(|this, _: &ClickEvent, _, cx| this.act_on_selection(Action::Discard, cx)),
                    ),
                    || "pane:selection-discard-confirm".to_string(),
                ))
                .child(tag(
                    button("selection-discard-cancel", Glyph::X, "Cancel").on_click(cx.listener(
                        |this, _: &ClickEvent, _, cx| {
                            this.confirming = false;
                            cx.notify();
                        },
                    )),
                    || "pane:selection-discard-cancel".to_string(),
                ));
        }
        bar.child(tag(
            stage_button("selection-stage", Glyph::Plus, "Stage", "Stage the selected lines")
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.act_on_selection(Action::Stage, cx))),
            || "pane:selection-stage".to_string(),
        ))
        .child(tag(
            stage_button(
                "selection-unstage",
                Glyph::Undo2,
                "Unstage",
                "Unstage the selected lines",
            )
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.act_on_selection(Action::Unstage, cx))),
            || "pane:selection-unstage".to_string(),
        ))
        .child(tag(
            stage_button(
                "selection-discard",
                Glyph::X,
                "Discard\u{2026}",
                "Discard the selected changes",
            )
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.confirming = true;
                this.stage_error = None;
                cx.notify();
            })),
            || "pane:selection-discard".to_string(),
        ))
        .child(self.hud_separator())
        .child(tag(
            button("selection-copy", Glyph::Copy, "Copy")
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.copy_selection(cx))),
            || "pane:selection-copy".to_string(),
        ))
        .child(tag(
            button("selection-open", Glyph::ExternalLink, "Open in Editor")
                .disabled(open.is_none())
                .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                    if let Some((path, line)) = open.clone() {
                        cx.emit(DiffPaneEvent::OpenInEditor { path, line });
                    }
                })),
            || "pane:selection-open".to_string(),
        ))
        .child(self.hud_separator())
        .child(tag(
            button("selection-comment", Glyph::MessageSquarePlus, "Comment")
                .disabled(!can_comment)
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                    this.selection = None;
                    this.add_comment(selection.file, last, window, cx);
                })),
            || "pane:selection-comment".to_string(),
        ))
        .child(self.hud_separator())
        .child(
            Button::new("selection-ask")
                .label("Ask Claude")
                .look(Look::Ghost)
                .icon_color(Glyph::Sparkles, 12.0, self.theme.ui.claude)
                .h(px(26.0))
                .disabled(true),
        )
    }

    fn line_menu(&self, menu_state: &LineMenu, cx: &mut Context<Self>) -> MenuAt {
        let label = match menu_state.line {
            Some(line) => format!("Open in Editor at Line {line}"),
            None => "Open in Editor".to_string(),
        };
        let pane = cx.entity();
        let (path, line) = (menu_state.path.clone(), menu_state.line);
        let close = {
            let pane = pane.clone();
            move |_: &mut Window, cx: &mut App| {
                pane.update(cx, |pane, cx| {
                    pane.line_menu = None;
                    cx.notify();
                })
            }
        };
        menu_at(
            "line-menu",
            menu_state.at,
            move |_, _| {
                let (pane, path) = (pane.clone(), path.clone());
                let open = MenuItem::new(label.clone(), move |_, cx| {
                    pane.update(cx, |_, cx| {
                        cx.emit(DiffPaneEvent::OpenInEditor {
                            path: path.clone(),
                            line,
                        })
                    })
                });
                vec![MenuSection::new(vec![open.into()])]
            },
            close,
        )
    }

    fn line_anchor(&self, file: usize, (hunk, line): (u32, u32)) -> Option<ThreadAnchor> {
        let file = &self.state.files[file];
        let diff_line = &file.hunks[hunk as usize].lines[line as usize];
        let (side, number) = match diff_line.kind {
            LineKind::Deleted => (Side::Old, diff_line.old),
            _ => (Side::New, diff_line.new),
        };
        Some(ThreadAnchor::line(&file.path, side, number?))
    }

    fn add_comment(&mut self, file: usize, at: (u32, u32), window: &mut Window, cx: &mut Context<Self>) {
        let (Some(anchor), Some(review)) = (self.line_anchor(file, at), &self.review) else {
            return;
        };
        let id = review
            .session
            .update(cx, |session, cx| session.create_thread(anchor, cx));
        self.focus_thread(id, window, cx);
    }

    pub fn comment_on_file(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(review) = &self.review else { return };
        let id = review
            .session
            .update(cx, |session, cx| session.create_thread(ThreadAnchor::file(path), cx));
        self.focus_thread(id, window, cx);
    }

    pub fn highlighted_files(&self) -> usize {
        self.highlights
            .iter()
            .filter(|highlight| matches!(highlight, Highlight::Ready(_)))
            .count()
    }

    pub fn is_highlighting(&self) -> bool {
        self.running > 0
    }

    fn sync(&mut self, cx: &mut Context<Self>) {
        if let Some(path) = self.state.take_visible_change() {
            self.share_visible(Some(path.clone()), cx);
            cx.emit(DiffPaneEvent::VisibleFile(path));
        }
        if let Some(mode) = self.state.take_shown_change() {
            cx.emit(DiffPaneEvent::ShownMode(mode));
        }
        if self.state.take_thread_steps_change(FILE_HEADER + HUNK).is_some() {
            cx.emit(DiffPaneEvent::ThreadSteps);
        }
        self.pump_highlights(cx);
        self.pump_previews(cx);
        cx.notify();
    }

    fn pump_highlights(&mut self, cx: &mut Context<Self>) {
        for index in self.state.files_to_highlight() {
            if self.running >= MAX_RUNNING_HIGHLIGHTS {
                break;
            }
            if !matches!(self.highlights.get(index), Some(Highlight::Missing)) {
                continue;
            }
            self.highlights[index] = Highlight::Running;
            self.running += 1;
            let file = Arc::clone(&self.state.files[index]);
            let task = cx.background_spawn({
                let file = Arc::clone(&file);
                async move { highlight::engine().highlight(&file) }
            });
            cx.spawn(async move |this, cx| {
                let tokens = task.await;
                this.update(cx, |this, cx| this.finish_highlight(&file, tokens, cx))
                    .ok();
            })
            .detach();
        }
    }

    fn pump_previews(&mut self, cx: &mut Context<Self>) {
        let Some(source) = self.preview_source.clone() else {
            return;
        };
        for index in self.state.previews_to_load() {
            let file = Arc::clone(&self.state.files[index]);
            if self
                .previews
                .get(&file.path)
                .is_some_and(|known| Arc::ptr_eq(&known.file, &file))
            {
                continue;
            }
            self.previews.insert(
                file.path.clone(),
                Preview {
                    file: Arc::clone(&file),
                    card: None,
                    measured: None,
                    _resized: None,
                },
            );
            let task = cx.background_spawn({
                let (source, file) = (source.clone(), Arc::clone(&file));
                async move { preview::load(&source.repo, &source.comparison, &file) }
            });
            cx.spawn(async move |this, cx| {
                let loaded = task.await;
                this.update(cx, |this, cx| this.finish_preview(&file, loaded, cx)).ok();
            })
            .detach();
        }
    }

    fn finish_preview(&mut self, file: &Arc<FileDiff>, loaded: Option<preview::Loaded>, cx: &mut Context<Self>) {
        let width = self.state.geometry().width;
        let Some(entry) = self.previews.get_mut(&file.path) else {
            return;
        };
        if !Arc::ptr_eq(&entry.file, file) {
            return;
        }
        if let Some(loaded) = loaded {
            let card = cx.new(|_| PreviewCard::new(file.path.clone(), Arc::new(loaded), width));
            entry._resized = Some(cx.subscribe(&card, |_, _, _: &PreviewEvent, cx| cx.notify()));
            entry.card = Some(card);
        }
        cx.notify();
    }

    fn measure_previews(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let width = self.state.geometry().width;
        let stale: Vec<(String, Entity<PreviewCard>, ImageMode)> = self
            .previews
            .iter()
            .filter(|(path, _)| self.state.shows_preview(path))
            .filter_map(|(path, entry)| {
                let card = entry.card.clone()?;
                let mode = card.read(cx).mode();
                (entry.measured != Some((width, mode))).then(|| (path.clone(), card, mode))
            })
            .collect();
        let mut changed = false;
        for (path, card, mode) in stale {
            card.update(cx, |card, _| card.set_width(width));
            let loaded = card.read(cx).loaded();
            let height = preview::height(&loaded, width, mode, window, cx);
            changed |= self.state.set_preview_height(&path, height + 1.0);
            if let Some(entry) = self.previews.get_mut(&path) {
                entry.measured = Some((width, mode));
            }
        }
        changed
    }

    fn finish_highlight(&mut self, file: &Arc<FileDiff>, tokens: Option<FileTokens>, cx: &mut Context<Self>) {
        self.running = self.running.saturating_sub(1);
        if let Some(index) = self.state.file_index(&file.path)
            && Arc::ptr_eq(&self.state.files[index], file)
        {
            self.highlights[index] = tokens.map_or(Highlight::Plain, |tokens| Highlight::Ready(Arc::new(tokens)));
            self.lines.invalidate_file(index as u32);
        }
        self.pump_highlights(cx);
        cx.notify();
    }

    fn toggle_collapsed(&mut self, path: &str, cx: &mut Context<Self>) {
        self.state.toggle_collapsed(path);
        self.sync(cx);
    }

    fn load_diff(&mut self, path: &str, cx: &mut Context<Self>) {
        self.state.load_diff(path);
        self.lines.clear();
        self.sync(cx);
    }

    fn prepare_lines(
        &mut self,
        bounds: gpui::Bounds<gpui::Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Frame {
        let started = self.probe.enabled.then(Instant::now);
        let geometry = Geometry {
            width: f32::from(bounds.size.width),
            height: f32::from(bounds.size.height),
            cell: self.fonts.cell,
        };
        self.origin = (bounds.origin.x, bounds.origin.y);
        let before = self.state.geometry();
        if geometry != before || self.remeasure {
            self.state.set_geometry(geometry);
            if geometry.width != before.width || self.remeasure {
                self.remeasure = false;
                self.measure_all(window, cx);
            }
            self.sync(cx);
        }
        if self.measure_previews(window, cx) {
            self.sync(cx);
        }
        let style = Style {
            fonts: &self.fonts,
            theme: &self.theme,
            wrap: self.state.wrap_cols(),
        };
        self.lines.begin_frame(&style);
        let scroll = self.state.scroll();
        let viewport = geometry.height;
        let selection = self.selection;
        let mut painted = Vec::new();
        let mut fillers = Vec::new();
        for index in self.state.visible_rows() {
            let row = self.state.rows.rows[index];
            let (hunk, cells) = match row.kind {
                RowKind::Line { hunk, line } => (hunk, [(Column::Unified, Some(line)), (Column::Unified, None)]),
                RowKind::Pair { hunk, left, right } => (hunk, [(Column::Left, left), (Column::Right, right)]),
                _ => continue,
            };
            let (y, height) = (self.state.tops.top(index) - scroll, self.state.tops.height(index));
            let file = &self.state.files[row.file as usize];
            for (column, line) in cells {
                let Some(line) = line else {
                    if column != Column::Unified {
                        fillers.push(PaintFiller { y, height, column });
                    }
                    continue;
                };
                let tokens = tokens_of(self.highlights.get(row.file as usize));
                let diff_line = &file.hunks[hunk as usize].lines[line as usize];
                let side = side_of(tokens, column, diff_line.kind, hunk, line);
                let selected = selection
                    .filter(|selection| selection.file == row.file as usize && selection.column == column)
                    .and_then(|selection| selection.chars_in(hunk, line, diff_line.text.len()));
                let spans = tokens.map_or(&[][..], |tokens| tokens.line_on(side, hunk as usize, line as usize));
                let request = LineRequest {
                    key: LineKey {
                        file: row.file,
                        hunk,
                        line,
                        side,
                    },
                    hunk: &file.hunks[hunk as usize],
                    spans,
                    y,
                    height,
                    column,
                    selected,
                };
                painted.push(self.lines.line(&style, window, request, viewport));
            }
        }
        if let Some(started) = started {
            self.probe.lines_ms.push(started.elapsed().as_secs_f32() * 1000.0);
        }
        Frame {
            scroll_x: self.state.scroll_x(),
            theme: self.theme,
            column_width: column_width(geometry.width),
            lines: painted,
            fillers,
            thumb: self.thumb(window),
        }
    }

    fn thumb(&self, window: &mut Window) -> Option<Thumb> {
        const VISIBLE_SECONDS: f32 = 1.0;
        const FADE_SECONDS: f32 = 0.3;
        let thumb = self.state.thumb()?;
        let held = self.lane_hovered || self.thumb_drag.is_some();
        let faded = if held {
            1.0
        } else {
            let elapsed = self.last_scroll?.elapsed().as_secs_f32();
            if elapsed >= VISIBLE_SECONDS {
                return None;
            }
            window.request_animation_frame();
            (VISIBLE_SECONDS - elapsed) / FADE_SECONDS
        };
        Some(Thumb {
            top: thumb.top,
            height: thumb.height,
            opacity: 0.6 * faded.min(1.0),
            wide: held,
        })
    }

    fn press_lane(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) {
        self.thumb_drag = self.state.press_thumb(f32::from(event.position.y - self.origin.1));
        self.sync(cx);
    }

    fn drag_thumb(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some(drag) = self.thumb_drag else { return };
        self.state.drag_thumb(drag, f32::from(event.position.y - self.origin.1));
        self.sync(cx);
    }

    fn release_thumb(&mut self, cx: &mut Context<Self>) {
        if self.thumb_drag.take().is_some() {
            self.last_scroll = Some(Instant::now());
            cx.notify();
        }
    }

    fn scroll_lane(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        div()
            .id("scroll-lane")
            .absolute()
            .top_0()
            .right_0()
            .h_full()
            .w(px(SCROLL_LANE))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                this.lane_hovered = *hovered;
                if !hovered {
                    this.last_scroll = Some(Instant::now());
                }
                cx.notify();
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    this.press_lane(event, cx);
                }),
            )
    }

    fn finish_frame(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.probe.end();
        if matches!(self.first_paint, FirstPaint::Waiting) && !self.state.rows.rows.is_empty() {
            self.first_paint = FirstPaint::Scheduled;
            let pane = cx.entity();
            window.on_next_frame(move |_, cx| {
                pane.update(cx, |pane, cx| {
                    pane.first_paint = FirstPaint::Sent;
                    cx.emit(DiffPaneEvent::FirstPaint);
                });
            });
            window.request_animation_frame();
        }
    }

    fn placed(y: f32, height: f32) -> Div {
        div().absolute().top(px(y)).left_0().w_full().h(px(height))
    }

    fn file_header(&self, file: usize, glass: Option<Hsla>, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = &self.theme;
        let pinned = glass.is_some();
        let diff = &self.state.files[file];
        let collapsed = self.state.collapsed.contains(&diff.path);
        let viewed = self.state.viewed.contains(&diff.path);
        let collapse_path = diff.path.clone();
        let viewed_path = diff.path.clone();
        let open_path = diff.path.clone();
        let comment_path = diff.path.clone();
        let copy_path = diff.path.clone();
        let reveal_path = diff.path.clone();
        let open_line = diff.first_changed_new_line();
        let can_open = diff.status != FileStatus::Deleted;
        let threads = self
            .state
            .blocks
            .iter()
            .filter(|slot| matches!(slot.id, BlockId::Thread(_)) && slot.anchor.path() == diff.path)
            .count();
        let chevron = div()
            .id((if pinned { "pinned-collapse" } else { "collapse" }, file))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .w(px(12.0))
            .h(px(20.0))
            .cursor_pointer()
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.toggle_collapsed(&collapse_path, cx)))
            .child(glyph_icon(
                if collapsed { Glyph::CaretRight } else { Glyph::CaretDown },
                12.0,
                theme.diff.muted,
            ));
        let id = if pinned { "pinned-header" } else { "file-header" };
        let left = div()
            .flex()
            .items_center()
            .gap(px(7.0))
            .min_w_0()
            .child(chevron)
            .child(glyph_icon(
                Glyph::FileLines,
                14.0,
                theme.tint(file_kind(&diff.path).tint()),
            ))
            .child(chrome::path_text(theme, diff))
            .child(status_badge(
                diff.status.code(),
                BadgeTone::Plain,
                self.fonts.mono.clone(),
                theme,
            ))
            .child(chrome::counts(theme, &self.fonts.mono, diff.additions, diff.deletions))
            .child(chrome::minibar(theme, diff.additions, diff.deletions))
            .children((threads > 0).then(|| chrome::thread_count(theme, threads)));
        let bar = div()
            .flex()
            .items_center()
            .h(px(FILE_HEADER))
            .w_full()
            .pl(px(10.0))
            .pr(px(6.0))
            .child(left)
            .child(div().flex_1())
            .children(
                self.uncommitted
                    .contains(&diff.path)
                    .then(|| chip_label(theme, "Uncommitted", ChipTone::Amber).mr(px(10.0))),
            )
            .children(
                self.view_switch(file, pinned, cx)
                    .map(|switch| div().flex().items_center().child(switch).child(div().w(px(6.0)))),
            )
            .child(tag(
                bar_button(
                    theme,
                    "file-copy-path",
                    Glyph::Copy,
                    "Copy Path",
                    true,
                    false,
                    move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(copy_path.clone())),
                ),
                || format!("pane:copy-path:{}", diff.path),
            ))
            .child(tag(
                bar_button(
                    theme,
                    "file-reveal",
                    Glyph::Folder,
                    "Reveal in Finder",
                    can_open,
                    false,
                    cx.listener(move |_, _: &ClickEvent, _, cx| {
                        cx.emit(DiffPaneEvent::RevealInFinder(reveal_path.clone()))
                    }),
                ),
                || format!("pane:reveal-in-finder:{}", diff.path),
            ))
            .child(tag(
                bar_button(
                    theme,
                    "file-comment",
                    Glyph::MessageSquarePlus,
                    "Comment on file",
                    self.review.is_some(),
                    false,
                    cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.comment_on_file(&comment_path, window, cx)
                    }),
                ),
                || format!("pane:comment-on-file:{}", diff.path),
            ))
            .child(tag(
                bar_button(
                    theme,
                    "file-open",
                    Glyph::ExternalLink,
                    "Open in Editor",
                    can_open,
                    false,
                    cx.listener(move |_, _: &ClickEvent, _, cx| {
                        cx.emit(DiffPaneEvent::OpenInEditor {
                            path: open_path.clone(),
                            line: open_line,
                        })
                    }),
                ),
                || format!("pane:open-in-editor:{}", diff.path),
            ))
            .child(self.more_menu(file, pinned, cx))
            .child(div().w(px(8.0)))
            .child(tag(
                viewed_toggle(
                    theme,
                    (if pinned { "pinned-viewed" } else { "viewed" }, file),
                    viewed,
                    State::Normal,
                )
                .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                    cx.emit(DiffPaneEvent::ToggleViewed(viewed_path.clone()))
                })),
                || format!("pane:viewed:{}", diff.path),
            ));
        let bar = bar.id((id, file));
        match glass {
            Some(glass) => bar.border_b_1().border_color(theme.diff.line).bg(glass),
            None => bar.border_y_1().border_color(theme.diff.line).bg(theme.background),
        }
    }

    fn view_switch(&self, file: usize, pinned: bool, cx: &mut Context<Self>) -> Option<Div> {
        let theme = &self.theme;
        let path = self.state.files[file].path.clone();
        if !self.state.previewable(&path) {
            return None;
        }
        const LABELS: [&str; 2] = ["Preview", "Diff"];
        let selected = if self.state.shows_preview(&path) { 0 } else { 1 };
        let pane = cx.entity();
        let target = path.clone();
        Some(label_switch(
            theme,
            if pinned {
                "pinned-preview-switch"
            } else {
                "preview-switch"
            },
            &LABELS,
            selected,
            move |index| format!("pane:preview-switch:{path}:{}", LABELS[index]),
            Rc::new(move |index, _, cx| {
                pane.update(cx, |pane, cx| pane.set_preview(&target, index == 0, cx));
            }),
        ))
    }

    fn preview_row(&self, file: usize, height: f32) -> impl IntoElement {
        let theme = &self.theme;
        let path = &self.state.files[file].path;
        let body = match self.previews.get(path).and_then(|entry| entry.card.clone()) {
            Some(card) => tag(div().size_full(), || format!("pane:preview-card:{path}")).child(card),
            None => div()
                .flex()
                .items_center()
                .justify_center()
                .size_full()
                .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SECONDARY))
                .text_color(theme.diff.muted)
                .child("Loading preview\u{2026}"),
        };
        tag(div().w_full().h(px(height)).overflow_hidden(), || {
            format!("pane:preview:{path}")
        })
        .border_b_1()
        .border_color(theme.diff.line)
        .child(body)
    }

    fn more_menu(&self, file: usize, pinned: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let diff = self.state.files[file].clone();
        let collapsed = self.state.collapsed.contains(&diff.path);
        let viewed = self.state.viewed.contains(&diff.path);
        let pane = cx.entity();
        let id = format!("{}:{}", if pinned { "pinned-more" } else { "file-more" }, diff.path);
        let trigger = tag(
            bar_button(
                &self.theme,
                "file-more",
                Glyph::Ellipsis,
                "More",
                true,
                false,
                |_, _, _| {},
            ),
            || format!("pane:{id}"),
        );
        menu_button(id, trigger, move |_, _| {
            let emit = |event: DiffPaneEvent| {
                let pane = pane.clone();
                move |_: &mut Window, cx: &mut App| pane.update(cx, |_, cx| cx.emit(event.clone()))
            };
            let patch = file_patch(&diff);
            let copy_diff = MenuItem::new("Copy Diff", move |_, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(patch.clone()))
            })
            .icon(Glyph::Copy, None);
            let toggle_path = diff.path.clone();
            let toggle_pane = pane.clone();
            let collapse = MenuItem::new(if collapsed { "Expand File" } else { "Collapse File" }, move |_, cx| {
                toggle_pane.update(cx, |pane, cx| pane.toggle_collapsed(&toggle_path, cx))
            });
            let mark = MenuItem::new(
                if viewed { "Mark as Not Viewed" } else { "Mark as Viewed" },
                emit(DiffPaneEvent::ToggleViewed(diff.path.clone())),
            );
            vec![
                MenuSection::new(vec![copy_diff.into()]),
                MenuSection::new(vec![collapse.into(), mark.into()]),
            ]
        })
        .anchor(gpui::Corner::TopRight)
    }

    fn hunk_bar(&self, file: usize, hunk: usize, glass: Option<Hsla>) -> Div {
        let theme = &self.theme;
        let source = &self.state.files[file].hunks[hunk];
        let bar = tag(div(), || format!("pane:hunk:{}:{hunk}", self.state.files[file].path))
            .flex()
            .items_center()
            .h(px(HUNK))
            .w_full()
            .pl(px(12.0))
            .pr(px(8.0))
            .whitespace_nowrap()
            .font(self.fonts.mono.clone())
            .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SMALL))
            .text_color(theme.diff.muted)
            .child(div().min_w_0().overflow_hidden().child(source.header.clone()));
        match glass {
            Some(glass) => bar.border_b_1().border_color(theme.diff.line).bg(glass),
            None => bar.border_y_1().border_color(theme.diff.line).bg(theme.diff.hunk),
        }
    }

    fn notice(&self, file: usize, notice: Notice, cx: &mut Context<Self>) -> Div {
        let theme = &self.theme;
        let path = self.state.files[file].path.clone();
        let mut body = tag(div(), || format!("pane:notice:{path}"))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(8.0))
            .h(px(NOTICE))
            .w_full()
            .border_b_1()
            .border_color(theme.diff.line)
            .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SECONDARY))
            .text_color(theme.diff.muted)
            .child(SharedString::from(notice.text()));
        if notice.can_load() {
            body = body.child(
                div()
                    .id(("load-diff", file))
                    .flex()
                    .items_center()
                    .h(px(22.0))
                    .px(px(8.0))
                    .rounded(px(5.0))
                    .border_1()
                    .border_color(theme.diff.line)
                    .bg(theme.diff.cap)
                    .cursor_pointer()
                    .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SECONDARY))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.diff.text)
                    .child("Load diff")
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.load_diff(&path, cx))),
            );
        }
        body
    }

    fn block_row(&self, slot: u32, y: f32, height: f32, cx: &App) -> Option<Div> {
        let thread = &self.state.blocks[slot as usize];
        let card: AnyView = match thread.id {
            BlockId::Thread(id) => self.card(id)?.clone().into(),
            BlockId::Note(index) => self.notes.get(index as usize)?.view.clone().into(),
        };
        let key = match (thread.id, &self.review) {
            (BlockId::Thread(id), Some(review)) => review.session.read(cx).thread(id).map(card_key),
            _ => None,
        };
        let frame = ThreadFrame::of(&thread.anchor, self.state.split_width());
        let card_height = height - frame.top - frame.bottom - if frame.rule { 1.0 } else { 0.0 };
        let style = StyleRefinement::default().w_full().h(px(card_height));
        let mut row = tag(Self::placed(y, height), || {
            format!("pane:thread-row:{}", key.unwrap_or_default())
        })
        .overflow_hidden()
        .pl(px(frame.left))
        .pr(px(frame.right))
        .pt(px(frame.top))
        .pb(px(frame.bottom));
        if frame.rule {
            row = row.border_b_1().border_color(self.theme.diff.line);
        }
        if let Some(width) = self.state.split_width().filter(|_| !frame.rule) {
            row = row.child(
                div()
                    .absolute()
                    .top_0()
                    .left(px(column_width(width)))
                    .w(px(SPLIT_DIVIDER))
                    .h_full()
                    .bg(self.theme.diff.line),
            );
        }
        Some(row.child(card.cached(style)))
    }

    fn column_span(&self, column: Column) -> (f32, f32) {
        let width = self.state.geometry().width;
        let half = column_width(width);
        match column {
            Column::Unified => (0.0, width),
            Column::Left => (0.0, half),
            Column::Right => (half + SPLIT_DIVIDER, width - half - SPLIT_DIVIDER),
        }
    }

    fn add_comment_button(&self, hovered: Hovered, y: f32, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = &self.theme;
        let numbers = if hovered.column == Column::Unified { 2.0 } else { 1.0 };
        let left = self.column_span(hovered.column).0 + NUMBER_COLUMN * numbers - 18.0;
        let anchor = self.line_anchor(hovered.file, (hovered.hunk, hovered.line));
        tag(div().id(("add-comment", hovered.row)), || match anchor {
            Some(ThreadAnchor::Line { path, side, line }) => format!("pane:add-comment:{path}:{side:?}:{line}"),
            _ => "pane:add-comment".to_string(),
        })
        .absolute()
        .top(px(y + 1.0))
        .left(px(left))
        .flex()
        .items_center()
        .justify_center()
        .size(px(18.0))
        .rounded(px(4.0))
        .bg(theme.accent)
        .cursor_pointer()
        .child(glyph_icon(Glyph::Plus, 12.0, theme.on_accent))
        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
            this.add_comment(hovered.file, (hovered.hunk, hovered.line), window, cx)
        }))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
    }

    fn backdrop(&self, y: f32) -> Hsla {
        let theme = &self.theme;
        let Some(index) = self.state.tops.row_at(self.state.scroll() + y.max(0.0)) else {
            return theme.background;
        };
        let row = self.state.rows.rows[index];
        match row.kind {
            RowKind::Hunk { .. } => theme.diff.hunk,
            RowKind::Line { hunk, line } => {
                match self.state.files[row.file as usize].hunks[hunk as usize].lines[line as usize].kind {
                    LineKind::Added => theme.diff.add_bg,
                    LineKind::Deleted => theme.diff.del_bg,
                    LineKind::Context => theme.background,
                }
            }
            _ => theme.background,
        }
    }

    fn sticky(&self, sticky: Sticky, cx: &mut Context<Self>) -> Div {
        let file = sticky.file as usize;
        let mut group = div()
            .absolute()
            .top(px(sticky.push))
            .left_0()
            .w_full()
            .flex()
            .flex_col();
        let glass = |center: f32| Some(self.theme.pinned_glass(self.backdrop(sticky.push + center)));
        group = group.child(self.file_header(file, glass(FILE_HEADER / 2.0), cx));
        if let Some(hunk) = sticky.hunk {
            group = group.child(self.hunk_bar(file, hunk as usize, glass(FILE_HEADER + HUNK / 2.0)));
        }
        group
    }
}

impl Render for DiffPane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.probe.begin();
        self.measure_previews(window, cx);
        let current = Theme::of(cx);
        if self.theme.dark != current.dark {
            self.lines.clear();
        }
        self.theme = current;
        let theme = self.theme;
        let scroll = self.state.scroll();
        let mut root = region(tag(div().id("diff-pane"), || "pane"), || "pane".into())
            .key_context(KEY_CONTEXT)
            // On the root, gpui would focus the pane on every click inside it and take focus from a comment box.
            .child(div().size_0().track_focus(&self.focus))
            .on_action(cx.listener(|this, _: &CopySelection, _, cx| this.copy_selection(cx)))
            .on_action(cx.listener(|this, _: &ClearSelection, _, cx| {
                if this.selection.take().is_some() {
                    this.confirming = false;
                    this.stage_error = None;
                    cx.notify();
                }
            }))
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(theme.background)
            .text_color(theme.diff.text)
            .font(self.fonts.ui.clone())
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
                let delta = event.delta.pixel_delta(px(LINE));
                this.scroll_by(-f32::from(delta.y), -f32::from(delta.x), cx);
            }))
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                this.drag_selection(event, cx);
                let before = this.hovered();
                this.mouse = Some((event.position.x - this.origin.0, event.position.y - this.origin.1));
                if this.hovered() != before {
                    cx.notify();
                }
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, event: &MouseDownEvent, _, cx| this.open_menu(event, cx)),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    if event.modifiers.platform {
                        this.open_from_gutter(event, cx);
                    } else {
                        this.start_selection(event, window, cx);
                    }
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| this.end_selection(cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| this.end_selection(cx)),
            )
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if !hovered && this.mouse.take().is_some() {
                    cx.notify();
                }
            }));
        if self.state.files.is_empty() {
            return root
                .flex()
                .items_center()
                .justify_center()
                .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::BODY))
                .text_color(theme.diff.muted)
                .child("No changes");
        }
        let pane = cx.entity();
        let painter = pane.clone();
        root = root.child(
            canvas(
                move |bounds, window, cx| pane.update(cx, |pane, cx| pane.prepare_lines(bounds, window, cx)),
                move |bounds, frame, window, cx: &mut App| {
                    lines::paint(&frame, bounds, window, cx);
                    if painter.read(cx).thumb_drag.is_some() {
                        let pane = painter.clone();
                        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                            if phase == DispatchPhase::Bubble {
                                pane.update(cx, |pane, cx| pane.drag_thumb(event, cx));
                            }
                        });
                        let pane = painter.clone();
                        window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                            if phase == DispatchPhase::Bubble && event.button == MouseButton::Left {
                                pane.update(cx, |pane, cx| pane.release_thumb(cx));
                            }
                        });
                    }
                    painter.update(cx, |pane, cx| pane.finish_frame(window, cx));
                },
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
        );
        let mut hud_layer = None;
        for index in self.state.visible_rows() {
            let row = self.state.rows.rows[index];
            let y = self.state.tops.top(index) - scroll;
            let height = self.state.tops.height(index);
            let file = row.file as usize;
            let element = match row.kind {
                RowKind::Line { .. } | RowKind::Pair { .. } | RowKind::Gap => continue,
                RowKind::Block { slot } => match self.block_row(slot, y, height, cx) {
                    Some(thread) => thread,
                    None => continue,
                },
                RowKind::FileHeader => Self::placed(y, height).child(self.file_header(file, None, cx)),
                RowKind::Hunk { hunk } => Self::placed(y, height).child(self.hunk_bar(file, hunk as usize, None)),
                RowKind::Notice(notice) => Self::placed(y, height).child(self.notice(file, notice, cx)),
                RowKind::Preview => Self::placed(y, height).child(self.preview_row(file, height)),
            };
            root = root.child(element);
        }
        if let Some(selection) = self.selection.filter(|selection| !selection.is_caret()) {
            let (left, width) = self.column_span(selection.column);
            let mut first = None;
            for index in self.state.visible_rows() {
                let row = self.state.rows.rows[index];
                if !selection.covers(row) {
                    continue;
                }
                let y = self.state.tops.top(index) - scroll;
                first.get_or_insert((y, matches!(row.kind, RowKind::Hunk { .. })));
                if selection.extent == Extent::Chars {
                    continue;
                }
                root = root.child(
                    Self::placed(y, self.state.tops.height(index))
                        .left(px(left))
                        .w(px(width))
                        .bg(theme.accent.opacity(0.24)),
                );
            }
            if let Some((y, on_header)) = first.filter(|_| !selection.dragging) {
                let sticky = self.state.position().and_then(|position| position.sticky);
                let floor = sticky.map_or(0.0, |sticky| {
                    sticky.push + FILE_HEADER + sticky.hunk.map_or(0.0, |_| HUNK)
                });
                let top = if on_header {
                    y + (HUNK - HUD) / 2.0
                } else {
                    y - HUD - 4.0
                };
                hud_layer = Some(self.selection_hud(selection, top.max(floor + 4.0), cx));
            }
        }
        if let Some(hovered) = self
            .hovered()
            .filter(|_| !self.selection.is_some_and(|selection| selection.dragging))
        {
            let y = self.state.tops.top(hovered.row) - scroll;
            let height = self.state.tops.height(hovered.row);
            let (left, width) = self.column_span(hovered.column);
            root = root.child(
                Self::placed(y, height)
                    .left(px(left))
                    .w(px(width))
                    .bg(theme.diff.text.opacity(0.05)),
            );
            root = root.child(self.add_comment_button(hovered, y, cx));
        }
        if let Some(sticky) = self.state.position().and_then(|position| position.sticky) {
            root = root.child(self.sticky(sticky, cx));
        }
        if self.state.max_scroll() > 0.0 {
            root = root.child(self.scroll_lane(cx));
        }
        if let Some(hud) = hud_layer {
            root = root.child(hud);
        }
        if let Some(menu_state) = &self.line_menu {
            root = root.child(self.line_menu(menu_state, cx));
        }
        root
    }
}

fn tokens_of(highlight: Option<&Highlight>) -> Option<&Arc<FileTokens>> {
    match highlight {
        Some(Highlight::Ready(tokens)) => Some(tokens),
        _ => None,
    }
}

fn side_of(tokens: Option<&Arc<FileTokens>>, column: Column, kind: LineKind, hunk: u32, line: u32) -> Side {
    let differs = tokens.is_some_and(|tokens| tokens.old_context_differs(hunk as usize, line as usize));
    column.side(kind, differs)
}

fn hunk_patch(hunk: &diffpilot_core::diff::Hunk) -> String {
    let mut patch = hunk.header.clone();
    for line in &hunk.lines {
        patch.push('\n');
        patch.push(match line.kind {
            LineKind::Added => '+',
            LineKind::Deleted => '-',
            LineKind::Context => ' ',
        });
        patch.push_str(&line.text);
    }
    patch
}

fn file_patch(diff: &FileDiff) -> String {
    let old = match diff.status {
        FileStatus::Added => "/dev/null".to_string(),
        _ => format!("a/{}", diff.old_path.as_deref().unwrap_or(&diff.path)),
    };
    let new = match diff.status {
        FileStatus::Deleted => "/dev/null".to_string(),
        _ => format!("b/{}", diff.path),
    };
    let mut patch = format!("--- {old}\n+++ {new}");
    for hunk in &diff.hunks {
        patch.push('\n');
        patch.push_str(&hunk_patch(hunk));
    }
    patch
}
