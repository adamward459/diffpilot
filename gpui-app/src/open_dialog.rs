use crate::comparison_control::{Select, menu_sections};
use crate::comparison_menu::{base_glyph, base_sections, base_title, branch_glyph, branch_sections, branch_title};
use crate::folder_menu::{FolderSnapshot, folder_name, snapshot};
use crate::folder_picker::pick_folders;
use crate::folder_prs::{FolderPrs, PrList};
use crate::linked_folders::{LinkedEvent, LinkedFolders, folder_chip};
use crate::path_hints::{ACTIVE, HintAccept, HintNext, HintPrev, PathHints};
use crate::remote::rows::{Tone, outcome_tone};
use crate::repo_view::tilde_path;
use diffpilot_core::comparison::Comparison;
use diffpilot_core::folder_status::FolderStatus;
use diffpilot_core::forge::{self, ChangeRequest, Outcome, Review};
use diffpilot_core::guide::linked;
use diffpilot_core::settings::Settings;
use diffpilot_ui_kit::components::menu::menu_button;
use diffpilot_ui_kit::components::{Button, Look, Presence, dialog, filter_field, glyph_icon, spinner, tag};
use diffpilot_ui_kit::{Glyph, Theme, text_px, text_size};
use gpui::prelude::*;
use gpui::{
    AnyElement, App, Context, Entity, EventEmitter, FocusHandle, FontWeight, KeyBinding, ScrollHandle, SharedString,
    Subscription, Task, Window, actions, div, px,
};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{Sizable, h_flex, v_flex};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

const WIDTH: f32 = 800.;
const HEIGHT: f32 = 580.;
const LIST_WIDTH: f32 = 250.;
const CONTEXT: &str = "OpenFolderDialog";

actions!(diffpilot_open, [ConfirmOpen, CancelOpen, RecentNext, RecentPrev]);

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("enter", ConfirmOpen, Some("OpenFolderDialog && !Input")),
        KeyBinding::new("escape", CancelOpen, Some(CONTEXT)),
        KeyBinding::new("down", RecentNext, Some("OpenFolderDialog && !Input")),
        KeyBinding::new("up", RecentPrev, Some("OpenFolderDialog && !Input")),
        KeyBinding::new("down", RecentNext, Some("SheetSearch > Input")),
        KeyBinding::new("up", RecentPrev, Some("SheetSearch > Input")),
    ]);
}

#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    Comparison(Comparison),
    PullRequest(ChangeRequest),
}

#[derive(Debug, Clone, PartialEq)]
pub struct OpenDraft {
    pub folder: PathBuf,
    pub linked: Vec<PathBuf>,
    pub target: Target,
}

#[derive(Debug, Clone, PartialEq)]
pub enum OpenEvent {
    Open {
        folder: PathBuf,
        linked: Vec<PathBuf>,
        comparison: Option<Comparison>,
    },
    Cancel,
}

pub struct OpenFolderDialog {
    settings: Arc<Settings>,
    prs: Entity<FolderPrs>,
    folders: Vec<PathBuf>,
    snapshots: HashMap<PathBuf, FolderSnapshot>,
    selected: Option<PathBuf>,
    links: HashMap<PathBuf, Vec<PathBuf>>,
    draft: Option<OpenDraft>,
    fallback: Option<Comparison>,
    start_comparison: Option<(PathBuf, Comparison)>,
    search: Entity<InputState>,
    hints: Entity<PathHints>,
    list_scroll: ScrollHandle,
    editor: Entity<LinkedFolders>,
    editing_links: bool,
    error: Option<SharedString>,
    bad_path: Option<String>,
    opening: bool,
    presence: Presence,
    focus: FocusHandle,
    tasks: Vec<Task<()>>,
    open_task: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<OpenEvent> for OpenFolderDialog {}

impl OpenFolderDialog {
    pub fn new(
        settings: Arc<Settings>,
        start: Option<(PathBuf, Comparison)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let prs = FolderPrs::shared(cx);
        let mut folders = settings.folders();
        if let Some((folder, _)) = &start
            && !folders.contains(folder)
        {
            folders.insert(0, folder.clone());
        }
        prs.update(cx, |prs, cx| prs.refresh(&folders, cx));
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search folders, paths, PR titles, or #123"));
        let hints = cx.new(|_| PathHints::default());
        let editor = cx.new(|cx| LinkedFolders::new(PathBuf::new(), Vec::new(), window, cx));
        let subscriptions = vec![
            cx.observe(&prs, |_, _, cx| cx.notify()),
            cx.observe(&hints, |_, _, cx| cx.notify()),
            cx.subscribe_in(&search, window, |this, _, event: &InputEvent, window, cx| match event {
                InputEvent::Change => this.search_changed(cx),
                InputEvent::PressEnter { .. } => this.submit_search(window, cx),
                _ => {}
            }),
            cx.subscribe(&editor, |this, _, LinkedEvent::Changed(linked), cx| {
                if let Some(folder) = this.selected.clone() {
                    this.links.insert(folder, linked.clone());
                }
                if let Some(draft) = &mut this.draft {
                    draft.linked = linked.clone();
                }
                cx.notify();
            }),
        ];
        let focus = cx.focus_handle();
        search.update(cx, |search, cx| search.focus(window, cx));
        let mut presence = Presence::new(false);
        presence.set_open(true, cx, |this: &mut Self| &mut this.presence);
        let mut this = Self {
            settings,
            prs,
            folders: folders.clone(),
            snapshots: HashMap::new(),
            selected: None,
            links: HashMap::new(),
            draft: None,
            fallback: None,
            start_comparison: start,
            search,
            hints,
            list_scroll: ScrollHandle::new(),
            editor,
            editing_links: false,
            error: None,
            bad_path: None,
            opening: false,
            presence,
            focus,
            tasks: Vec::new(),
            open_task: None,
            _subscriptions: subscriptions,
        };
        for folder in &folders {
            this.load(folder.clone(), cx);
        }
        let first = match &this.start_comparison {
            Some((folder, _)) => Some(folder.clone()),
            None => folders.into_iter().next(),
        };
        if let Some(first) = first {
            this.select(first, cx);
        }
        this
    }

