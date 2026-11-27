use crate::bench::{Bench, Mark};
use crate::format::{compact, file_count, grouped};
use crate::sidebar_rows::{
    Arrow, ArrowOutcome, Row, RowInput, RowKind, arrow, build_rows, index_of, scroll_target, step,
};
use diffpilot_core::changes::{ChangeSummary, FileChange};
use diffpilot_core::file_kind::file_kind;
use diffpilot_core::tree::{FileEntry, FileFilter, Folder, SidebarMode};
use diffpilot_ui_kit::components::{BadgeTone, State, filter_field, glass_button, status_badge, tag};
use diffpilot_ui_kit::{Glyph, Theme};
use gpui::{
    AnyElement, App, ClickEvent, Context, Entity, EventEmitter, FocusHandle, Focusable, InteractiveElement,
    IntoElement, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Subscription,
    UniformListScrollHandle, Window, actions, div, font, point, prelude::*, px, uniform_list,
};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::scroll::{Scrollbar, ScrollbarHandle};
use gpui_component::{ActiveTheme, Icon, Sizable, h_flex, v_flex};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

actions!(sidebar, [SelectPrevious, SelectNext, ArrowLeft, ArrowRight]);

pub const ROW_HEIGHT: f32 = 24.0;
const INDENT: f32 = 14.0;
const CONTEXT: &str = "Sidebar";

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        gpui::KeyBinding::new("up", SelectPrevious, Some(CONTEXT)),
        gpui::KeyBinding::new("down", SelectNext, Some(CONTEXT)),
        gpui::KeyBinding::new("left", ArrowLeft, Some(CONTEXT)),
        gpui::KeyBinding::new("right", ArrowRight, Some(CONTEXT)),
    ]);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SidebarEvent {
    Select(String),
}

pub struct LoadedChanges {
    pub changes: Vec<FileChange>,
    pub root: Folder,
    pub files: Vec<Arc<FileEntry>>,
    pub elapsed: std::time::Duration,
}

pub struct Sidebar {
    changes: Vec<FileChange>,
    root: Folder,
    files: Vec<Arc<FileEntry>>,
    filter: FileFilter,
    mode: SidebarMode,
    collapsed: HashSet<String>,
    viewed: HashSet<String>,
    comments: HashMap<String, usize>,
    rows: Vec<Row>,
    selected: Option<String>,
    summary: ChangeSummary,
    is_loading: bool,
    filter_input: Entity<InputState>,
    scroll: UniformListScrollHandle,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<SidebarEvent> for Sidebar {}

impl Focusable for Sidebar {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Sidebar {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let filter_input = cx.new(|cx| InputState::new(window, cx).placeholder("Filter"));
        let query = std::env::var("DIFFPILOT_FILTER").unwrap_or_default();
        if !query.is_empty() {
            filter_input.update(cx, |input, cx| input.set_value(query.clone(), window, cx));
        }
        let subscription = cx.subscribe_in(&filter_input, window, |this, input, event: &InputEvent, _, cx| {
            if matches!(event, InputEvent::Change) {
                let query = input.read(cx).value().to_string();
                if query != this.filter.query {
                    this.filter.query = query;
                    this.rebuild_rows(cx);
                }
            }
        });
        Self {
            changes: Vec::new(),
            root: Folder::default(),
            files: Vec::new(),
            filter: FileFilter {
                query,
                ..FileFilter::default()
            },
            mode: SidebarMode::Tree,
            collapsed: HashSet::new(),
            viewed: HashSet::new(),
            comments: HashMap::new(),
            rows: Vec::new(),
            selected: None,
            summary: ChangeSummary::default(),
            is_loading: true,
            filter_input,
            scroll: UniformListScrollHandle::new(),
            focus: cx.focus_handle(),
            _subscriptions: vec![subscription],
        }
    }

