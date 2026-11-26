use crate::bench::{Bench, Mark};
use crate::comparison_control::{Select, comparison_control};
use crate::diff_pane::layout::SPLIT_MIN_WIDTH;
use crate::folder_menu::FolderMenu;
use crate::guide::chrome::{chapter_bar, overview_bar};
use crate::guide::inspector::{GuideInspector, InspectorEvent};
use crate::guide::navigator::{GuideNavigator, NavigatorEvent};
use crate::guide::session::{GuideEvent, GuideRun, GuideSession, Step};
use crate::guide::stage::{GuideStage, StageEvent, banner};
use crate::guide::text::CodeRef;
use crate::pane::{DiffPane, DiffPaneEvent};
use crate::review::{PillEvent, ReviewEvent, ReviewHost, ReviewPill, ReviewSession};
use crate::sidebar::{LoadedChanges, Sidebar, SidebarEvent};
use diffpilot_core::changes;
use diffpilot_core::claude::{
    ClaudeCliReplier, ClaudeCliReviewer, Replier, Reviewer, ScriptedReplier, reviewer_from_env,
};
use diffpilot_core::comparison::{self, Comparison};
use diffpilot_core::diff::{self, DiffMode, FileDiff};
use diffpilot_core::editor;
use diffpilot_core::file_kind::file_kind;
use diffpilot_core::refs::Refs;
use diffpilot_core::run::RunKind;
use diffpilot_core::settings::{self, Settings};
use diffpilot_core::store::Store;
use diffpilot_core::sync::{self, BranchStatus};
use diffpilot_core::tree;
use diffpilot_core::viewed::{ViewedFiles, ViewedKey, ViewedStore};
use diffpilot_core::watcher::{self, WatchGuard};
use diffpilot_ui_kit::components::{
    BAR_HEIGHT, Button, Crumb, IconSegment, Look, OnSelect, SwitchItem, activity_capsule, bar_button, capsule, crumbs,
    glyph_icon, icon_switch, jump_bar, status_bar, switch_bar, tag,
};
use diffpilot_ui_kit::{Glyph, Theme};
use gpui::prelude::*;
use gpui::{
    App, Context, Entity, IntoElement, MouseButton, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Render,
    SharedString, Styled, Subscription, Task, Window, WindowControlArea, div, px,
};
use gpui_component::{ActiveTheme, Icon, InteractiveElementExt as _, StyledExt, h_flex, v_flex};
use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

const PANEL_INSET: f32 = 8.0;
const PANEL_RADIUS: f32 = 20.0;
const MIN_SIDEBAR: f32 = 240.0 + PANEL_INSET;
const IDEAL_SIDEBAR: f32 = 272.0 + PANEL_INSET;
const MAX_SIDEBAR: f32 = 480.0 + PANEL_INSET;
const TOOLBAR_HEIGHT: f32 = 52.0;
const GLASS_HEIGHT: f32 = 35.0;
const NAV_TABS: [SwitchItem; 2] = [
    SwitchItem {
        glyph: Glyph::File,
        glyph_color: None,
        label: "Files",
        hint: "\u{2318}1",
        enabled: true,
    },
    SwitchItem {
        glyph: Glyph::BookOpen,
        glyph_color: None,
        label: "Guide",
        hint: "\u{2318}2",
        enabled: true,
    },
];
const TRAFFIC_LIGHTS: f32 = 96.0;

const DIFF_MODES: [(DiffMode, &str); 2] = [(DiffMode::Unified, "Unified"), (DiffMode::Split, "Split")];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nav {
    Files,
    Guide,
}

#[derive(Clone, PartialEq)]
struct SessionKey {
    comparison: Comparison,
    branch: Option<String>,
}

struct ViewedState {
    files: Mutex<ViewedFiles>,
    version: AtomicU64,
}

impl ViewedState {
    fn mutate(&self, change: impl FnOnce(&mut ViewedFiles)) -> (u64, HashSet<String>) {
        let mut files = self.files.lock().unwrap_or_else(PoisonError::into_inner);
        change(&mut files);
        (self.version.fetch_add(1, Ordering::SeqCst) + 1, files.paths())
    }
}

struct DiffLoaded {
    files: Vec<FileDiff>,
    signature: Signature,
    git_and_parse: Duration,
    fingerprints: Duration,
}

fn load_changes(repo: &std::path::Path, comparison: &Comparison) -> LoadedChanges {
    let started = Instant::now();
    let changes = changes::load(repo, comparison).unwrap_or_default();
    let root = tree::build(&changes);
    let files = tree::files(&root);
    LoadedChanges {
        changes,
        root,
        files,
        elapsed: started.elapsed(),
    }
}

fn load_diff(repo: &std::path::Path, comparison: &Comparison) -> DiffLoaded {
    let started = Instant::now();
    let files = diff::load(repo, comparison).unwrap_or_default();
    let git_and_parse = started.elapsed();
    let started = Instant::now();
    let signature = files
        .iter()
        .map(|file| (file.path.clone(), file.fingerprint()))
        .collect();
    DiffLoaded {
        files,
        signature,
        git_and_parse,
        fingerprints: started.elapsed(),
    }
}

/// What a session asks when Claude reviews or replies.
#[derive(Clone)]
pub struct Claude {
    pub reviewer: Arc<dyn Reviewer>,
    pub replier: Arc<dyn Replier>,
}

impl Claude {
    /// `DIFFPILOT_REVIEWER=scripted` swaps the `claude` command for canned answers.
    pub fn from_env(repo: &std::path::Path, settings: &Settings) -> Self {
        let scripted = std::env::var("DIFFPILOT_REVIEWER").is_ok_and(|kind| kind == "scripted");
        let replier: Arc<dyn Replier> = if scripted {
            Arc::new(ScriptedReplier::default())
        } else {
            Arc::new(ClaudeCliReplier {
                binary: settings.claude_path(),
                model: settings.model_for(RunKind::Reply).name.as_str(),
                ..ClaudeCliReplier::new(repo)
            })
        };
        let reviewer: Arc<dyn Reviewer> = if scripted {
            Arc::from(reviewer_from_env())
        } else {
            Arc::new(ClaudeCliReviewer {
                binary: settings.claude_path(),
                ..ClaudeCliReviewer::default()
            })
        };
        Self { reviewer, replier }
    }
}

fn home_relative(path: &std::path::Path) -> String {
    shorten(&relative_to(
        path,
        std::env::var_os("HOME").as_deref().map(std::path::Path::new),
    ))
}

fn shorten(path: &str) -> String {
    let parts: Vec<&str> = path.split('/').collect();
    match parts.as_slice() {
        [first, .., prev, last] if parts.len() > 4 => format!("{first}/\u{2026}/{prev}/{last}"),
        _ => path.to_string(),
    }
}