    pub fn is_mounted(&self) -> bool {
        self.presence.is_mounted()
    }

    pub fn is_loaded(&self) -> bool {
        self.draft.is_some()
    }

    pub fn is_open(&self) -> bool {
        self.presence.is_open()
    }

    pub fn focus_search(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.search.update(cx, |search, cx| search.focus(window, cx));
    }

    fn load(&mut self, folder: PathBuf, cx: &mut Context<Self>) {
        let settings = self.settings.clone();
        let path = folder.clone();
        let work = cx.background_spawn(async move { snapshot(&path, &settings) });
        self.tasks.push(cx.spawn(async move |this, cx| {
            let loaded = work.await;
            this.update(cx, |this, cx| {
                this.snapshots.insert(folder, loaded);
                this.build_draft();
                cx.notify();
            })
            .ok();
        }));
    }

    fn build_draft(&mut self) {
        if self.draft.is_some() {
            return;
        }
        let Some(folder) = self.selected.clone() else { return };
        let Some(loaded) = self.snapshots.get(&folder) else {
            return;
        };
        let comparison = match self.start_comparison.take() {
            Some((start, comparison)) if start == folder => comparison,
            _ => loaded.comparison.clone(),
        };
        self.draft = Some(OpenDraft {
            linked: self.linked_for(&folder),
            target: Target::Comparison(comparison),
            folder,
        });
    }

    fn linked_for(&self, folder: &Path) -> Vec<PathBuf> {
        self.links
            .get(folder)
            .cloned()
            .unwrap_or_else(|| self.settings.linked_folders(folder))
    }

    fn select(&mut self, folder: PathBuf, cx: &mut Context<Self>) {
        let linked = self.linked_for(&folder);
        self.editor
            .update(cx, |editor, cx| editor.set_repo(folder.clone(), linked, cx));
        self.selected = Some(folder.clone());
        self.draft = None;
        self.fallback = None;
        self.error = None;
        self.editing_links = false;
        if !self.snapshots.contains_key(&folder) {
            self.load(folder, cx);
        }
        self.build_draft();
        cx.notify();
    }

    fn add_folder(&mut self, folder: PathBuf, cx: &mut Context<Self>) {
        let folder = folder.canonicalize().unwrap_or(folder);
        if !self.folders.contains(&folder) {
            self.folders.insert(0, folder.clone());
            let added = [folder.clone()];
            self.prs.update(cx, |prs, cx| prs.refresh(&added, cx));
        }
        self.select(folder, cx);
    }

    fn query(&self, cx: &App) -> String {
        self.search.read(cx).value().trim().to_string()
    }

    fn search_changed(&mut self, cx: &mut Context<Self>) {
        self.error = None;
        let query = self.query(cx);
        self.hints.update(cx, |hints, cx| hints.set_text(&query, None, cx));
        let visible = self.visible_folders(cx);
        if self
            .selected
            .as_ref()
            .is_none_or(|selected| !visible.contains(selected))
            && let Some(first) = visible.into_iter().next()
        {
            self.select(first, cx);
        }
        cx.notify();
    }