    pub fn scroll_handle(&self) -> UniformListScrollHandle {
        self.scroll.clone()
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    pub fn set_changes(&mut self, loaded: LoadedChanges, cx: &mut Context<Self>) {
        let first = self.is_loading;
        self.is_loading = false;
        if loaded.changes != self.changes {
            self.changes = loaded.changes;
            self.root = loaded.root;
            self.files = loaded.files;
            self.summary = ChangeSummary::new(&self.changes, &self.viewed);
            self.rebuild_rows(cx);
        } else {
            cx.notify();
        }
        if first {
            Bench::mark_after_paint(cx, Mark::SidebarLoaded);
        }
    }

    pub fn set_viewed(&mut self, viewed: HashSet<String>, cx: &mut Context<Self>) {
        if viewed == self.viewed {
            return;
        }
        self.viewed = viewed;
        self.summary = ChangeSummary::new(&self.changes, &self.viewed);
        if self.filter.hide_viewed {
            self.rebuild_rows(cx);
        } else {
            cx.notify();
        }
    }

    pub fn set_comments(&mut self, comments: HashMap<String, usize>, cx: &mut Context<Self>) {
        if comments != self.comments {
            self.comments = comments;
            cx.notify();
        }
    }

    fn rebuild_rows(&mut self, cx: &mut Context<Self>) {
        self.rows = build_rows(&RowInput {
            root: &self.root,
            files: &self.files,
            filter: &self.filter,
            mode: self.mode,
            viewed: &self.viewed,
            collapsed: &self.collapsed,
        });
        cx.notify();
    }

    pub(crate) fn select_index(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(row) = self.rows.get(index) else { return };
        self.selected = Some(row.path.clone());
        let reveal = (!row.is_folder()).then(|| row.path.clone());
        self.scroll_into_view(index);
        window.focus(&self.focus);
        if let Some(path) = reveal {
            cx.emit(SidebarEvent::Select(path));
        }
        cx.notify();
    }

    fn scroll_into_view(&self, index: usize) {
        let (offset, viewport) = {
            let state = self.scroll.0.borrow();
            (state.base_handle.offset().y, state.base_handle.bounds().size.height)
        };
        if let Some(target) = scroll_target(index, ROW_HEIGHT, -f32::from(offset), f32::from(viewport)) {
            self.scroll.set_offset(point(px(0.), px(-target)));
        }
    }

    fn toggle_folder(&mut self, path: &str, cx: &mut Context<Self>) {
        if self.filter.has_query() || self.mode == SidebarMode::List {
            return;
        }
        if !self.collapsed.remove(path) {
            self.collapsed.insert(path.to_string());
        }
        self.rebuild_rows(cx);
    }

    #[cfg(test)]
    fn set_mode(&mut self, mode: SidebarMode, cx: &mut Context<Self>) {
        self.mode = mode;
        self.rebuild_rows(cx);
    }

    fn toggle_uncommitted(&mut self, cx: &mut Context<Self>) {
        self.filter.only_uncommitted = !self.filter.only_uncommitted;
        self.rebuild_rows(cx);
    }

    fn toggle_hide_viewed(&mut self, cx: &mut Context<Self>) {
        self.filter.hide_viewed = !self.filter.hide_viewed;
        self.rebuild_rows(cx);
    }

    fn on_step(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = step(&self.rows, self.selected.as_deref(), delta) {
            self.select_index(index, window, cx);
        }
    }

    fn on_arrow(&mut self, key: Arrow, window: &mut Window, cx: &mut Context<Self>) {
        let outcome = arrow(
            &self.rows,
            self.selected.as_deref(),
            key,
            self.mode == SidebarMode::List,
        );
        match outcome {
            ArrowOutcome::Collapse(path) | ArrowOutcome::Expand(path) => self.toggle_folder(&path, cx),
            ArrowOutcome::Select(index) => self.select_index(index, window, cx),
            ArrowOutcome::Nothing => {}
        }
    }

    fn summary_header(&self, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let tokens = Theme::of(cx);
        let muted = tokens.chrome.secondary;
        let summary = &self.summary;
        let fraction = if summary.file_count == 0 {
            0.0
        } else {
            summary.viewed as f32 / summary.file_count as f32
        };
        let text = if self.is_loading {
            h_flex().child("Loading changes…").into_any_element()
        } else {
            h_flex()
                .gap_1()
                .child(file_count(summary.file_count))
                .child("·")
                .child(
                    h_flex()
                        .gap_1()
                        .font_family(theme.mono_font_family.clone())
                        .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SMALL))
                        .child(
                            div()
                                .text_color(tokens.success)
                                .child(format!("+{}", compact(summary.additions))),
                        )
                        .child(
                            div()
                                .text_color(tokens.danger)
                                .child(format!("−{}", compact(summary.deletions))),
                        ),
                )
                .child("·")
                .child(format!(
                    "{}/{} viewed",
                    grouped(summary.viewed),
                    grouped(summary.file_count)
                ))
                .into_any_element()
        };
        v_flex()
            .id("summary")
            .gap(px(7.))
            .px_3()
            .pt(px(5.))
            .pb(px(10.))
            .child(
                tag(div(), || {
                    format!("sidebar:summary:{}/{} viewed", summary.viewed, summary.file_count)
                })
                .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SECONDARY))
                .text_color(muted)
                .whitespace_nowrap()
                .overflow_hidden()
                .child(text),
            )
            .child(
                div().h(px(4.)).w_full().rounded_full().bg(theme.muted).child(
                    div()
                        .h_full()
                        .w(gpui::relative(fraction))
                        .rounded_full()
                        .bg(tokens.accent),
                ),
            )
            .into_any_element()
    }

    fn filter_row(&self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = Theme::of(cx);
        let uncommitted = self.filter.only_uncommitted;
        let hide_viewed = self.filter.hide_viewed;
        h_flex()
            .h(px(44.))
            .gap(px(6.))
            .px(px(10.))
            .border_y_1()
            .border_color(tokens.ui.line)
            .child(tag(
                filter_field(
                    &tokens,
                    Input::new(&self.filter_input)
                        .appearance(false)
                        .bordered(false)
                        .focus_bordered(false)
                        .small(),
                )
                .id("filter-field"),
                || "sidebar:filter",
            ))
            .child(glass_button(
                "toggle-uncommitted",
                Glyph::Pencil,
                "Only uncommitted",
                uncommitted,
                State::Normal,
                &tokens,
                cx.listener(|this, _, _, cx| this.toggle_uncommitted(cx)),
            ))
            .child(glass_button(
                "toggle-hide-viewed",
                Glyph::Eye,
                "Hide viewed",
                hide_viewed,
                State::Normal,
                &tokens,
                cx.listener(|this, _, _, cx| this.toggle_hide_viewed(cx)),
            ))
            .into_any_element()
    }
}