fn relative_to(path: &std::path::Path, home: Option<&std::path::Path>) -> String {
    match home.and_then(|home| path.strip_prefix(home).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".into(),
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

type Signature = Vec<(String, String)>;

struct ClearJob {
    fingerprints: HashMap<String, String>,
    signature: Signature,
}

struct Session {
    id: u64,
    key: SessionKey,
    viewed: Option<Arc<ViewedState>>,
    applied_viewed: u64,
    fingerprints: HashMap<String, String>,
    shown: Option<Signature>,
    cleared: Option<Signature>,
    sidebar: Entity<Sidebar>,
    pane: Entity<DiffPane>,
    pill: Entity<ReviewPill>,
    review: Entity<ReviewSession>,
    guide: Entity<GuideSession>,
    navigator: Entity<GuideNavigator>,
    stage: Entity<GuideStage>,
    inspector: Entity<GuideInspector>,
    load_task: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

pub struct RepoView {
    repo: PathBuf,
    folder_name: SharedString,
    folder_menu: Option<Entity<FolderMenu>>,
    settings: Arc<Settings>,
    viewed_store: Arc<ViewedStore>,
    review_store: Arc<Store>,
    claude: Claude,
    viewed_states: HashMap<ViewedKey, Arc<ViewedState>>,
    refs: Option<Refs>,
    branch_status: Option<BranchStatus>,
    uncommitted: usize,
    comparison: Option<Comparison>,
    session: Option<Session>,
    next_session_id: u64,
    nav: Nav,
    inspector_open: bool,
    sidebar_open: bool,
    sidebar_width: Pixels,
    resizing: bool,
    should_move: Rc<Cell<bool>>,
    wrap: bool,
    mode: DiffMode,
    compact_toolbar: bool,
    visible_file: Option<String>,
    watch: Option<WatchGuard>,
    refs_task: Option<Task<()>>,
    status_task: Option<Task<()>>,
    _watch_task: Option<Task<()>>,
}

impl RepoView {
    pub fn new(
        repo: PathBuf,
        settings: Arc<Settings>,
        viewed_store: Arc<ViewedStore>,
        review_store: Arc<Store>,
        claude: Claude,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let folder_name = repo.file_name().map_or_else(
            || repo.to_string_lossy().into_owned(),
            |name| name.to_string_lossy().into_owned(),
        );
        window.set_window_title(&folder_name);
        let mode = settings::diff_mode_override().unwrap_or_else(|| settings.diff_mode());
        let wrap = settings.wrap_lines();
        let mut this = Self {
            repo,
            folder_name: folder_name.into(),
            folder_menu: None,
            settings,
            viewed_store,
            review_store,
            claude,
            viewed_states: HashMap::new(),
            refs: None,
            branch_status: None,
            uncommitted: 0,
            comparison: None,
            session: None,
            next_session_id: 0,
            nav: Nav::Files,
            inspector_open: true,
            sidebar_open: true,
            sidebar_width: px(IDEAL_SIDEBAR),
            resizing: false,
            should_move: Rc::new(Cell::new(false)),
            wrap,
            mode,
            compact_toolbar: false,
            visible_file: None,
            watch: None,
            refs_task: None,
            status_task: None,
            _watch_task: None,
        };
        cx.observe_global::<crate::settings::SettingsChanged>(|this, cx| this.apply_settings(cx))
            .detach();
        this.load_refs(window, cx);
        this.start_watching(window, cx);
        this
    }

    fn apply_settings(&mut self, cx: &mut Context<Self>) {
        let mode = settings::diff_mode_override().unwrap_or_else(|| self.settings.diff_mode());
        self.show_diff_mode(mode, cx);
        self.show_wrap(self.settings.wrap_lines(), cx);
        self.claude = Claude::from_env(&self.repo, &self.settings);
        if let Some(session) = &self.session {
            let claude = self.claude.clone();
            session.review.update(cx, |review, cx| {
                review.use_settings(claude.reviewer, claude.replier, cx)
            });
        }
    }

    fn show_wrap(&mut self, wrap: bool, cx: &mut Context<Self>) {
        if wrap == self.wrap {
            return;
        }
        self.wrap = wrap;
        if let Some(session) = &self.session {
            session.pane.update(cx, |pane, cx| pane.set_wrap(wrap, cx));
        }
        cx.notify();
    }

    pub fn set_title(&mut self, menu: Entity<FolderMenu>, cx: &mut Context<Self>) {
        self.folder_menu = Some(menu);
        cx.notify();
    }

    pub fn review_running(&self, cx: &App) -> bool {
        self.session
            .as_ref()
            .is_some_and(|session| session.pill.read(cx).is_running(cx))
    }

    pub fn set_compact_toolbar(&mut self, compact: bool, cx: &mut Context<Self>) {
        if self.compact_toolbar != compact {
            self.compact_toolbar = compact;
            cx.notify();
        }
    }

    fn show_comment_counts(&mut self, review: &Entity<ReviewSession>, cx: &mut Context<Self>) {
        let counts: HashMap<String, usize> = review.read(cx).comment_counts().into_iter().collect();
        if let Some(session) = &self.session {
            session
                .sidebar
                .update(cx, |sidebar, cx| sidebar.set_comments(counts, cx));
        }
    }

    pub fn set_diff_mode(&mut self, mode: DiffMode, cx: &mut Context<Self>) {
        self.show_diff_mode(mode, cx);
        self.settings.set_diff_mode(mode);
    }

    fn show_diff_mode(&mut self, mode: DiffMode, cx: &mut Context<Self>) {
        if mode == self.mode {
            return;
        }
        self.mode = mode;
        if let Some(session) = &self.session {
            session.pane.update(cx, |pane, cx| pane.set_mode(mode, cx));
        }
        cx.notify();
    }

    pub fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.sidebar_open = !self.sidebar_open;
        cx.notify();
    }

    fn load_refs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let repo = self.repo.clone();
        let settings = self.settings.clone();
        let task = cx.background_spawn(async move { comparison::load_comparison(&repo, &settings) });
        self.refs_task = Some(cx.spawn_in(window, async move |this, cx| {
            let (refs, comparison) = task.await;
            this.update_in(cx, |this, window, cx| this.apply_refs(refs, comparison, window, cx))
                .ok();
        }));
    }

    fn load_status(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let repo = self.repo.clone();
        let task = cx.background_spawn(async move { sync::status(&repo).ok() });
        self.status_task = Some(cx.spawn_in(window, async move |this, cx| {
            let status = task.await;
            this.update(cx, |this, cx| {
                this.branch_status = status;
                cx.notify();
            })
            .ok();
        }));
    }

    fn apply_refs(&mut self, refs: Refs, comparison: Comparison, window: &mut Window, cx: &mut Context<Self>) {
        self.refs = Some(refs);
        self.comparison = Some(comparison);
        self.load_status(window, cx);
        if !self.ensure_session(window, cx) {
            self.start_load(window, cx);
        }
        cx.notify();
    }

    pub fn select_comparison(&mut self, next: Comparison, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(refs), Some(current)) = (self.refs.clone(), self.comparison.clone()) else {
            return;
        };
        if next == current {
            return;
        }
        self.comparison = Some(next.clone());
        let repo = self.repo.clone();
        let settings = self.settings.clone();
        cx.background_spawn(async move {
            comparison::select_comparison(&settings, &repo, &refs, &current, next);
        })
        .detach();
        self.ensure_session(window, cx);
        cx.notify();
    }

    fn ensure_session(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let (Some(refs), Some(comparison)) = (&self.refs, &self.comparison) else {
            return false;
        };
        let key = SessionKey {
            comparison: comparison.clone(),
            branch: refs.checked_out.clone(),
        };
        if self.session.as_ref().is_some_and(|session| session.key == key) {
            return false;
        }
        let (branch_name, comparison) = (refs.checked_out.clone(), comparison.clone());
        self.next_session_id += 1;
        let sidebar = cx.new(|cx| Sidebar::new(window, cx));
        let pane = cx.new(|cx| DiffPane::new(window, cx));
        let (wrap, mode) = (self.wrap, self.mode);
        pane.update(cx, |pane, cx| {
            pane.set_wrap(wrap, cx);
            pane.set_mode(mode, cx);
        });
        let mut host = ReviewHost::new(Some(self.repo.clone()), comparison);
        host.branch_name = branch_name;
        let review = {
            let (settings, claude, store) = (self.settings.clone(), self.claude.clone(), self.review_store.clone());
            cx.new(|_| ReviewSession::new(host, settings, store, claude.reviewer, claude.replier))
        };
        pane.update(cx, |pane, cx| pane.set_review(review.clone(), window, cx));
        let pill = cx.new(|cx| ReviewPill::new(review.clone(), window, cx));
        let guide = {
            let (settings, store) = (self.settings.clone(), self.review_store.clone());
            cx.new(|cx| GuideSession::new(review.clone(), settings, store, cx))
        };
        let navigator = cx.new(|cx| GuideNavigator::new(guide.clone(), cx));
        let stage = {
            let settings = self.settings.clone();
            cx.new(|cx| GuideStage::new(guide.clone(), settings, cx))
        };
        let inspector = cx.new(|cx| GuideInspector::new(guide.clone(), cx));
        self.compact_toolbar = false;
        let subscriptions = vec![
            cx.subscribe_in(&guide, window, |this, _, _: &GuideEvent, window, cx| {
                this.sync_guide(window, cx);
                cx.notify();
            }),
            cx.subscribe_in(&navigator, window, |this, _, event: &NavigatorEvent, window, cx| match event {
                NavigatorEvent::Go(step) => this.go_to(*step, window, cx),
                NavigatorEvent::Reveal(path) => {
                    if let Some(session) = &this.session {
                        session.pane.update(cx, |pane, cx| pane.reveal(path, cx));
                    }
                }
            }),
            cx.subscribe_in(&stage, window, |this, _, StageEvent::Open(code): &StageEvent, window, cx| {
                this.open_ref(code, window, cx)
            }),
            cx.subscribe_in(&inspector, window, |this, _, event: &InspectorEvent, window, cx| match event {
                InspectorEvent::Open(code) => this.open_ref(code, window, cx),
                InspectorEvent::MarkViewedAndContinue => this.mark_chapter_viewed(window, cx),
            }),
            cx.subscribe_in(&review, window, |this, review, event: &ReviewEvent, _, cx| {
                if matches!(event, ReviewEvent::ThreadsChanged(change) if !matches!(change, crate::review::ThreadChange::Updated(_))) {
                    this.show_comment_counts(review, cx);
                }
            }),
            cx.subscribe_in(&pill, window, |this, _, event: &PillEvent, window, cx| match event {
                PillEvent::NeedsRoomChanged(room) => this.set_compact_toolbar(*room, cx),
                PillEvent::CommentOnFile(id) => {
                    if let Some(session) = &this.session {
                        session.pane.update(cx, |pane, cx| pane.focus_thread(*id, window, cx));
                    }
                }
            }),
            cx.subscribe_in(
                &sidebar,
                window,
                |this, _, event: &SidebarEvent, window, cx| match event {
                    SidebarEvent::Select(path) => {
                        if let Some(session) = &this.session {
                            session.pane.update(cx, |pane, cx| pane.reveal(path, cx));
                        }
                    }
                    SidebarEvent::ToggleViewed(path) => this.toggle_viewed(path.clone(), window, cx),
                },
            ),
            cx.subscribe_in(
                &pane,
                window,
                |this, _, event: &DiffPaneEvent, window, cx| match event {
                    DiffPaneEvent::ToggleViewed(path) => this.toggle_viewed(path.clone(), window, cx),
                    DiffPaneEvent::VisibleFile(path) => {
                        if this.visible_file.as_deref() != Some(path.as_str()) {
                            this.visible_file = Some(path.clone());
                            cx.notify();
                        }
                    }
                    DiffPaneEvent::OpenInEditor { path, line } => this.open_in_editor(path, *line, cx),
                    DiffPaneEvent::FirstPaint => Bench::mark_now(cx, Mark::FirstDiffPaint),
                    DiffPaneEvent::ShownMode(_) => cx.notify(),
                    DiffPaneEvent::OpenRef(code) => this.open_ref(code, window, cx),
                },
            ),
        ];
        self.session = Some(Session {
            id: self.next_session_id,
            key,
            viewed: None,
            applied_viewed: 0,
            fingerprints: HashMap::new(),
            shown: None,
            cleared: None,
            sidebar,
            pane,
            pill,
            review,
            guide,
            navigator,
            stage,
            inspector,
            load_task: None,
            _subscriptions: subscriptions,
        });
        self.start_load(window, cx);
        self.sync_guide(window, cx);
        true
    }

    fn start_load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = self.session.as_mut() else { return };
        let id = session.id;
        let comparison = session.key.comparison.clone();
        let refs = self.refs.clone();
        let repo = self.repo.clone();

        let changes_task = cx.background_spawn({
            let (repo, comparison) = (repo.clone(), comparison.clone());
            async move { load_changes(&repo, &comparison) }
        });
        let diff_task = cx.background_spawn({
            let (repo, comparison) = (repo.clone(), comparison.clone());
            async move { load_diff(&repo, &comparison) }
        });
        let existing = session.viewed.clone();
        let viewed_task = existing.is_none().then(|| {
            let store = self.viewed_store.clone();
            cx.background_spawn(async move {
                let key = ViewedKey::for_comparison(&repo, &comparison, refs.as_ref());
                let files = ViewedFiles::new(key.clone(), store);
                let paths = files.paths();
                (
                    key,
                    Arc::new(ViewedState {
                        files: Mutex::new(files),
                        version: AtomicU64::new(0),
                    }),
                    paths,
                )
            })
        });

        session.load_task = Some(cx.spawn_in(window, async move |this, cx| {
            let loaded = changes_task.await;
            this.update(cx, |this, cx| this.apply_changes(id, loaded, cx)).ok();

            let viewed = match viewed_task {
                Some(task) => {
                    let (key, loaded, paths) = task.await;
                    let Ok(state) = this.update(cx, |this, cx| this.install_viewed(id, key, loaded, paths, cx)) else {
                        return;
                    };
                    state
                }
                None => match existing {
                    Some(state) => state,
                    None => return,
                },
            };

            let diff = diff_task.await;
            let Ok(Some(ClearJob {
                fingerprints,
                signature,
            })) = this.update(cx, |this, cx| this.apply_diff(id, diff, cx))
            else {
                return;
            };
            let (version, paths) = cx
                .background_spawn(async move {
                    viewed.mutate(|files| {
                        let current: HashMap<String, String> = files
                            .paths()
                            .into_iter()
                            .filter_map(|path| fingerprints.get(&path).map(|print| (path, print.clone())))
                            .collect();
                        files.clear_changed(&current);
                    })
                })
                .await;
            this.update(cx, |this, cx| this.finish_clear(id, signature, version, paths, cx))
                .ok();
        }));
    }

    fn session_mut(&mut self, id: u64) -> Option<&mut Session> {
        self.session.as_mut().filter(|session| session.id == id)
    }

    fn apply_changes(&mut self, id: u64, loaded: LoadedChanges, cx: &mut Context<Self>) {
        Bench::note_stage(cx, "changes_git_and_tree", loaded.elapsed);
        let uncommitted: HashSet<String> = loaded
            .changes
            .iter()
            .filter(|change| change.is_uncommitted)
            .map(|change| change.path.clone())
            .collect();
        self.uncommitted = uncommitted.len();
        if let Some(session) = self.session.as_ref() {
            session
                .pane
                .update(cx, |pane, cx| pane.set_uncommitted(uncommitted, cx));
        }
        if let Some(session) = self.session_mut(id) {
            session
                .sidebar
                .update(cx, |sidebar, cx| sidebar.set_changes(loaded, cx));
        }
    }

    /// One `ViewedFiles` owns each key for the whole run. A second session on the same key shares it, so a
    /// save from the first can never be overwritten by a copy the second loaded earlier.
    fn install_viewed(
        &mut self,
        id: u64,
        key: ViewedKey,
        loaded: Arc<ViewedState>,
        paths: HashSet<String>,
        cx: &mut Context<Self>,
    ) -> Arc<ViewedState> {
        let known = self.viewed_states.contains_key(&key);
        let state = self.viewed_states.entry(key).or_insert(loaded).clone();
        let Some(session) = self.session_mut(id) else {
            return state;
        };
        session.viewed = Some(state.clone());
        if known {
            let task = cx.background_spawn({
                let state = state.clone();
                async move { state.mutate(|_| {}) }
            });
            cx.spawn(async move |this, cx| {
                let (version, paths) = task.await;
                this.update(cx, |this, cx| this.apply_viewed(id, version, paths, cx))
                    .ok();
            })
            .detach();
        } else {
            session
                .sidebar
                .update(cx, |sidebar, cx| sidebar.set_viewed(paths.clone(), cx));
            session
                .guide
                .update(cx, |guide, cx| guide.set_viewed(paths.clone(), cx));
            session.pane.update(cx, |pane, cx| pane.set_viewed(paths, cx));
        }
        state
    }

    /// Shows the diff if it changed. Returns the viewed-mark cleanup still owed for it: `cleared` is only set once
    /// that cleanup has run, so a load that replaces this one retries it.
    fn apply_diff(&mut self, id: u64, diff: DiffLoaded, cx: &mut Context<Self>) -> Option<ClearJob> {
        Bench::note_stage(cx, "diff_git_and_parse", diff.git_and_parse);
        Bench::note_stage(cx, "diff_fingerprints", diff.fingerprints);
        let session = self.session_mut(id)?;
        if session.cleared.as_ref() == Some(&diff.signature) {
            return None;
        }
        let fingerprints: HashMap<String, String> = diff.signature.iter().cloned().collect();
        if session.shown.as_ref() != Some(&diff.signature) {
            session.fingerprints = fingerprints.clone();
            session.shown = Some(diff.signature.clone());
            session.pane.update(cx, |pane, cx| pane.set_files(diff.files, cx));
        }
        Some(ClearJob {
            fingerprints,
            signature: diff.signature,
        })
    }

    fn finish_clear(
        &mut self,
        id: u64,
        signature: Signature,
        version: u64,
        paths: HashSet<String>,
        cx: &mut Context<Self>,
    ) {
        if let Some(session) = self.session_mut(id)
            && session.shown.as_ref() == Some(&signature)
        {
            session.cleared = Some(signature);
        }
        self.apply_viewed(id, version, paths, cx);
    }

    fn apply_viewed(&mut self, id: u64, version: u64, paths: HashSet<String>, cx: &mut Context<Self>) {
        let Some(session) = self.session_mut(id) else { return };
        if version <= session.applied_viewed {
            return;
        }
        session.applied_viewed = version;
        session
            .sidebar
            .update(cx, |sidebar, cx| sidebar.set_viewed(paths.clone(), cx));
        session
            .guide
            .update(cx, |guide, cx| guide.set_viewed(paths.clone(), cx));
        session.pane.update(cx, |pane, cx| pane.set_viewed(paths, cx));
    }

    fn open_in_editor(&self, path: &str, line: Option<u32>, cx: &mut Context<Self>) {
        let target = editor::OpenTarget {
            path: self.repo.join(path),
            line,
            column: None,
        };
        let settings = self.settings.clone();
        cx.background_spawn(async move {
            let choice = settings.editor().unwrap_or_else(editor::detect);
            if let Err(error) = editor::open(&choice, &target) {
                eprintln!("diffpilot: {error}");
            }
        })
        .detach();
    }

    fn toggle_viewed(&mut self, path: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = &self.session else { return };
        let Some(state) = session.viewed.clone() else { return };
        let id = session.id;
        let fingerprint = session.fingerprints.get(&path).cloned();
        let task =
            cx.background_spawn(async move { state.mutate(|files| files.toggle(&path, fingerprint.as_deref())) });
        cx.spawn_in(window, async move |this, cx| {
            let (version, paths) = task.await;
            this.update(cx, |this, cx| this.apply_viewed(id, version, paths, cx))
                .ok();
        })
        .detach();
    }

    fn mark_viewed(&mut self, paths: Vec<String>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = &self.session else { return };
        let Some(state) = session.viewed.clone() else { return };
        let id = session.id;
        let fingerprints = session.fingerprints.clone();
        let task = cx.background_spawn(async move {
            state.mutate(|files| {
                let unviewed: Vec<&String> = paths.iter().filter(|path| !files.contains(path)).collect();
                for path in unviewed {
                    files.toggle(path, fingerprints.get(path).map(String::as_str));
                }
            })
        });
        cx.spawn_in(window, async move |this, cx| {
            let (version, paths) = task.await;
            this.update(cx, |this, cx| this.apply_viewed(id, version, paths, cx))
                .ok();
        })
        .detach();
    }

    pub fn nav(&self) -> Nav {
        self.nav
    }

    pub fn show_nav(&mut self, nav: Nav, window: &mut Window, cx: &mut Context<Self>) {
        if nav != self.nav {
            self.nav = nav;
            self.sync_guide(window, cx);
            cx.notify();
        }
    }

    pub fn toggle_guide(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let next = match self.nav {
            Nav::Files => Nav::Guide,
            Nav::Guide => Nav::Files,
        };
        self.show_nav(next, window, cx);
    }

    pub fn toggle_inspector(&mut self, cx: &mut Context<Self>) {
        self.inspector_open = !self.inspector_open;
        cx.notify();
    }

    fn chapter_open(&self, cx: &App) -> bool {
        let Some(session) = &self.session else { return false };
        let guide = session.guide.read(cx);
        self.nav == Nav::Guide
            && guide.chapter().is_some()
            && guide.stored().is_some()
            && !matches!(guide.run(), GuideRun::Writing { .. })
    }

    pub fn step_chapter(&mut self, delta: isize, cx: &mut Context<Self>) {
        if !self.chapter_open(cx) {
            return;
        }
        if let Some(session) = &self.session {
            session.guide.update(cx, |guide, cx| match delta {
                1.. => guide.next(cx),
                _ => guide.previous(cx),
            });
        }
    }

    fn go_to(&mut self, step: Step, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(session) = &self.session {
            session.guide.update(cx, |guide, cx| guide.go(step, cx));
        }
        self.sync_guide(window, cx);
    }

    fn sync_guide(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = &self.session else { return };
        let (filter, notes) = if self.chapter_open(cx) {
            let guide = session.guide.read(cx);
            (guide.chapter().map(|chapter| chapter.files.clone()), guide.pane_notes())
        } else {
            (None, Vec::new())
        };
        session.pane.update(cx, |pane, cx| {
            pane.set_filter(filter, cx);
            pane.set_notes(notes, window, cx);
        });
    }

    /// Jumps to a `[[path#L12]]` ref: into the chapter that holds the file when the open one does not.
    fn open_ref(&mut self, code: &CodeRef, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = &self.session else { return };
        let guide = session.guide.read(cx);
        let here = guide
            .chapter()
            .is_some_and(|chapter| chapter.files.contains(&code.path));
        let target = guide
            .reading()
            .chapters
            .iter()
            .position(|chapter| chapter.files.contains(&code.path));
        let pane = session.pane.clone();
        if self.nav == Nav::Guide
            && !here
            && let Some(index) = target
        {
            self.go_to(Step::Chapter(index), window, cx);
        } else if self.nav == Nav::Guide && target.is_none() {
            return;
        }
        let side = code.side();
        pane.update(cx, |pane, cx| match code.line {
            Some(line) => pane.reveal_line(&code.path, side, line, cx),
            None => pane.reveal(&code.path, cx),
        });
    }

    fn mark_chapter_viewed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = &self.session else { return };
        if session.viewed.is_none() {
            return;
        }
        let guide = session.guide.clone();
        let Some(chapter) = guide.read(cx).chapter() else {
            return;
        };
        let paths = chapter.files.clone();
        self.mark_viewed(paths, window, cx);
        guide.update(cx, |guide, cx| guide.next(cx));
        self.sync_guide(window, cx);
    }

    fn start_watching(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let repo = self.repo.clone();
        let task = cx.background_spawn(async move { watcher::watch(&repo) });
        self._watch_task = Some(cx.spawn_in(window, async move |this, cx| {
            let Ok((guard, changes)) = task.await else { return };
            this.update(cx, |this, _| this.watch = Some(guard)).ok();
            while let Ok(change) = changes.recv().await {
                let mut head_changed = change.head_changed;
                while let Ok(more) = changes.try_recv() {
                    head_changed |= more.head_changed;
                }
                let applied = this.update_in(cx, |this, window, cx| this.on_repo_change(head_changed, window, cx));
                if applied.is_err() {
                    break;
                }
            }
        }));
    }

    fn on_repo_change(&mut self, head_changed: bool, window: &mut Window, cx: &mut Context<Self>) {
        if head_changed {
            self.load_refs(window, cx);
        } else {
            self.start_load(window, cx);
            self.load_status(window, cx);
        }
    }

    pub fn sidebar_scroll(&self, cx: &gpui::App) -> Option<(gpui::UniformListScrollHandle, usize)> {
        let session = self.session.as_ref()?;
        let sidebar = session.sidebar.read(cx);
        Some((sidebar.scroll_handle(), sidebar.row_count()))
    }
}