    fn move_selection(&mut self, forward: bool, cx: &mut Context<Self>) {
        let visible = self.visible_folders(cx);
        let at = self
            .selected
            .as_ref()
            .and_then(|selected| visible.iter().position(|folder| folder == selected));
        let next = match (at, forward) {
            (Some(at), true) => (at + 1).min(visible.len().saturating_sub(1)),
            (Some(at), false) => at.saturating_sub(1),
            (None, _) => 0,
        };
        if let Some(folder) = visible.get(next).cloned() {
            self.list_scroll.scroll_to_item(next);
            self.select(folder, cx);
        }
    }

    fn step_hints(&mut self, forward: bool, cx: &mut Context<Self>) {
        self.hints.update(cx, |hints, cx| hints.step(forward, cx));
    }

    fn accept_hint(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = self.hints.read(cx).pick() {
            self.search.update(cx, |search, cx| search.set_value(text, window, cx));
        }
    }

    fn submit_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = self.hints.read(cx).highlighted() {
            self.search.update(cx, |search, cx| search.set_value(text, window, cx));
        }
        let query = self.query(cx);
        if self.search.read(cx).value().len() != query.len() {
            let clean = query.clone();
            self.search.update(cx, |search, cx| search.set_value(clean, window, cx));
        }
        if !linked::is_path(&query) {
            return self.open(cx);
        }
        let path = linked::expand_tilde(&query);
        if path.is_dir() {
            self.add_folder(path, cx);
            self.open(cx);
        } else {
            self.bad_path = Some(query);
            cx.notify();
        }
    }

    fn requests(&self, folder: &Path, cx: &App) -> Vec<ChangeRequest> {
        self.prs
            .read(cx)
            .list(folder)
            .map(|list| list.requests().to_vec())
            .unwrap_or_default()
    }

    fn visible_folders(&self, cx: &App) -> Vec<PathBuf> {
        let query = self.query(cx);
        self.folders
            .iter()
            .filter(|folder| folder_matches(&query, &folder_name(folder), &self.requests(folder, cx)))
            .cloned()
            .collect()
    }

    fn visible_requests(&self, cx: &App) -> Vec<ChangeRequest> {
        let Some(folder) = &self.selected else {
            return Vec::new();
        };
        matching_requests(&self.query(cx), &folder_name(folder), self.requests(folder, cx))
    }

    fn basis(&self) -> Option<Comparison> {
        let draft = self.draft.as_ref()?;
        match &draft.target {
            Target::Comparison(comparison) => Some(comparison.clone()),
            Target::PullRequest(_) => self
                .fallback
                .clone()
                .or_else(|| Some(self.snapshots.get(&draft.folder)?.comparison.clone())),
        }
    }

    fn set_comparison(&mut self, comparison: Comparison, cx: &mut Context<Self>) {
        if let Some(draft) = &mut self.draft {
            draft.target = Target::Comparison(comparison);
            self.fallback = None;
            self.error = None;
            cx.notify();
        }
    }

    fn pick_pull_request(&mut self, change: ChangeRequest, cx: &mut Context<Self>) {
        let basis = self.basis();
        if let Some(draft) = &mut self.draft {
            if matches!(draft.target, Target::Comparison(_)) {
                self.fallback = basis;
            }
            draft.target = Target::PullRequest(change);
            self.error = None;
            cx.notify();
        }
    }

    fn clear_pull_request(&mut self, cx: &mut Context<Self>) {
        if let Some(basis) = self.basis()
            && let Some(draft) = &mut self.draft
        {
            draft.target = Target::Comparison(basis);
            self.fallback = None;
            cx.notify();
        }
    }

    fn open(&mut self, cx: &mut Context<Self>) {
        if self.opening {
            return;
        }
        let Some(folder) = self.selected.clone() else { return };
        let linked = match &self.draft {
            Some(draft) => draft.linked.clone(),
            None => self.settings.linked_folders(&folder),
        };
        let comparison = match self.draft.as_ref().map(|draft| draft.target.clone()) {
            Some(Target::PullRequest(change)) => return self.open_pull_request(folder, linked, change, cx),
            Some(Target::Comparison(comparison)) => Some(comparison),
            None => None,
        };
        cx.emit(OpenEvent::Open {
            folder,
            linked,
            comparison,
        });
    }

    fn open_pull_request(
        &mut self,
        folder: PathBuf,
        linked: Vec<PathBuf>,
        change: ChangeRequest,
        cx: &mut Context<Self>,
    ) {
        let Some(link) = self.prs.read(cx).connection(&folder) else {
            self.error = Some("This folder has no remote to fetch the pull request from.".into());
            cx.notify();
            return;
        };
        self.opening = true;
        self.error = None;
        cx.notify();
        let repo = folder.clone();
        let work = cx.background_spawn(async move {
            forge::review_without_checkout(&repo, &link.remote, link.forge.as_ref(), &change)
                .map_err(|error| error.to_string())
        });
        self.open_task = Some(cx.spawn(async move |this, cx| {
            let result = work.await;
            this.update(cx, |this, cx| {
                this.opening = false;
                if this.presence.is_open() {
                    match result {
                        Ok(review) => cx.emit(OpenEvent::Open {
                            folder,
                            linked,
                            comparison: Some(review.comparison),
                        }),
                        Err(message) => this.error = Some(message.into()),
                    }
                }
                cx.notify();
            })
            .ok();
        }));
    }