impl Render for Sidebar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let empty_text: Option<&'static str> = (!self.is_loading && self.rows.is_empty()).then(|| {
            if self.filter.is_active() {
                "No matching files"
            } else {
                "No changes"
            }
        });
        let muted = theme.muted_foreground;
        let count = self.rows.len();
        let header = self.summary_header(cx);
        let filter_row = self.filter_row(cx);

        let list = uniform_list(
            "sidebar-rows",
            count,
            cx.processor(move |this, range: std::ops::Range<usize>, _window, cx| {
                range
                    .filter_map(|index| {
                        let row = this.rows.get(index)?;
                        Some(this.render_row(index, row, cx))
                    })
                    .collect()
            }),
        )
        .track_scroll(self.scroll.clone())
        .size_full();

        v_flex()
            .id("sidebar")
            .size_full()
            .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SECONDARY))
            .child(header)
            .child(filter_row)
            .child(
                tag(div().id("file-outline"), || format!("sidebar:rows:{}", self.rows.len()))
                    .key_context(CONTEXT)
                    .track_focus(&self.focus)
                    .on_action(cx.listener(|this, _: &SelectPrevious, window, cx| this.on_step(-1, window, cx)))
                    .on_action(cx.listener(|this, _: &SelectNext, window, cx| this.on_step(1, window, cx)))
                    .on_action(cx.listener(|this, _: &ArrowLeft, window, cx| this.on_arrow(Arrow::Left, window, cx)))
                    .on_action(cx.listener(|this, _: &ArrowRight, window, cx| this.on_arrow(Arrow::Right, window, cx)))
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(list)
                    .child(Scrollbar::vertical(&self.scroll))
                    .when_some(empty_text, |this, text| {
                        this.child(
                            div()
                                .absolute()
                                .inset_0()
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_color(muted)
                                .child(text),
                        )
                    }),
            )
    }
}