fn drag_region(
    should_move: &Rc<Cell<bool>>,
) -> impl FnOnce(gpui::Stateful<gpui::Div>) -> gpui::Stateful<gpui::Div> + use<> {
    let state = should_move.clone();
    move |region| {
        let (down, up, moved) = (state.clone(), state.clone(), state);
        region
            .window_control_area(WindowControlArea::Drag)
            .on_double_click(|_, window, _| window.titlebar_double_click())
            .on_mouse_down(MouseButton::Left, move |_, _, _| down.set(true))
            .on_mouse_up(MouseButton::Left, move |_, _, _| up.set(false))
            .on_mouse_move(move |_, window, _| {
                if moved.replace(false) {
                    window.start_window_move();
                }
            })
    }
}

fn sidebar_toggle(cx: &mut Context<RepoView>) -> Button {
    Button::new("toggle-sidebar")
        .look(Look::Ghost)
        .icon(Glyph::PanelLeft, 16.)
        .size(px(28.))
        .rounded_full()
        .tooltip("Toggle Sidebar")
        .on_click(cx.listener(|this, _, _, cx| this.toggle_sidebar(cx)))
}

impl RepoView {
    fn diff_mode_switch(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let view = cx.entity().downgrade();
        let on_select: OnSelect = Rc::new(move |index, _, cx| {
            view.update(cx, |this, cx| this.set_diff_mode(DIFF_MODES[index].0, cx))
                .ok();
        });
        let shown = self
            .session
            .as_ref()
            .map_or(self.mode, |session| session.pane.read(cx).state().shown_mode());
        let selected = DIFF_MODES.iter().position(|(mode, _)| *mode == shown).unwrap_or(0);
        let split_note = (shown != self.mode)
            .then(|| format!("Split view needs a pane at least {} pt wide", SPLIT_MIN_WIDTH as u32));
        let items = DIFF_MODES
            .iter()
            .map(|(mode, title)| IconSegment {
                name: title,
                glyph: if *mode == DiffMode::Unified {
                    Glyph::Rows2
                } else {
                    Glyph::Columns2
                },
                tooltip: match (&split_note, mode) {
                    (Some(note), DiffMode::Split) => note.clone().into(),
                    _ => (*title).into(),
                },
            })
            .collect();
        icon_switch(&Theme::of(cx), "diff-mode", items, selected, on_select).into_any_element()
    }