    fn cancel(&mut self, cx: &mut Context<Self>) {
        if !self.presence.is_open() {
            return;
        }
        self.presence.set_open(false, cx, |this| &mut this.presence);
        cx.emit(OpenEvent::Cancel);
    }

    fn browse(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = pick_folders(false, cx);
        cx.spawn_in(window, async move |this, cx| {
            let Some(folder) = picked.await.and_then(|paths| paths.into_iter().next()) else {
                return;
            };
            this.update(cx, |this, cx| this.add_folder(folder, cx)).ok();
        })
        .detach();
    }

    fn toggle_links(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editing_links = !self.editing_links;
        if self.editing_links {
            self.editor.update(cx, |editor, cx| editor.focus_input(window, cx));
        }
        cx.notify();
    }
}

pub fn matches_request(query: &str, change: &ChangeRequest) -> bool {
    let query = query.trim().to_lowercase();
    if let Some(digits) = query.strip_prefix('#') {
        return change.number.to_string().starts_with(digits);
    }
    query.is_empty() || change.title.to_lowercase().contains(&query) || change.number.to_string() == query
}

pub fn folder_matches(query: &str, name: &str, requests: &[ChangeRequest]) -> bool {
    let query = query.trim();
    linked::is_path(query)
        || query.is_empty()
        || name.to_lowercase().contains(&query.to_lowercase())
        || requests.iter().any(|change| matches_request(query, change))
}

pub fn matching_requests(query: &str, name: &str, requests: Vec<ChangeRequest>) -> Vec<ChangeRequest> {
    let query = query.trim();
    if linked::is_path(query) || name.to_lowercase().contains(&query.to_lowercase()) {
        return requests;
    }
    requests
        .into_iter()
        .filter(|change| matches_request(query, change))
        .collect()
}

pub fn folder_detail(status: Option<&FolderStatus>) -> String {
    let Some(status) = status else { return String::new() };
    let branch = status.branch.as_deref().unwrap_or("detached HEAD");
    match status.files {
        0 => format!("{branch} · no changes"),
        1 => format!("{branch} · 1 file changed"),
        files => format!("{branch} · {files} files changed"),
    }
}

pub fn branches_text(change: &ChangeRequest) -> String {
    if change.from_this_repo {
        format!("{} → {}", change.head_ref, change.base_ref)
    } else {
        format!(
            "{}:{} → {} · from a fork",
            change.author, change.head_ref, change.base_ref
        )
    }
}

pub fn review_pill(review: Review) -> Option<(&'static str, Tone)> {
    match review {
        Review::Approved => Some(("Approved", Tone::Passed)),
        Review::ChangesRequested => Some(("Changes requested", Tone::Failed)),
        Review::Requested => Some(("Review required", Tone::Running)),
        Review::None => None,
    }
}

pub fn checks_pill(checks: Option<Outcome>) -> Option<(&'static str, Tone)> {
    let outcome = checks?;
    let text = match outcome {
        Outcome::Passed => "Checks passed",
        Outcome::Failed => "Checks failed",
        Outcome::Running | Outcome::Queued => "Checks running",
        Outcome::ActionRequired => "Checks need action",
        Outcome::Skipped | Outcome::Cancelled => return None,
    };
    Some((text, outcome_tone(outcome)))
}

fn pill(text: &'static str, tone: Tone, tokens: &Theme) -> impl IntoElement {
    let color = tone.color(tokens);
    div()
        .flex_none()
        .h(px(18.))
        .px(px(7.))
        .flex()
        .items_center()
        .rounded_full()
        .bg(color.opacity(0.14))
        .text_size(text_px(text_size::SMALL))
        .text_color(color)
        .whitespace_nowrap()
        .child(text)
}

fn heading(text: impl Into<SharedString>, tokens: &Theme) -> gpui::Div {
    div()
        .text_size(text_px(text_size::SMALL))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(tokens.ui.muted)
        .child(text.into())
}