impl Sidebar {
    fn render_row(&self, index: usize, row: &Row, cx: &mut Context<Self>) -> AnyElement {
        let tokens = Theme::of(cx);
        let selected = self.selected.as_deref() == Some(row.path.as_str());
        let on_accent = selected;
        let primary = if on_accent {
            tokens.on_accent
        } else {
            tokens.chrome.label
        };
        let secondary = if on_accent {
            tokens.on_accent_dim
        } else {
            tokens.chrome.secondary
        };
        let tertiary = if on_accent {
            tokens.on_accent_dim
        } else {
            tokens.chrome.tertiary
        };
        let selection_bg = tokens.accent;
        let hover_bg = tokens.hover;
        let path = row.path.clone();
        let depth = row.depth as f32;

        let base = tag(h_flex().id(("row", index)), || format!("sidebar:row:{path}"))
            .group("row")
            .w_full()
            .h(px(ROW_HEIGHT))
            .pl(px(0.5 + depth * INDENT))
            .pr(px(15.5))
            .gap(px(6.))
            .rounded(px(8.))
            .cursor_pointer()
            .when(selected, |this| this.bg(selection_bg))
            .when(!selected, |this| this.hover(|style| style.bg(hover_bg)))
            .on_click(cx.listener({
                let path = path.clone();
                move |this, event: &ClickEvent, window, cx| {
                    if let Some(index) = index_of(&this.rows, &path) {
                        this.select_index(index, window, cx);
                        if event.click_count() == 2 && this.rows[index].is_folder() {
                            this.toggle_folder(&path, cx);
                        }
                    }
                }
            }));

        let element = match &row.kind {
            RowKind::Folder { name, expanded } => {
                let chevron_path = path.clone();
                let tree_mode = self.mode == SidebarMode::Tree && !self.filter.has_query();
                base.child(
                    div()
                        .id(("chevron", index))
                        .ml(px(2.))
                        .size(px(14.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .when(tree_mode, |this| {
                            this.on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                cx.stop_propagation();
                                if let Some(index) = index_of(&this.rows, &chevron_path) {
                                    this.select_index(index, window, cx);
                                }
                                this.toggle_folder(&chevron_path, cx);
                            }))
                        })
                        .child(
                            Icon::new(if *expanded {
                                Glyph::ChevronDown
                            } else {
                                Glyph::ChevronRight
                            })
                            .size(px(11.))
                            .text_color(secondary),
                        ),
                )
                .child(
                    Icon::new(Glyph::FolderFilled)
                        .size(px(17.))
                        .ml(px(-1.))
                        .text_color(if on_accent {
                            tokens.on_accent
                        } else {
                            tokens.chrome.folder
                        }),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(primary)
                        .child(SharedString::from(name.clone())),
                )
            }
            RowKind::File { entry, show_parent } => {
                let change = &entry.change;
                let viewed = self.viewed.contains(&entry.change.path);
                let comments = self.comments.get(&entry.change.path).copied().unwrap_or(0);
                let icon_color = if on_accent {
                    tokens.on_accent
                } else {
                    tokens.tint(file_kind(&entry.name).tint())
                };
                let name_color = if viewed && !on_accent { secondary } else { primary };
                let tone = if on_accent {
                    BadgeTone::OnAccent
                } else if change.is_uncommitted {
                    BadgeTone::Uncommitted
                } else {
                    BadgeTone::Plain
                };
                base.child(div().w(px(14.)))
                    .child(
                        Icon::new(Glyph::for_file(&entry.name))
                            .size(px(13.))
                            .text_color(icon_color),
                    )
                    .child(
                        div()
                            .flex_shrink()
                            .min_w_0()
                            .truncate()
                            .text_color(name_color)
                            .when(viewed, |this| tag(this, || format!("sidebar:is-viewed:{path}")))
                            .child(SharedString::from(entry.name.clone())),
                    )
                    .when(*show_parent, |this| {
                        this.child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SMALL))
                                .text_color(tertiary)
                                .child(SharedString::from(entry.parent_path.clone())),
                        )
                    })
                    .when(!*show_parent, |this| this.child(div().flex_1()))
                    .when(comments > 0, |this| {
                        this.child(
                            tag(h_flex(), || format!("sidebar:comments:{path}:{comments}"))
                                .gap(px(2.))
                                .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SMALL))
                                .text_color(secondary)
                                .child(Icon::new(Glyph::MessageSquare).size(px(11.)))
                                .child(comments.to_string()),
                        )
                    })
                    .child(status_badge(
                        change.status.code(),
                        tone,
                        font(cx.theme().mono_font_family.clone()),
                        &tokens,
                    ))
            }
        };
        div()
            .w_full()
            .px(px(6.))
            .when(selected, |this| tag(this, || format!("sidebar:selected:{path}")))
            .child(element)
            .into_any_element()
    }
}