    fn wrap_button(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        bar_button(
            &Theme::of(cx),
            "toggle-wrap",
            Glyph::TextWrap,
            "Wrap lines",
            true,
            self.wrap,
            cx.listener(|this, _, _, cx| {
                let wrap = !this.wrap;
                this.show_wrap(wrap, cx);
                this.settings.set_wrap_lines(wrap);
            }),
        )
        .into_any_element()
    }

    fn path_crumbs(&self, tokens: &Theme) -> gpui::AnyElement {
        let Some(path) = self.visible_file.as_deref() else {
            return div().into_any_element();
        };
        let parts: Vec<&str> = path.split('/').collect();
        let items = parts
            .iter()
            .enumerate()
            .map(|(index, part)| {
                let last = index + 1 == parts.len();
                if last {
                    Crumb {
                        glyph: Glyph::for_file(part),
                        color: Some(tokens.tint(file_kind(part).tint())),
                        label: (*part).to_string().into(),
                        last,
                    }
                } else {
                    Crumb {
                        glyph: Glyph::FolderFilled,
                        color: Some(tokens.chrome.folder),
                        label: (*part).to_string().into(),
                        last,
                    }
                }
            })
            .collect();
        crumbs(tokens, items).into_any_element()
    }

    fn guide_jump_bar(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let tokens = Theme::of(cx);
        let Some(session) = &self.session else {
            return div().into_any_element();
        };
        if !self.chapter_open(cx) {
            return overview_bar(&session.guide, &tokens, cx).into_any_element();
        }
        chapter_bar(&session.guide, self.visible_file.as_deref(), &tokens, cx)
            .child(self.diff_mode_switch(cx))
            .child(self.wrap_button(cx))
            .into_any_element()
    }