impl OpenFolderDialog {
    fn recent_row(&self, index: usize, folder: &PathBuf, tokens: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let name = folder_name(folder);
        let selected = self.selected.as_ref() == Some(folder);
        let detail = folder_detail(self.snapshots.get(folder).and_then(|loaded| loaded.status.as_ref()));
        let badge = self.prs.read(cx).list(folder).map(PrList::badge);
        let badge = match badge {
            Some(Some(text)) => tag(
                div()
                    .flex_none()
                    .px(px(7.))
                    .h(px(18.))
                    .flex()
                    .items_center()
                    .rounded_full()
                    .bg(tokens.chrome.badge_fill)
                    .text_size(text_px(text_size::SMALL))
                    .text_color(tokens.chrome.secondary)
                    .whitespace_nowrap()
                    .child(SharedString::from(text.clone())),
                {
                    let name = name.clone();
                    move || format!("open:badge:{name}:{text}")
                },
            )
            .into_any_element(),
            _ => tag(
                div().child(spinner(("open-badge-spinner", index), 12., tokens.ui.muted)),
                {
                    let name = name.clone();
                    move || format!("open:badge-loading:{name}")
                },
            )
            .into_any_element(),
        };
        let target = folder.clone();
        tag(
            h_flex()
                .id(("open-recent", index))
                .w_full()
                .gap(px(8.))
                .px(px(10.))
                .py(px(7.))
                .rounded(px(7.))
                .cursor_pointer()
                .when(selected, |row| row.bg(tokens.press))
                .when(!selected, |row| row.hover(|row| row.bg(tokens.hover)))
                .on_click(cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                    window.focus(&this.focus);
                    this.select(target.clone(), cx);
                    if event.click_count() == 2 {
                        this.open(cx);
                    }
                }))
                .child(glyph_icon(Glyph::FolderFilled, 16., tokens.chrome.folder))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .truncate()
                                .text_size(text_px(text_size::BODY))
                                .font_weight(FontWeight::MEDIUM)
                                .child(SharedString::from(name.clone())),
                        )
                        .child(
                            div()
                                .truncate()
                                .text_size(text_px(text_size::SMALL))
                                .text_color(tokens.ui.muted)
                                .child(SharedString::from(detail)),
                        ),
                )
                .child(badge),
            move || format!("open:folder:{name}"),
        )
        .into_any_element()
    }

    fn compare_row(&self, tokens: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let loaded = self
            .draft
            .as_ref()
            .and_then(|draft| Some((draft, self.snapshots.get(&draft.folder)?)));
        let (Some((draft, loaded)), Some(basis)) = (loaded, self.basis()) else {
            return tag(
                div()
                    .text_size(text_px(text_size::BODY))
                    .text_color(tokens.ui.muted)
                    .child("Loading branches…"),
                || "open:compare-loading",
            )
            .into_any_element();
        };
        let weak = cx.entity().downgrade();
        let select: Select = Rc::new(move |next, _, cx| {
            weak.update(cx, |this, cx| this.set_comparison(next, cx)).ok();
        });
        let (branches, bases) = (
            branch_sections(&loaded.refs, &basis),
            base_sections(&loaded.refs, &basis),
        );
        let (branch_select, base_select) = (select.clone(), select);
        let (branch_label, base_label, branch_icon, base_icon) = match &draft.target {
            Target::Comparison(comparison) => (
                branch_title(&loaded.refs, comparison),
                base_title(comparison),
                branch_glyph(comparison),
                base_glyph(comparison),
            ),
            Target::PullRequest(change) => (
                change.head_ref.clone(),
                change.base_ref.clone(),
                Glyph::GitPullRequest,
                Glyph::ArrowLeftRight,
            ),
        };
        let trigger = |id: &'static str, glyph: Glyph, label: String| {
            let text = label.clone();
            tag(
                Button::new(id).regular().icon(glyph, 14.).caret(true).child(
                    tag(div().font_weight(FontWeight::MEDIUM), move || {
                        format!("open:{id}-text:{label}")
                    })
                    .child(SharedString::from(text)),
                ),
                move || id,
            )
        };
        let pull_request = match &draft.target {
            Target::PullRequest(change) => Some(change.number),
            Target::Comparison(_) => None,
        };
        h_flex()
            .gap(px(8.))
            .items_center()
            .child(
                menu_button(
                    "open-branch",
                    trigger("open-branch", branch_icon, branch_label),
                    move |_, _| menu_sections(&branches, &branch_select),
                )
                .min_width(230.)
                .arrow(),
            )
            .child(
                div()
                    .text_size(text_px(text_size::BODY))
                    .text_color(tokens.chrome.secondary)
                    .child("compared to"),
            )
            .child(
                menu_button("open-base", trigger("open-base", base_icon, base_label), move |_, _| {
                    menu_sections(&bases, &base_select)
                })
                .min_width(230.)
                .arrow(),
            )
            .children(pull_request.map(|number| {
                tag(
                    h_flex()
                        .h(px(24.))
                        .pl(px(9.))
                        .pr(px(3.))
                        .gap(px(2.))
                        .items_center()
                        .rounded_full()
                        .bg(tokens.ui.cap)
                        .text_size(text_px(text_size::SECONDARY))
                        .text_color(tokens.ui.text2)
                        .whitespace_nowrap()
                        .child(format!("From PR #{number}"))
                        .child(tag(
                            Button::new("open-chip-clear")
                                .look(Look::Ghost)
                                .icon(Glyph::X, 10.)
                                .tooltip("Back to the folder's comparison")
                                .on_click(cx.listener(|this, _, _, cx| this.clear_pull_request(cx))),
                            || "open:chip-clear",
                        )),
                    move || format!("open:chip:{number}"),
                )
            }))
            .into_any_element()
    }

    fn request_row(&self, index: usize, change: &ChangeRequest, tokens: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let selected = matches!(
            self.draft.as_ref().map(|draft| &draft.target),
            Some(Target::PullRequest(picked)) if picked.number == change.number
        );
        let number = change.number;
        let target = change.clone();
        tag(
            v_flex()
                .id(("open-request", index))
                .w_full()
                .gap(px(3.))
                .px(px(10.))
                .py(px(7.))
                .rounded(px(7.))
                .cursor_pointer()
                .when(selected, |row| row.bg(tokens.press))
                .when(!selected, |row| row.hover(|row| row.bg(tokens.hover)))
                .on_click(cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                    window.focus(&this.focus);
                    this.pick_pull_request(target.clone(), cx);
                    if event.click_count() == 2 {
                        this.open(cx);
                    }
                }))
                .child(
                    div()
                        .truncate()
                        .text_size(text_px(text_size::BODY))
                        .font_weight(FontWeight::MEDIUM)
                        .child(SharedString::from(format!("#{} {}", change.number, change.title))),
                )
                .child(
                    h_flex()
                        .gap(px(6.))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .font_family("Menlo")
                                .text_size(text_px(text_size::SMALL))
                                .text_color(tokens.ui.muted)
                                .child(SharedString::from(branches_text(change))),
                        )
                        .children(review_pill(change.review).map(|(text, tone)| pill(text, tone, tokens)))
                        .children(checks_pill(change.checks).map(|(text, tone)| pill(text, tone, tokens))),
                ),
            move || format!("open:pr:{number}"),
        )
        .into_any_element()
    }

    fn requests_section(&self, tokens: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let Some(folder) = &self.selected else {
            return div().flex_1().into_any_element();
        };
        let list = self.prs.read(cx).list(folder).cloned();
        let visible = self.visible_requests(cx);
        let note = |text: String, tag_name: &'static str| {
            tag(
                div()
                    .py(px(8.))
                    .text_size(text_px(text_size::BODY))
                    .text_color(tokens.ui.muted)
                    .child(SharedString::from(text)),
                move || tag_name,
            )
            .into_any_element()
        };
        let body = match &list {
            None | Some(PrList::Loading(None)) => note("Loading pull requests…".into(), "open:prs-loading"),
            Some(PrList::NoRemote) => note(
                "This folder has no GitHub remote, or you are not signed in.".into(),
                "open:prs-none",
            ),
            Some(PrList::Failed(message)) => {
                note(format!("Could not load pull requests. {message}"), "open:prs-failed")
            }
            Some(PrList::Loaded(_) | PrList::Loading(Some(_))) if visible.is_empty() => {
                note("No open pull requests.".into(), "open:prs-empty")
            }
            Some(_) => v_flex()
                .id("open-requests")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .gap(px(2.))
                .children(
                    visible
                        .iter()
                        .enumerate()
                        .map(|(index, change)| self.request_row(index, change, tokens, cx)),
                )
                .into_any_element(),
        };
        let count = visible.len();
        v_flex()
            .flex_1()
            .min_h_0()
            .gap(px(6.))
            .child(tag(heading(format!("PULL REQUESTS · {count}"), tokens), move || {
                format!("open:prs-count:{count}")
            }))
            .child(body)
            .into_any_element()
    }

    fn linked_row(&self, tokens: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let linked = self
            .draft
            .as_ref()
            .map(|draft| draft.linked.clone())
            .unwrap_or_default();
        let chips = linked.iter().map(|folder| {
            let name = folder_name(folder);
            tag(folder_chip(tokens, Glyph::Link, &name), move || {
                format!("open:linked-chip:{name}")
            })
        });
        v_flex()
            .gap(px(8.))
            .child(
                h_flex()
                    .gap(px(6.))
                    .items_center()
                    .text_size(text_px(text_size::SECONDARY))
                    .text_color(tokens.ui.muted)
                    .child("Linked:")
                    .children(chips)
                    .when(linked.is_empty(), |row| row.child("none"))
                    .child(tag(
                        Button::new("open-linked-edit")
                            .look(Look::Ghost)
                            .label("Edit…")
                            .on_click(cx.listener(|this, _, window, cx| this.toggle_links(window, cx))),
                        || "open:linked-edit",
                    )),
            )
            .when(self.editing_links, |this| this.child(self.editor.clone()))
            .into_any_element()
    }
}