#[cfg(test)]
impl Sidebar {
    pub fn comments(&self) -> &HashMap<String, usize> {
        &self.comments
    }

    pub fn paths(&self) -> Vec<String> {
        self.rows.iter().map(|row| row.path.clone()).collect()
    }

    pub fn summary(&self) -> &ChangeSummary {
        &self.summary
    }

    pub fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use diffpilot_core::changes::FileStatus;
    use diffpilot_core::tree;
    use gpui::TestAppContext;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn change(path: &str, uncommitted: bool) -> FileChange {
        FileChange {
            path: path.into(),
            status: FileStatus::Modified,
            additions: 3,
            deletions: 1,
            is_uncommitted: uncommitted,
        }
    }

    fn loaded() -> LoadedChanges {
        let changes = vec![
            change("docs/archive.md", false),
            change("docs/Cache.md", true),
            change("src/main.rs", false),
            change("package.json", false),
        ];
        let root = tree::build(&changes);
        let files = tree::files(&root);
        LoadedChanges {
            changes,
            root,
            files,
            elapsed: Default::default(),
        }
    }

    fn setup(cx: &mut TestAppContext) {
        cx.update(|cx| {
            diffpilot_ui_kit::init(cx);
            bind_keys(cx);
        });
    }

    fn collect(sidebar: &Entity<Sidebar>, cx: &mut gpui::VisualTestContext) -> Rc<RefCell<Vec<SidebarEvent>>> {
        let events = Rc::new(RefCell::new(Vec::new()));
        let sink = events.clone();
        cx.update(|_, cx| {
            cx.subscribe(sidebar, move |_, event: &SidebarEvent, _| {
                sink.borrow_mut().push(event.clone())
            })
            .detach();
        });
        events
    }

    #[gpui::test]
    fn shows_rows_and_a_summary_once_changes_arrive(cx: &mut TestAppContext) {
        setup(cx);
        let (sidebar, cx) = cx.add_window_view(Sidebar::new);
        assert!(sidebar.read_with(cx, |sidebar, _| sidebar.is_loading));

        sidebar.update(cx, |sidebar, cx| sidebar.set_changes(loaded(), cx));
        cx.run_until_parked();

        sidebar.read_with(cx, |sidebar, _| {
            assert!(!sidebar.is_loading);
            assert_eq!(sidebar.summary().file_count, 4);
            assert_eq!(sidebar.summary().additions, 12);
            assert_eq!(
                sidebar.paths(),
                [
                    "docs",
                    "docs/archive.md",
                    "docs/Cache.md",
                    "src",
                    "src/main.rs",
                    "package.json"
                ]
            );
        });
    }

    #[gpui::test]
    fn viewed_files_raise_the_summary_and_hide_viewed_removes_them(cx: &mut TestAppContext) {
        setup(cx);
        let (sidebar, cx) = cx.add_window_view(Sidebar::new);
        sidebar.update(cx, |sidebar, cx| sidebar.set_changes(loaded(), cx));
        sidebar.update(cx, |sidebar, cx| {
            sidebar.set_viewed(HashSet::from(["docs/archive.md".to_string()]), cx);
        });
        sidebar.read_with(cx, |sidebar, _| assert_eq!(sidebar.summary().viewed, 1));

        sidebar.update(cx, |sidebar, cx| sidebar.toggle_hide_viewed(cx));
        sidebar.read_with(cx, |sidebar, _| {
            assert!(!sidebar.paths().contains(&"docs/archive.md".to_string()))
        });

        sidebar.update(cx, |sidebar, cx| sidebar.set_viewed(HashSet::new(), cx));
        sidebar.read_with(cx, |sidebar, _| {
            assert!(sidebar.paths().contains(&"docs/archive.md".to_string()));
            assert_eq!(sidebar.summary().viewed, 0);
        });
    }