    fn jump_bar(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        if self.nav == Nav::Guide {
            return self.guide_jump_bar(cx);
        }
        let tokens = Theme::of(cx);
        let threads = self
            .session
            .as_ref()
            .and_then(|session| session.pane.read(cx).review().cloned())
            .map_or(0, |review| review.read(cx).thread_count());
        let nothing = |_: &gpui::ClickEvent, _: &mut Window, _: &mut gpui::App| {};
        jump_bar(&tokens)
            .id("jump-bar")
            .child(bar_button(
                &tokens,
                "jump-back",
                Glyph::ChevronLeft,
                "Back",
                false,
                false,
                nothing,
            ))
            .child(bar_button(
                &tokens,
                "jump-forward",
                Glyph::ChevronRight,
                "Forward",
                false,
                false,
                nothing,
            ))
            .child(
                tag(div(), || {
                    format!("jump-bar:path:{}", self.visible_file.as_deref().unwrap_or_default())
                })
                .flex_1()
                .min_w_0()
                .child(self.path_crumbs(&tokens)),
            )
            .child(
                tag(h_flex(), || format!("jump-bar:threads:{threads}"))
                    .gap(px(3.))
                    .flex_none()
                    .text_size(px(13.))
                    .text_color(tokens.ui.text2)
                    .child(
                        Icon::new(Glyph::MessageSquare)
                            .size(px(14.))
                            .text_color(tokens.ui.muted),
                    )
                    .child(threads.to_string()),
            )
            .child(bar_button(
                &tokens,
                "thread-previous",
                Glyph::ChevronUp,
                "Previous thread",
                false,
                false,
                nothing,
            ))
            .child(bar_button(
                &tokens,
                "thread-next",
                Glyph::ChevronDown,
                "Next thread",
                false,
                false,
                nothing,
            ))
            .child(div().w(px(4.)))
            .child(self.diff_mode_switch(cx))
            .child(self.wrap_button(cx))
            .into_any_element()
    }

    fn bottom_bar(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let tokens = Theme::of(cx);
        let uncommitted = self.uncommitted;
        status_bar(&tokens)
            .id("bottom-bar")
            .h(px(BAR_HEIGHT))
            .child(
                h_flex()
                    .gap(px(6.))
                    .opacity(0.6)
                    .child(glyph_icon(Glyph::SquareTerminal, 14., tokens.ui.muted))
                    .child("Claude \u{b7} no review yet"),
            )
            .child(div().flex_1())
            .when(uncommitted > 0, |this| {
                this.child(
                    h_flex()
                        .gap(px(5.))
                        .text_color(tokens.chrome.uncommitted)
                        .child(glyph_icon(Glyph::Pencil, 13., tokens.chrome.uncommitted))
                        .child(format!("{uncommitted} uncommitted")),
                )
            })
            .child(
                h_flex()
                    .gap(px(4.))
                    .opacity(0.45)
                    .child(glyph_icon(Glyph::GitCommit, 14., tokens.ui.muted))
                    .child("Commit\u{2026}"),
            )
            .into_any_element()
    }