impl Render for OpenFolderDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Theme::of(cx);
        let visible = self.visible_folders(cx);
        let rows: Vec<AnyElement> = visible
            .iter()
            .enumerate()
            .map(|(index, folder)| self.recent_row(index, folder, &tokens, cx))
            .collect();
        let selected = self.selected.clone();
        let for_request = matches!(
            self.draft.as_ref().map(|draft| &draft.target),
            Some(Target::PullRequest(_))
        );
        let right = match &selected {
            Some(folder) => v_flex()
                .flex_1()
                .min_w_0()
                .gap(px(14.))
                .child(
                    v_flex()
                        .gap(px(2.))
                        .child(tag(
                            div()
                                .text_size(text_px(text_size::TITLE))
                                .font_weight(FontWeight::SEMIBOLD)
                                .truncate()
                                .child(SharedString::from(folder_name(folder))),
                            {
                                let name = folder_name(folder);
                                move || format!("open:selected:{name}")
                            },
                        ))
                        .child(
                            div()
                                .truncate()
                                .font_family("Menlo")
                                .text_size(text_px(text_size::SMALL))
                                .text_color(tokens.ui.muted)
                                .child(SharedString::from(tilde_path(folder))),
                        ),
                )
                .child(
                    v_flex()
                        .gap(px(6.))
                        .child(heading("COMPARE", &tokens))
                        .child(self.compare_row(&tokens, cx)),
                )
                .child(self.requests_section(&tokens, cx))
                .child(self.linked_row(&tokens, cx))
                .into_any_element(),
            None => v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .text_size(text_px(text_size::BODY))
                .text_color(tokens.ui.muted)
                .child(tag(div(), || "open:none").child("No folder matches."))
                .into_any_element(),
        };
        let error = match &self.bad_path {
            Some(bad) if *bad == self.query(cx) => Some(SharedString::from("That path is not a folder.")),
            _ => self.error.clone(),
        };
        let hint = match (error, for_request) {
            (Some(error), _) => tag(div().text_color(tokens.amber), || "open:error").child(error),
            (None, true) => div().child("Opens without a checkout. Your working tree stays as it is."),
            (None, false) => div(),
        };
        let hinting = self.hints.read(cx).is_active();
        let hint_list = hinting.then(|| {
            div()
                .absolute()
                .top(px(50.))
                .left(px(16.))
                .w(px(WIDTH - 32. - 100.))
                .occlude()
                .child(self.hints.read(cx).list(&tokens))
        });
        let content = v_flex()
            .id("open-folder-content")
            .relative()
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .size_full()
            .p(px(16.))
            .gap(px(12.))
            .on_action(cx.listener(|this, _: &ConfirmOpen, _, cx| this.open(cx)))
            .on_action(cx.listener(|this, _: &CancelOpen, _, cx| this.cancel(cx)))
            .on_action(cx.listener(|this, _: &RecentNext, _, cx| this.move_selection(true, cx)))
            .on_action(cx.listener(|this, _: &RecentPrev, _, cx| this.move_selection(false, cx)))
            .child(
                h_flex()
                    .gap(px(8.))
                    .child(tag(
                        filter_field(
                            &tokens,
                            Input::new(&self.search)
                                .appearance(false)
                                .bordered(false)
                                .focus_bordered(false)
                                .small(),
                        )
                        .key_context(if hinting { ACTIVE } else { "SheetSearch" })
                        .on_action(cx.listener(|this, _: &HintNext, _, cx| this.step_hints(true, cx)))
                        .on_action(cx.listener(|this, _: &HintPrev, _, cx| this.step_hints(false, cx)))
                        .on_action(cx.listener(|this, _: &HintAccept, window, cx| this.accept_hint(window, cx)))
                        .on_action(cx.listener(|this, _: &RecentNext, _, cx| this.move_selection(true, cx)))
                        .on_action(cx.listener(|this, _: &RecentPrev, _, cx| this.move_selection(false, cx))),
                        || "open:search",
                    ))
                    .child(tag(
                        Button::new("open-browse")
                            .regular()
                            .icon(Glyph::Folder, 14.)
                            .label("Browse…")
                            .on_click(cx.listener(|this, _, window, cx| this.browse(window, cx))),
                        || "open:browse",
                    )),
            )
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .gap(px(16.))
                    .child(
                        v_flex()
                            .w(px(LIST_WIDTH))
                            .flex_none()
                            .h_full()
                            .gap(px(6.))
                            .child(heading("RECENT", &tokens))
                            .child(
                                v_flex()
                                    .id("open-recent-list")
                                    .track_scroll(&self.list_scroll)
                                    .flex_1()
                                    .min_h_0()
                                    .overflow_y_scroll()
                                    .gap(px(2.))
                                    .children(rows),
                            ),
                    )
                    .child(div().w(px(1.)).h_full().flex_none().bg(tokens.ui.line))
                    .child(right),
            )
            .child(
                h_flex()
                    .gap(px(8.))
                    .items_center()
                    .child(
                        tag(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(text_px(text_size::SECONDARY))
                                .text_color(tokens.ui.text2),
                            || "open:hint",
                        )
                        .child(hint),
                    )
                    .child(tag(
                        Button::new("open-folder-cancel")
                            .label("Cancel")
                            .on_click(cx.listener(|this, _, _, cx| this.cancel(cx))),
                        || "open-folder-cancel",
                    ))
                    .child(tag(
                        Button::new("open-folder-confirm")
                            .look(Look::Primary)
                            .label(if self.opening { "Opening…" } else { "Open" })
                            .disabled(self.selected.is_none() || self.opening)
                            .on_click(cx.listener(|this, _, _, cx| this.open(cx))),
                        || "open-folder-confirm",
                    )),
            )
            .children(hint_list);
        dialog("open-folder", &self.presence)
            .tags("open-folder:backdrop", "open-folder:dialog")
            .size(WIDTH, HEIGHT)
            .on_backdrop_click(cx.listener(|this, _, _, cx| this.cancel(cx)))
            .child(content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(number: u64, title: &str) -> ChangeRequest {
        ChangeRequest {
            number,
            title: title.into(),
            url: String::new(),
            author: "mira".into(),
            head_ref: "feature/x".into(),
            base_ref: "main".into(),
            head_sha: String::new(),
            from_this_repo: true,
            review: Review::None,
            checks: None,
        }
    }

    #[test]
    fn a_request_matches_by_title_or_number() {
        let change = request(479, "Bulk select in the inbox");
        assert!(matches_request("bulk", &change));
        assert!(matches_request("#479", &change));
        assert!(matches_request("#47", &change));
        assert!(matches_request("479", &change));
        assert!(!matches_request("#48", &change));
        assert!(!matches_request("undo", &change));
        assert!(matches_request("", &change));
    }

    #[test]
    fn a_folder_matches_by_name_or_by_one_of_its_requests() {
        let requests = [request(1, "Undo keeps a timer")];
        assert!(folder_matches("", "alpha", &[]));
        assert!(folder_matches("alp", "alpha", &[]));
        assert!(folder_matches("timer", "alpha", &requests));
        assert!(!folder_matches("timer", "alpha", &[]));
        assert!(folder_matches("~/code", "alpha", &[]), "a path keeps every folder");
    }

    #[test]
    fn a_folder_that_matches_by_name_keeps_all_its_requests() {
        let requests = vec![request(1, "Undo"), request(2, "Bulk")];
        assert_eq!(matching_requests("alpha", "alpha", requests.clone()).len(), 2);
        let found = matching_requests("bulk", "alpha", requests.clone());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].number, 2);
        assert_eq!(matching_requests("", "alpha", requests).len(), 2);
    }

    #[test]
    fn the_folder_detail_names_the_branch_and_the_changed_files() {
        let status = |branch: Option<&str>, files| FolderStatus {
            branch: branch.map(str::to_string),
            files,
            ..FolderStatus::default()
        };
        assert_eq!(folder_detail(None), "");
        assert_eq!(folder_detail(Some(&status(Some("main"), 0))), "main · no changes");
        assert_eq!(folder_detail(Some(&status(Some("main"), 1))), "main · 1 file changed");
        assert_eq!(
            folder_detail(Some(&status(None, 12))),
            "detached HEAD · 12 files changed"
        );
    }

    #[test]
    fn a_fork_names_its_owner() {
        let mut change = request(1, "t");
        assert_eq!(branches_text(&change), "feature/x → main");
        change.from_this_repo = false;
        assert_eq!(branches_text(&change), "mira:feature/x → main · from a fork");
    }

    #[test]
    fn the_pills_follow_review_and_checks() {
        assert_eq!(review_pill(Review::Approved).map(|pill| pill.0), Some("Approved"));
        assert_eq!(review_pill(Review::None), None);
        assert_eq!(
            checks_pill(Some(Outcome::Failed)).map(|pill| pill.0),
            Some("Checks failed")
        );
        assert_eq!(
            checks_pill(Some(Outcome::Queued)).map(|pill| pill.0),
            Some("Checks running")
        );
        assert_eq!(checks_pill(Some(Outcome::Skipped)), None);
        assert_eq!(checks_pill(None), None);
    }
}