    #[gpui::test]
    fn the_filter_field_narrows_the_tree_and_expands_folders(cx: &mut TestAppContext) {
        setup(cx);
        let (sidebar, cx) = cx.add_window_view(Sidebar::new);
        sidebar.update(cx, |sidebar, cx| sidebar.set_changes(loaded(), cx));
        sidebar.update(cx, |sidebar, cx| sidebar.toggle_folder("docs", cx));
        sidebar.read_with(cx, |sidebar, _| {
            assert!(!sidebar.paths().contains(&"docs/archive.md".to_string()))
        });

        let input = sidebar.read_with(cx, |sidebar, _| sidebar.filter_input.clone());
        input.update_in(cx, |input, window, cx| input.set_value("archive", window, cx));
        cx.run_until_parked();
        sidebar.read_with(cx, |sidebar, _| {
            assert_eq!(sidebar.paths(), ["docs", "docs/archive.md"])
        });

        input.update_in(cx, |input, window, cx| input.set_value("", window, cx));
        cx.run_until_parked();
        sidebar.read_with(cx, |sidebar, _| {
            assert!(!sidebar.paths().contains(&"docs/archive.md".to_string()));
            assert!(sidebar.paths().contains(&"src/main.rs".to_string()));
        });
    }

    #[gpui::test]
    fn list_mode_is_flat_and_only_uncommitted_filters(cx: &mut TestAppContext) {
        setup(cx);
        let (sidebar, cx) = cx.add_window_view(Sidebar::new);
        sidebar.update(cx, |sidebar, cx| sidebar.set_changes(loaded(), cx));
        sidebar.update(cx, |sidebar, cx| sidebar.set_mode(SidebarMode::List, cx));
        sidebar.read_with(cx, |sidebar, _| assert_eq!(sidebar.paths().len(), 4));
        sidebar.update(cx, |sidebar, cx| sidebar.toggle_uncommitted(cx));
        sidebar.read_with(cx, |sidebar, _| assert_eq!(sidebar.paths(), ["docs/Cache.md"]));
    }

    #[gpui::test]
    fn selecting_a_file_reveals_it_but_selecting_a_folder_does_not(cx: &mut TestAppContext) {
        setup(cx);
        let (sidebar, cx) = cx.add_window_view(Sidebar::new);
        sidebar.update(cx, |sidebar, cx| sidebar.set_changes(loaded(), cx));
        let events = collect(&sidebar, cx);

        sidebar.update_in(cx, |sidebar, window, cx| sidebar.select_index(0, window, cx));
        assert!(events.borrow().is_empty());
        sidebar.update_in(cx, |sidebar, window, cx| sidebar.select_index(1, window, cx));
        assert_eq!(*events.borrow(), [SidebarEvent::Select("docs/archive.md".into())]);
        sidebar.read_with(cx, |sidebar, _| assert_eq!(sidebar.selected(), Some("docs/archive.md")));
    }

    #[gpui::test]
    fn arrow_keys_move_the_selection_and_collapse_folders(cx: &mut TestAppContext) {
        setup(cx);
        let (sidebar, cx) = cx.add_window_view(Sidebar::new);
        sidebar.update(cx, |sidebar, cx| sidebar.set_changes(loaded(), cx));
        let events = collect(&sidebar, cx);
        let focus = sidebar.read_with(cx, |sidebar, _| sidebar.focus.clone());
        cx.update(|window, _| window.focus(&focus));
        cx.run_until_parked();

        cx.simulate_keystrokes("down");
        sidebar.read_with(cx, |sidebar, _| assert_eq!(sidebar.selected(), Some("docs")));
        cx.simulate_keystrokes("left");
        sidebar.read_with(cx, |sidebar, _| {
            assert!(!sidebar.paths().contains(&"docs/archive.md".to_string()))
        });
        cx.simulate_keystrokes("right down");
        sidebar.read_with(cx, |sidebar, _| assert_eq!(sidebar.selected(), Some("docs/archive.md")));
        assert_eq!(*events.borrow(), [SidebarEvent::Select("docs/archive.md".into())]);
        cx.simulate_keystrokes("up");
        sidebar.read_with(cx, |sidebar, _| assert_eq!(sidebar.selected(), Some("docs")));
    }

    #[gpui::test]
    fn an_identical_reload_keeps_the_rows(cx: &mut TestAppContext) {
        setup(cx);
        let (sidebar, cx) = cx.add_window_view(Sidebar::new);
        sidebar.update(cx, |sidebar, cx| sidebar.set_changes(loaded(), cx));
        sidebar.update(cx, |sidebar, cx| sidebar.toggle_folder("src", cx));
        sidebar.update(cx, |sidebar, cx| sidebar.set_changes(loaded(), cx));
        sidebar.read_with(cx, |sidebar, _| {
            assert!(!sidebar.paths().contains(&"src/main.rs".to_string()))
        });
    }
}