    fn toolbar(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let tokens = Theme::of(cx);
        let view = cx.entity().downgrade();
        let select: Select = Rc::new(move |next, window, cx| {
            view.update(cx, |this, cx| this.select_comparison(next, window, cx))
                .ok();
        });
        let control = match (&self.refs, &self.comparison) {
            (Some(refs), Some(comparison)) => {
                Some(comparison_control(refs, comparison, self.compact_toolbar, select, &tokens).into_any_element())
            }
            _ => None,
        };
        let pill = self.session.as_ref().map(|session| {
            capsule(&tokens, GLASS_HEIGHT)
                .id("toolbar-claude-slot")
                .px(px(6.))
                .child(session.pill.clone())
        });
        let activity = self.branch_status.as_ref().map(|status| {
            let counts = status.upstream.is_some().then_some((status.ahead, status.behind));
            activity_capsule(&tokens, counts)
        });
        let inspector = bar_button(
            &tokens,
            "toggle-inspector",
            Glyph::PanelRight,
            "Inspector",
            self.chapter_open(cx),
            self.chapter_open(cx) && self.inspector_open,
            cx.listener(|this, _, _, cx| this.toggle_inspector(cx)),
        );
        let drag = drag_region(&self.should_move);
        let leading = (!self.sidebar_open).then(|| tag(sidebar_toggle(cx), || "toolbar:sidebar-toggle"));
        drag(
            h_flex()
                .id("toolbar")
                .h(px(TOOLBAR_HEIGHT))
                .flex_shrink_0()
                .gap(px(10.))
                .pr(px(12.))
                .border_b_1()
                .border_color(cx.theme().border),
        )
        .child(
            h_flex()
                .min_w(px(110.))
                .max_w(px(300.))
                .flex_shrink()
                .pl(px(if self.sidebar_open { 20. } else { TRAFFIC_LIGHTS }))
                .gap_2()
                .children(leading)
                .child(
                    v_flex()
                        .min_w_0()
                        .child(match &self.folder_menu {
                            Some(menu) => menu.clone().into_any_element(),
                            None => div()
                                .text_size(px(15.))
                                .font_semibold()
                                .text_color(tokens.ui.text)
                                .whitespace_nowrap()
                                .child(self.folder_name.clone())
                                .into_any_element(),
                        })
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(tokens.ui.muted)
                                .whitespace_nowrap()
                                .overflow_hidden()
                                .text_ellipsis()
                                .child(home_relative(&self.repo)),
                        ),
                ),
        )
        .children(pill)
        .children(control)
        .child(div().flex_1())
        .children(activity)
        .child(inspector)
        .into_any_element()
    }
}

impl Render for RepoView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let background = theme.background;
        let tokens = Theme::of(cx);
        let width = self.sidebar_width;
        let drag = drag_region(&self.should_move);
        let toggle = tag(sidebar_toggle(cx), || "sidebar:toggle");
        let toolbar = self.toolbar(cx);
        let jump_bar = self.jump_bar(cx);
        let bottom_bar = self.bottom_bar(cx);

        let sidebar_content = match (&self.session, self.nav) {
            (Some(session), Nav::Files) => session.sidebar.clone().into_any_element(),
            (Some(session), Nav::Guide) => session.navigator.clone().into_any_element(),
            (None, _) => div().flex_1().into_any_element(),
        };
        let chapter_open = self.chapter_open(cx);
        let detail_content = match &self.session {
            Some(session) if self.nav == Nav::Guide && !chapter_open => session.stage.clone().into_any_element(),
            Some(session) if chapter_open => {
                let notice = banner(&session.guide, &tokens, cx);
                v_flex()
                    .size_full()
                    .children(notice.map(|notice| div().px(px(16.)).pt(px(12.)).pb(px(4.)).child(notice)))
                    .child(div().flex_1().min_h_0().child(session.pane.clone()))
                    .into_any_element()
            }
            Some(session) => session.pane.clone().into_any_element(),
            None => div().into_any_element(),
        };
        let inspector = match &self.session {
            Some(session) if chapter_open && self.inspector_open => Some(session.inspector.clone()),
            _ => None,
        };
        let selected_nav = match self.nav {
            Nav::Files => 0,
            Nav::Guide => 1,
        };
        let on_nav: OnSelect = {
            let view = cx.entity().downgrade();
            Rc::new(move |index, window, cx| {
                let nav = if index == 0 { Nav::Files } else { Nav::Guide };
                view.update(cx, |this, cx| this.show_nav(nav, window, cx)).ok();
            })
        };

        h_flex()
            .id("repo-view")
            .size_full()
            .bg(background)
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                if !this.resizing {
                    return;
                }
                if event.pressed_button != Some(MouseButton::Left) {
                    this.resizing = false;
                    return;
                }
                this.sidebar_width = event.position.x.clamp(px(MIN_SIDEBAR), px(MAX_SIDEBAR));
                cx.notify();
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, _| this.resizing = false),
            )
            .when(self.sidebar_open, |this| {
                this.child(
                    div()
                        .id("sidebar-column")
                        .relative()
                        .w(width)
                        .flex_shrink_0()
                        .h_full()
                        .bg(background)
                        .child(
                            div()
                                .absolute()
                                .top_0()
                                .bottom_0()
                                .left_0()
                                .w(px(PANEL_INSET))
                                .bg(tokens.chrome.gutter),
                        )
                        .child(
                            v_flex()
                                .absolute()
                                .top(px(PANEL_INSET))
                                .bottom(px(PANEL_INSET))
                                .left(px(PANEL_INSET))
                                .right_0()
                                .rounded(px(PANEL_RADIUS))
                                .overflow_hidden()
                                .bg(tokens.chrome.panel)
                                .border_1()
                                .border_color(tokens.chrome.panel_border)
                                .shadow(tokens.glass_shadow())
                                .child(
                                    drag(
                                        h_flex()
                                            .id("sidebar-top")
                                            .h(px(37.))
                                            .flex_shrink_0()
                                            .pr(px(14.))
                                            .justify_end()
                                            .items_start()
                                            .pt(px(PANEL_INSET - 1.)),
                                    )
                                    .child(toggle),
                                )
                                .child(div().px(px(10.)).pb(px(8.)).child(switch_bar(
                                    &tokens,
                                    "nav-tab",
                                    &NAV_TABS,
                                    selected_nav,
                                    on_nav,
                                )))
                                .child(div().flex_1().min_h_0().child(sidebar_content)),
                        )
                        .child(
                            div()
                                .id("sidebar-handle")
                                .absolute()
                                .top_0()
                                .bottom_0()
                                .right_0()
                                .w(px(6.))
                                .cursor_col_resize()
                                .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, _| this.resizing = true)),
                        ),
                )
            })
            .child(
                v_flex()
                    .id("detail")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(toolbar)
                    .child(jump_bar)
                    .child(
                        h_flex()
                            .flex_1()
                            .min_h_0()
                            .child(div().flex_1().min_w_0().h_full().child(detail_content))
                            .children(inspector),
                    )
                    .child(bottom_bar),
            )
    }
}

#[cfg(feature = "testing")]
impl RepoView {
    pub fn pane(&self) -> Entity<DiffPane> {
        self.current().pane.clone()
    }

    pub fn guide(&self) -> Entity<GuideSession> {
        self.current().guide.clone()
    }

    pub fn review(&self) -> Entity<ReviewSession> {
        self.current().review.clone()
    }
}

#[cfg(any(test, feature = "testing"))]
impl RepoView {
    fn current(&self) -> &Session {
        self.session.as_ref().expect("a session")
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_toolbar_path_starts_at_the_home_folder() {
        use std::path::Path;
        let home = Some(Path::new("/Users/me"));
        assert_eq!(relative_to(Path::new("/Users/me/code/app"), home), "~/code/app");
        assert_eq!(relative_to(Path::new("/Users/me"), home), "~");
        assert_eq!(shorten("~/a/b/c/d/e"), "~/\u{2026}/d/e");
        assert_eq!(shorten("~/a/b/c"), "~/a/b/c");
        assert_eq!(relative_to(Path::new("/srv/app"), home), "/srv/app");
        assert_eq!(relative_to(Path::new("/Users/me/app"), None), "/Users/me/app");
    }

    use super::*;
    use crate::testing::{Opened, git, open, open_with, write};
    use gpui::TestAppContext;

    fn sidebar_paths(opened: &Opened, cx: &mut gpui::VisualTestContext) -> Vec<String> {
        let sidebar = opened.view.read_with(cx, |view, _| view.current().sidebar.clone());
        sidebar.read_with(cx, |sidebar, _| sidebar.paths())
    }

    #[gpui::test]
    fn opens_on_the_checked_out_branch_compared_to_main(cx: &mut TestAppContext) {
        let (opened, cx) = open(cx);
        opened.view.read_with(cx, |view, _| {
            assert_eq!(view.comparison, Some(Comparison::checked_out("main", true)));
            assert_eq!(
                view.refs.as_ref().and_then(|refs| refs.checked_out.as_deref()),
                Some("feature")
            );
        });
        let paths = sidebar_paths(&opened, cx);
        assert_eq!(paths, ["docs", "docs/committed.md", "src", "src/staged.rs", "base.txt"]);
        let sidebar = opened.view.read_with(cx, |view, _| view.current().sidebar.clone());
        sidebar.read_with(cx, |sidebar, _| assert_eq!(sidebar.summary().file_count, 3));
    }

    #[gpui::test]
    fn picking_a_comparison_starts_a_new_session_and_remembers_it(cx: &mut TestAppContext) {
        let (opened, cx) = open(cx);
        let first = opened.view.read_with(cx, |view, _| view.current().id);

        opened.view.update_in(cx, |view, window, cx| {
            view.select_comparison(Comparison::Staged, window, cx)
        });
        cx.run_until_parked();

        let second = opened.view.read_with(cx, |view, _| view.current().id);
        assert_ne!(first, second);
        assert_eq!(sidebar_paths(&opened, cx), ["src", "src/staged.rs"]);

        opened.view.update_in(cx, |view, window, cx| {
            view.select_comparison(Comparison::WorkingTree, window, cx)
        });
        cx.run_until_parked();
        assert_eq!(sidebar_paths(&opened, cx), ["src", "src/staged.rs", "base.txt"]);

        let saved = opened.view.read_with(cx, |view, _| {
            let key = comparison::store_key(&view.repo, Some("feature"));
            view.settings.comparison(&key)
        });
        assert_eq!(saved, Some(Comparison::WorkingTree));
    }

    #[gpui::test]
    fn the_diff_mode_is_remembered_and_reaches_every_session(cx: &mut TestAppContext) {
        let (opened, cx) = open(cx);
        let pane_mode = |opened: &Opened, cx: &mut gpui::VisualTestContext| {
            let pane = opened.view.read_with(cx, |view, _| view.current().pane.clone());
            pane.read_with(cx, |pane, _| pane.state().mode())
        };
        assert_eq!(pane_mode(&opened, cx), DiffMode::Unified);

        opened
            .view
            .update(cx, |view, cx| view.set_diff_mode(DiffMode::Split, cx));
        cx.run_until_parked();
        assert_eq!(pane_mode(&opened, cx), DiffMode::Split);
        assert_eq!(
            opened.view.read_with(cx, |view, _| view.settings.diff_mode()),
            DiffMode::Split
        );

        opened.view.update_in(cx, |view, window, cx| {
            view.select_comparison(Comparison::Staged, window, cx)
        });
        cx.run_until_parked();
        assert_eq!(
            pane_mode(&opened, cx),
            DiffMode::Split,
            "the next session starts in split"
        );
    }

    #[gpui::test]
    fn a_saved_diff_mode_is_used_when_the_folder_opens(cx: &mut TestAppContext) {
        let settings = Settings::in_memory();
        settings.set_diff_mode(DiffMode::Split);
        let (opened, cx) = open_with(cx, settings);
        let pane = opened.view.read_with(cx, |view, _| view.current().pane.clone());
        pane.read_with(cx, |pane, _| assert_eq!(pane.state().mode(), DiffMode::Split));
    }

    #[gpui::test]
    fn a_narrow_window_shows_unified_rows_while_the_choice_stays_split(cx: &mut TestAppContext) {
        let (opened, cx) = open(cx);
        opened
            .view
            .update(cx, |view, cx| view.set_diff_mode(DiffMode::Split, cx));
        cx.simulate_resize(gpui::size(px(1400.0), px(800.0)));
        cx.run_until_parked();
        let pane = opened.view.read_with(cx, |view, _| view.current().pane.clone());
        pane.read_with(cx, |pane, _| assert_eq!(pane.state().shown_mode(), DiffMode::Split));

        cx.simulate_resize(gpui::size(px(800.0), px(800.0)));
        cx.run_until_parked();
        pane.read_with(cx, |pane, _| {
            assert_eq!(pane.state().mode(), DiffMode::Split);
            assert_eq!(pane.state().shown_mode(), DiffMode::Unified);
        });
        assert_eq!(opened.view.read_with(cx, |view, _| view.mode), DiffMode::Split);
    }

    #[gpui::test]
    fn selecting_the_same_comparison_keeps_the_session(cx: &mut TestAppContext) {
        let (opened, cx) = open(cx);
        let first = opened.view.read_with(cx, |view, _| view.current().id);
        opened.view.update_in(cx, |view, window, cx| {
            view.select_comparison(Comparison::checked_out("main", true), window, cx)
        });
        cx.run_until_parked();
        assert_eq!(opened.view.read_with(cx, |view, _| view.current().id), first);
    }

    #[gpui::test]
    fn reviewing_another_branch_shows_its_changes(cx: &mut TestAppContext) {
        let (opened, cx) = open(cx);
        opened.view.update_in(cx, |view, window, cx| {
            view.select_comparison(Comparison::branch("feature", "main"), window, cx)
        });
        cx.run_until_parked();
        assert_eq!(sidebar_paths(&opened, cx), ["docs", "docs/committed.md"]);
    }

    #[gpui::test]
    fn toggling_viewed_updates_the_sidebar_and_the_store(cx: &mut TestAppContext) {
        let (opened, cx) = open(cx);
        opened
            .view
            .update_in(cx, |view, window, cx| view.toggle_viewed("base.txt".into(), window, cx));
        cx.run_until_parked();

        let sidebar = opened.view.read_with(cx, |view, _| view.current().sidebar.clone());
        sidebar.read_with(cx, |sidebar, _| assert_eq!(sidebar.summary().viewed, 1));
        let key = opened.view.read_with(cx, |view, _| {
            ViewedKey::for_comparison(&view.repo, &Comparison::checked_out("main", true), view.refs.as_ref())
        });
        let saved = opened.store.load(&key);
        let fingerprint = saved.get("base.txt").expect("saved");
        assert!(!fingerprint.is_empty(), "the file fingerprint is stored with the mark");

        opened
            .view
            .update_in(cx, |view, window, cx| view.toggle_viewed("base.txt".into(), window, cx));
        cx.run_until_parked();
        sidebar.read_with(cx, |sidebar, _| assert_eq!(sidebar.summary().viewed, 0));
        assert!(opened.store.load(&key).is_empty());
    }

    #[gpui::test]
    fn the_sidebar_selection_reveals_the_file_in_the_pane(cx: &mut TestAppContext) {
        let (opened, cx) = open(cx);
        let (sidebar, pane) = opened.view.read_with(cx, |view, _| {
            let session = view.current();
            (session.sidebar.clone(), session.pane.clone())
        });
        assert_eq!(pane.read_with(cx, |pane, _| pane.state().scroll()), 0.0);

        sidebar.update_in(cx, |sidebar, window, cx| sidebar.select_index(3, window, cx));
        cx.run_until_parked();

        pane.read_with(cx, |pane, _| {
            assert!(pane.state().scroll() > 0.0, "the pane scrolled");
            assert_eq!(pane.state().visible_path(), Some("src/staged.rs"));
        });
    }

    #[gpui::test]
    fn a_change_on_disk_reloads_the_sidebar(cx: &mut TestAppContext) {
        let (opened, cx) = open(cx);
        let repo = opened.view.read_with(cx, |view, _| view.repo.clone());
        write(&repo, "added.txt", "new\n");
        git(&repo, &["add", "added.txt"]);
        opened
            .view
            .update_in(cx, |view, window, cx| view.on_repo_change(false, window, cx));
        cx.run_until_parked();
        assert!(sidebar_paths(&opened, cx).contains(&"added.txt".to_string()));
    }

    #[gpui::test]
    fn a_head_change_picks_up_the_new_branch(cx: &mut TestAppContext) {
        let (opened, cx) = open(cx);
        let (repo, first) = opened
            .view
            .read_with(cx, |view, _| (view.repo.clone(), view.current().id));
        git(&repo, &["stash", "-q", "-u"]);
        git(&repo, &["checkout", "-q", "main"]);
        opened
            .view
            .update_in(cx, |view, window, cx| view.on_repo_change(true, window, cx));
        cx.run_until_parked();
        let (branch, second) = opened.view.read_with(cx, |view, _| {
            (
                view.refs.as_ref().and_then(|refs| refs.checked_out.clone()),
                view.current().id,
            )
        });
        assert_eq!(branch.as_deref(), Some("main"));
        assert_ne!(first, second);
    }

    fn loaded(signature: &[(&str, &str)]) -> DiffLoaded {
        DiffLoaded {
            files: Vec::new(),
            signature: signature
                .iter()
                .map(|(path, print)| (path.to_string(), print.to_string()))
                .collect(),
            git_and_parse: Duration::ZERO,
            fingerprints: Duration::ZERO,
        }
    }

    #[gpui::test]
    fn a_diff_is_cleaned_of_stale_viewed_marks_until_the_cleanup_finishes(cx: &mut TestAppContext) {
        let (opened, cx) = open(cx);
        opened.view.read_with(cx, |view, _| {
            let session = view.current();
            assert!(session.cleared.is_some() && session.cleared == session.shown);
        });
        opened.view.update(cx, |view, cx| {
            let id = view.current().id;
            let next = [("base.txt", "changed")];
            assert!(view.apply_diff(id, loaded(&next), cx).is_some());
            assert!(
                view.apply_diff(id, loaded(&next), cx).is_some(),
                "the same diff arriving again, before its cleanup finished, still owes the cleanup"
            );
            let job = view.apply_diff(id, loaded(&next), cx).expect("still owed");
            view.finish_clear(id, job.signature, 1, HashSet::new(), cx);
            assert!(view.apply_diff(id, loaded(&next), cx).is_none());
        });
    }

    #[gpui::test]
    fn two_sessions_on_one_branch_share_one_set_of_viewed_marks(cx: &mut TestAppContext) {
        let (opened, cx) = open(cx);
        opened
            .view
            .update_in(cx, |view, window, cx| view.toggle_viewed("base.txt".into(), window, cx));
        opened.view.update_in(cx, |view, window, cx| {
            view.select_comparison(Comparison::Staged, window, cx)
        });
        cx.run_until_parked();
        let (first, second) = opened.view.read_with(cx, |view, _| {
            let session = view.current();
            (view.viewed_states.len(), session.viewed.clone().expect("viewed"))
        });
        assert_eq!(first, 1, "one owner for the feature branch");
        opened.view.update_in(cx, |view, window, cx| {
            view.toggle_viewed("src/staged.rs".into(), window, cx)
        });
        cx.run_until_parked();
        let key = opened.view.read_with(cx, |view, _| {
            ViewedKey::for_comparison(&view.repo, &Comparison::Staged, view.refs.as_ref())
        });
        let saved = opened.store.load(&key);
        assert!(
            saved.contains_key("base.txt") && saved.contains_key("src/staged.rs"),
            "{saved:?}"
        );
        assert_eq!(second.files.lock().unwrap().paths().len(), 2);
    }

    fn review_session(opened: &Opened, cx: &mut gpui::VisualTestContext) -> Entity<ReviewSession> {
        let pane = opened.view.read_with(cx, |view, _| view.current().pane.clone());
        pane.read_with(cx, |pane, _| pane.review().cloned().expect("a review session"))
    }

    fn run_review(opened: &Opened, cx: &mut gpui::VisualTestContext) {
        review_session(opened, cx).update(cx, |session, cx| session.start_review(cx));
        cx.run_until_parked();
    }

    #[gpui::test]
    fn review_findings_become_rows_in_the_pane_and_counts_in_the_sidebar(cx: &mut TestAppContext) {
        let (opened, cx) = open(cx);
        run_review(&opened, cx);
        let (sidebar, pane) = opened.view.read_with(cx, |view, _| {
            let session = view.current();
            (session.sidebar.clone(), session.pane.clone())
        });
        let counts = sidebar.read_with(cx, |sidebar, _| sidebar.comments().clone());
        assert_eq!(counts.get("base.txt"), Some(&1));
        assert_eq!(counts.get("src/staged.rs"), Some(&2));
        assert_eq!(counts.get("docs/committed.md"), Some(&1));
        assert_eq!(counts.get(""), Some(&1), "the summary belongs to the whole change");
        let slots = pane.read_with(cx, |pane, _| pane.state().blocks.len());
        assert_eq!(slots, 5);

        let session = review_session(&opened, cx);
        let first = session.read_with(cx, |session, _| session.threads().next().map(|thread| thread.id));
        session.update(cx, |session, cx| session.remove_thread(first.expect("a thread"), cx));
        let counts = sidebar.read_with(cx, |sidebar, _| sidebar.comments().clone());
        assert_eq!(counts.values().sum::<usize>(), 4);
        assert_eq!(pane.read_with(cx, |pane, _| pane.state().blocks.len()), 4);
    }

    #[gpui::test]
    fn the_session_knows_the_comparison_branch_and_files(cx: &mut TestAppContext) {
        let (opened, cx) = open(cx);
        let review = review_session(&opened, cx);
        review.read_with(cx, |session, _| {
            assert_eq!(session.host().comparison, Comparison::checked_out("main", true));
            assert_eq!(session.host().branch_name.as_deref(), Some("feature"));
            assert_eq!(session.file_count(), 3);
            assert!(session.host().visible_path.is_some());
        });
    }

    #[gpui::test]
    fn the_toolbar_compacts_while_a_review_runs_and_while_its_result_shows(cx: &mut TestAppContext) {
        let (opened, cx) = open(cx);
        let compact = |cx: &mut gpui::VisualTestContext| opened.view.read_with(cx, |view, _| view.compact_toolbar);
        assert!(!compact(cx));
        review_session(&opened, cx).update(cx, |session, cx| session.start_review(cx));
        assert!(compact(cx), "running");
        cx.run_until_parked();
        assert!(compact(cx), "the result shows for a few seconds");
        cx.executor()
            .advance_clock(Duration::from_secs(crate::review::pill::RESULT_SECONDS + 1));
        cx.run_until_parked();
        assert!(!compact(cx), "the result is gone");
    }

    #[gpui::test]
    fn the_pill_comments_on_the_top_file_and_the_pane_shows_the_thread(cx: &mut TestAppContext) {
        let (opened, cx) = open(cx);
        let (pill, pane) = opened.view.read_with(cx, |view, _| {
            let session = view.current();
            (session.pill.clone(), session.pane.clone())
        });
        pill.update(cx, |pill, cx| pill.comment_on_file(cx));
        cx.run_until_parked();
        let review = review_session(&opened, cx);
        let id = review.read_with(cx, |session, _| session.threads().next().map(|thread| thread.id));
        assert!(pane.read_with(cx, |pane, _| pane.card(id.expect("a thread")).is_some()));
    }
}
