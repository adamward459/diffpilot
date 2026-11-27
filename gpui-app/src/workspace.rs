use crate::bench::{self, Bench, Mark};
use crate::folder_menu::{FolderMenu, FolderMenuEvent, SHORTCUT_SLOTS};
use crate::folder_picker::pick_folders;
use crate::open_dialog::{OpenEvent, OpenFolderDialog};
use crate::remote::SyncKind;
use crate::repo_view::{Nav, RepoView};
use crate::settings::{self, OpenSettings, PageEvent, Section, SettingsPage};
use crate::tour::{self, Tour};
use diffpilot_core::diff::DiffMode;
use diffpilot_core::settings::Settings;
use diffpilot_core::store::{DataScope, Store};
use diffpilot_core::viewed::ViewedStore;
use diffpilot_ui_kit::Glyph;
use diffpilot_ui_kit::components::{Presence, dialog, tour as tour_overlay, tour_card};
use gpui::prelude::*;
use gpui::{
    App, ClickEvent, Context, Entity, FocusHandle, IntoElement, KeyDownEvent, Menu, MenuItem, MouseDownEvent,
    ParentElement, Render, Styled, Subscription, Window, WindowControlArea, actions, div, px,
};
use gpui_component::button::Button;
use gpui_component::{ActiveTheme, Icon, Sizable, StyledExt, h_flex, v_flex};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const MAX_LIVE_VIEWS: usize = 4;
const SETTINGS_WIDTH: f32 = 780.;
const SETTINGS_HEIGHT: f32 = 600.;
const SETTINGS_MARGIN: f32 = 32.;

actions!(
    diffpilot,
    [
        OpenFolder,
        ToggleSidebar,
        UnifiedDiff,
        SplitDiff,
        Fetch,
        Pull,
        Push,
        Quit,
        Folder1,
        Folder2,
        Folder3,
        Folder4,
        Folder5,
        Folder6,
        Folder7,
        Folder8,
        Folder9,
        ShowFiles,
        ShowGuide,
        ToggleGuide,
        NextChapter,
        PreviousChapter,
        ToggleInspector,
        ToggleClaudeArea,
        ToggleComments,
        ShowTour
    ]
);

pub fn init(cx: &mut App) {
    cx.bind_keys([
        gpui::KeyBinding::new("cmd-o", OpenFolder, None),
        gpui::KeyBinding::new("cmd-q", Quit, None),
        gpui::KeyBinding::new("ctrl-cmd-1", Folder1, None),
        gpui::KeyBinding::new("ctrl-cmd-2", Folder2, None),
        gpui::KeyBinding::new("ctrl-cmd-3", Folder3, None),
        gpui::KeyBinding::new("ctrl-cmd-4", Folder4, None),
        gpui::KeyBinding::new("ctrl-cmd-5", Folder5, None),
        gpui::KeyBinding::new("ctrl-cmd-6", Folder6, None),
        gpui::KeyBinding::new("ctrl-cmd-7", Folder7, None),
        gpui::KeyBinding::new("ctrl-cmd-8", Folder8, None),
        gpui::KeyBinding::new("ctrl-cmd-9", Folder9, None),
        gpui::KeyBinding::new("ctrl-cmd-s", ToggleSidebar, None),
        gpui::KeyBinding::new("shift-cmd-f", Fetch, None),
        gpui::KeyBinding::new("shift-cmd-l", Pull, None),
        gpui::KeyBinding::new("shift-cmd-u", Push, None),
        gpui::KeyBinding::new("cmd-1", ShowFiles, None),
        gpui::KeyBinding::new("cmd-2", ShowGuide, None),
        gpui::KeyBinding::new("alt-cmd-0", ToggleInspector, None),
        gpui::KeyBinding::new("shift-cmd-y", ToggleClaudeArea, None),
        gpui::KeyBinding::new("g", ToggleGuide, Some("!Input")),
        gpui::KeyBinding::new("c", ToggleComments, Some("!Input")),
        gpui::KeyBinding::new("]", NextChapter, Some("!Input")),
        gpui::KeyBinding::new("[", PreviousChapter, Some("!Input")),
    ]);
    crate::sidebar::bind_keys(cx);
    crate::diff_pane::bind_keys(cx);
    crate::review::init(cx);
    crate::open_dialog::init(cx);
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.set_menus(vec![
        Menu {
            name: "Diffpilot".into(),
            items: vec![
                MenuItem::action("Settings…", OpenSettings),
                MenuItem::separator(),
                MenuItem::action("Quit Diffpilot", Quit),
            ],
        },
        Menu {
            name: "File".into(),
            items: vec![MenuItem::action("Open Folder…", OpenFolder)],
        },
        Menu {
            name: "Branch".into(),
            items: vec![
                MenuItem::action("Fetch", Fetch),
                MenuItem::action("Pull", Pull),
                MenuItem::action("Push", Push),
            ],
        },
        Menu {
            name: "View".into(),
            items: vec![
                MenuItem::action("Toggle Sidebar", ToggleSidebar),
                MenuItem::separator(),
                MenuItem::action("Unified Diff", UnifiedDiff),
                MenuItem::action("Split Diff", SplitDiff),
                MenuItem::separator(),
                MenuItem::action("Show Files", ShowFiles),
                MenuItem::action("Show Guide", ShowGuide),
                MenuItem::action("Toggle Inspector", ToggleInspector),
                MenuItem::action("Toggle Claude Activity", ToggleClaudeArea),
                MenuItem::action("Toggle Comments", ToggleComments),
            ],
        },
        Menu {
            name: "Help".into(),
            items: vec![MenuItem::action("Show Tour", ShowTour)],
        },
    ]);
}

pub struct Workspace {
    settings: Arc<Settings>,
    viewed_store: Arc<ViewedStore>,
    menu: Entity<FolderMenu>,
    views: Vec<(PathBuf, Entity<RepoView>)>,
    review_store: Arc<Store>,
    benching: bool,
    settings_page: Option<(Entity<SettingsPage>, Subscription)>,
    settings_presence: Presence,
    open_dialog: Option<(Entity<OpenFolderDialog>, Subscription)>,
    focus: FocusHandle,
    tour: Tour,
    tour_focus: FocusHandle,
    tour_pending: bool,
    _menu_events: Subscription,
    _focus_lost: Subscription,
}

impl Workspace {
    pub fn new(
        settings: Arc<Settings>,
        viewed_store: Arc<ViewedStore>,
        review_store: Arc<Store>,
        folder: Option<PathBuf>,
        benching: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let menu = cx.new(|_| FolderMenu::new(settings.clone()));
        let menu_events = cx.subscribe_in(&menu, window, |this, _, event, window, cx| match event {
            FolderMenuEvent::Select(folder) => this.show_folder(folder.clone(), window, cx),
            FolderMenuEvent::Add => this.open_folder(window, cx),
        });
        let focus_lost = cx.on_focus_lost(window, |this, window, cx| this.restore_focus(window, cx));
        let mut this = Self {
            settings,
            viewed_store,
            menu,
            views: Vec::new(),
            review_store,
            benching,
            settings_page: None,
            settings_presence: Presence::new(false),
            open_dialog: None,
            focus: cx.focus_handle(),
            tour: Tour::default(),
            tour_focus: cx.focus_handle(),
            tour_pending: false,
            _menu_events: menu_events,
            _focus_lost: focus_lost,
        };
        window.focus(&this.focus);
        if let Some(folder) = folder {
            this.show_folder(folder, window, cx);
        }
        if benching {
            this.watch_bench(window, cx);
        }
        this
    }

    fn show_folder(&mut self, folder: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.settings_page = None;
        self.settings_presence = Presence::new(false);
        self.open_dialog = None;
        let folder = folder.canonicalize().unwrap_or(folder);
        self.settings.remember_folder(&folder);
        self.tour_pending = !self.benching && !self.settings.tour_seen();
        match self.views.iter().position(|(path, _)| *path == folder) {
            Some(index) => {
                let view = self.views.remove(index);
                self.views.insert(0, view);
            }
            None => {
                let view = self.new_view(&folder, window, cx);
                self.views.insert(0, (folder.clone(), view));
                self.evict_extra_views(cx);
            }
        }
        if let Some(name) = folder.file_name() {
            window.set_window_title(&name.to_string_lossy());
        }
        let folders = self.settings.folders();
        self.menu.update(cx, |menu, cx| menu.set_folders(folders, &folder, cx));
        cx.notify();
    }

    fn new_view(&self, folder: &Path, window: &mut Window, cx: &mut Context<Self>) -> Entity<RepoView> {
        let settings = self.settings.clone();
        let viewed_store = self.viewed_store.clone();
        let review_store = self.review_store.clone();
        let menu = self.menu.clone();
        let path = folder.to_path_buf();
        cx.new(|cx| {
            let mut view = RepoView::new(path, settings, viewed_store, review_store, window, cx);
            view.set_title(menu, cx);
            view
        })
    }

    /// A view caches its session id and threads, so after their rows are deleted it starts over from the store.
    fn rebuild_views(&mut self, scope: &DataScope, window: &mut Window, cx: &mut Context<Self>) {
        for index in 0..self.views.len() {
            let folder = self.views[index].0.clone();
            if scope.touches_folder(&folder) {
                self.views[index].1 = self.new_view(&folder, window, cx);
            }
        }
        cx.notify();
    }

    fn evict_extra_views(&mut self, cx: &App) {
        while self.views.len() > MAX_LIVE_VIEWS {
            let victim = (1..self.views.len())
                .rev()
                .find(|&index| !self.views[index].1.read(cx).review_running(cx))
                .unwrap_or(self.views.len() - 1);
            self.views.remove(victim);
        }
    }

    pub fn settings_page(&self) -> Option<&Entity<SettingsPage>> {
        self.settings_page
            .as_ref()
            .filter(|_| self.settings_presence.is_open())
            .map(|(page, _)| page)
    }

    pub fn active_view(&self) -> Option<&Entity<RepoView>> {
        self.views.first().map(|(_, view)| view)
    }

    fn switch_to_index(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index >= SHORTCUT_SLOTS {
            return;
        }
        if let Some(folder) = self.settings.folders().into_iter().nth(index) {
            self.show_folder(folder, window, cx);
        }
    }

    pub fn open_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = pick_folders(false, cx);
        cx.spawn_in(window, async move |this, cx| {
            let Some(folder) = picked.await.and_then(|paths| paths.into_iter().next()) else {
                return;
            };
            this.update_in(cx, |this, window, cx| this.show_open_dialog(folder, window, cx))
                .ok();
        })
        .detach();
    }

    fn show_open_dialog(&mut self, folder: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let settings = self.settings.clone();
        let dialog = cx.new(|cx| OpenFolderDialog::new(settings, folder, window, cx));
        let events = cx.subscribe_in(&dialog, window, |this, _, event: &OpenEvent, window, cx| match event {
            OpenEvent::Open(draft) => {
                this.settings.set_linked_folders(&draft.folder, draft.linked.clone());
                this.show_folder(draft.folder.clone(), window, cx);
            }
            OpenEvent::Cancel => window.focus(&this.focus),
        });
        self.open_dialog = Some((dialog, events));
        cx.notify();
    }

    pub fn open_settings(&mut self, section: Option<Section>, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(page) = self.settings_page().cloned() {
            page.update(cx, |page, cx| page.focus_search(window, cx));
            return;
        }
        if let Some((page, _)) = &self.settings_page
            && self.settings_presence.is_mounted()
        {
            page.update(cx, |page, cx| page.focus_search(window, cx));
            self.settings_presence
                .set_open(true, cx, |this| &mut this.settings_presence);
            return;
        }
        let Some(services) = settings::services(cx) else { return };
        let settings = self.settings.clone();
        let page = cx.new(|cx| SettingsPage::new(settings, services, section.unwrap_or(Section::General), window, cx));
        let closing = cx.subscribe_in(&page, window, |this, _, event: &PageEvent, window, cx| match event {
            PageEvent::Close => this.close_settings(window, cx),
            PageEvent::DataDeleted(scope) => this.rebuild_views(scope, window, cx),
        });
        self.settings_page = Some((page, closing));
        self.settings_presence = Presence::new(false);
        self.settings_presence
            .set_open(true, cx, |this| &mut this.settings_presence);
    }

    // The menu bar and every shortcut reach this view through the focused element, so focus must never rest on
    // an element that left the window.
    fn restore_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tour.is_open() {
            window.focus(&self.tour_focus);
            return;
        }
        match self.settings_page().cloned() {
            Some(page) => page.update(cx, |page, cx| page.focus_search(window, cx)),
            None => window.focus(&self.focus),
        }
    }

    fn start_tour(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.benching {
            self.tour_pending = false;
            return;
        }
        if !self.tour.start(&tour::shown(window, cx)) {
            return;
        }
        self.tour_pending = false;
        window.focus(&self.tour_focus);
        cx.notify();
    }

    fn move_tour(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let shown = tour::shown(window, cx);
        if forward && self.tour.is_last(&shown) {
            self.close_tour(window, cx);
            return;
        }
        if forward {
            self.tour.next(&shown);
        } else {
            self.tour.back(&shown);
        }
        cx.notify();
    }

    fn close_tour(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.tour.is_open() {
            return;
        }
        self.tour.close();
        self.settings.set_tour_seen(true);
        window.focus(&self.focus);
        cx.notify();
    }

    fn tour_overlay(&self, shown: &[tour::Stop], cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let stop = self.tour.stop()?;
        let (place, total) = self.tour.position(shown)?;
        let card = tour_card(
            stop.title(),
            stop.body(),
            format!("{place} of {total}"),
            cx.listener(|this, _: &ClickEvent, window, cx| this.move_tour(false, window, cx)),
            cx.listener(|this, _: &ClickEvent, window, cx| this.move_tour(true, window, cx)),
            cx.listener(|this, _: &ClickEvent, window, cx| this.close_tour(window, cx)),
        )
        .can_go_back(!self.tour.is_first(shown))
        .next_label(if self.tour.is_last(shown) { "Done" } else { "Next" });
        Some(
            tour_overlay("tour", self.tour.step()?)
                .target(move |window, cx| tour::bounds_of(stop, window, cx))
                .radius(stop.radius())
                .track_focus(&self.tour_focus)
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    if event.keystroke.modifiers.modified() {
                        return;
                    }
                    match event.keystroke.key.as_str() {
                        "right" | "enter" => this.move_tour(true, window, cx),
                        "left" => this.move_tour(false, window, cx),
                        "escape" => this.close_tour(window, cx),
                        _ => {}
                    }
                }))
                .on_dismiss(cx.listener(|this, _: &MouseDownEvent, window, cx| this.close_tour(window, cx)))
                .card(card)
                .into_any_element(),
        )
    }

    fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings_page().is_none() {
            return;
        }
        self.settings_presence
            .set_open(false, cx, |this| &mut this.settings_presence);
        window.focus(&self.focus);
        if let Some(repo) = self.active_view() {
            repo.read(cx).focus_sidebar(window, cx);
        }
    }

    fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        if let Some(repo) = self.active_view() {
            repo.update(cx, |repo, cx| repo.toggle_sidebar(cx));
        }
    }

    fn run_remote(&mut self, kind: SyncKind, cx: &mut Context<Self>) {
        if let Some(repo) = self.active_view() {
            repo.update(cx, |repo, cx| repo.run_remote(kind, cx));
        }
    }

    fn show_nav(&mut self, nav: Nav, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(repo) = self.active_view() {
            repo.update(cx, |repo, cx| repo.show_nav(nav, window, cx));
        }
    }

    fn set_diff_mode(&mut self, mode: DiffMode, cx: &mut Context<Self>) {
        if let Some(repo) = self.active_view() {
            repo.update(cx, |repo, cx| repo.set_diff_mode(mode, cx));
        }
    }

    fn watch_bench(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.entity();
        window.on_next_frame(move |window, cx| {
            Bench::mark_after_paint(cx, Mark::WindowShown);
            wait_for_bench(this, 0, window, cx);
        });
    }

    fn settings_dialog(&self, page: &Entity<SettingsPage>, cx: &mut Context<Self>) -> gpui::AnyElement {
        dialog("settings", &self.settings_presence)
            .tags("settings:backdrop", "settings:dialog")
            .size(SETTINGS_WIDTH, SETTINGS_HEIGHT)
            .margin(SETTINGS_MARGIN)
            .on_backdrop_click(cx.listener(|this, _, window, cx| this.close_settings(window, cx)))
            .child(page.clone())
            .into_any_element()
    }

    fn empty_state(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (sidebar, border, muted) = (theme.sidebar, theme.sidebar_border, theme.muted_foreground);
        h_flex()
            .size_full()
            .bg(theme.background)
            .child(
                v_flex()
                    .id("empty-sidebar")
                    .w(px(272.))
                    .h_full()
                    .flex_shrink_0()
                    .bg(sidebar)
                    .border_r_1()
                    .border_color(border)
                    .child(div().h(px(38.)).w_full().window_control_area(WindowControlArea::Drag))
                    .child(
                        v_flex()
                            .flex_1()
                            .items_center()
                            .justify_center()
                            .gap_2()
                            .px_6()
                            .child(Icon::new(Glyph::Folder).size(px(36.)).text_color(muted))
                            .child(
                                div()
                                    .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::TITLE))
                                    .font_semibold()
                                    .child("No Folder"),
                            )
                            .child(
                                div()
                                    .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::BODY))
                                    .text_color(muted)
                                    .text_center()
                                    .child("Press ⌘O to open a Git repository."),
                            )
                            .child(
                                Button::new("open-folder")
                                    .small()
                                    .label("Open Folder…")
                                    .mt_2()
                                    .on_click(cx.listener(|this, _, window, cx| this.open_folder(window, cx))),
                            ),
                    ),
            )
            .child(div().flex_1().h_full().window_control_area(WindowControlArea::Drag))
    }
}

fn wait_for_bench(workspace: Entity<Workspace>, settle: usize, window: &mut Window, cx: &mut App) {
    let Some(bench) = Bench::get(cx) else { return };
    let ready = bench.ready_to_scroll();
    let settle = if ready { settle + 1 } else { 0 };
    if settle >= 60 {
        let scroll = workspace
            .read(cx)
            .active_view()
            .and_then(|repo| repo.read(cx).sidebar_scroll(cx));
        if let Some((handle, rows)) = scroll {
            bench.begin_scroll(handle, rows);
            bench.run_scroll(window);
            return;
        }
    }
    window.on_next_frame(move |window, cx| wait_for_bench(workspace, settle, window, cx));
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let shown = tour::shown(window, cx);
        if self.tour_pending {
            self.start_tour(window, cx);
            if self.tour_pending {
                window.request_animation_frame();
            }
        }
        tour::begin_frame(window, cx);
        let tour = self.tour_overlay(&shown, cx);
        let content = match self.active_view() {
            Some(repo) => repo.clone().into_any_element(),
            None => self.empty_state(cx).into_any_element(),
        };
        let dialog = self
            .settings_page
            .as_ref()
            .filter(|_| self.settings_presence.is_mounted())
            .map(|(page, _)| self.settings_dialog(page, cx));
        let open_dialog = self
            .open_dialog
            .as_ref()
            .filter(|(dialog, _)| dialog.read(cx).is_mounted())
            .map(|(dialog, _)| dialog.clone());
        let app = div()
            .id("workspace")
            .size_full()
            .on_action(cx.listener(|this, _: &OpenFolder, window, cx| this.open_folder(window, cx)))
            .on_action(cx.listener(|this, _: &OpenSettings, window, cx| this.open_settings(None, window, cx)))
            .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| this.toggle_sidebar(cx)))
            .on_action(cx.listener(|this, _: &UnifiedDiff, _, cx| this.set_diff_mode(DiffMode::Unified, cx)))
            .on_action(cx.listener(|this, _: &SplitDiff, _, cx| this.set_diff_mode(DiffMode::Split, cx)))
            .on_action(cx.listener(|this, _: &Fetch, _, cx| this.run_remote(SyncKind::Fetch, cx)))
            .on_action(cx.listener(|this, _: &Pull, _, cx| this.run_remote(SyncKind::Pull, cx)))
            .on_action(cx.listener(|this, _: &Push, _, cx| this.run_remote(SyncKind::Push, cx)))
            .on_action(cx.listener(|this, _: &ShowFiles, window, cx| this.show_nav(Nav::Files, window, cx)))
            .on_action(cx.listener(|this, _: &ShowGuide, window, cx| this.show_nav(Nav::Guide, window, cx)))
            .on_action(cx.listener(|this, _: &ToggleGuide, window, cx| {
                if let Some(repo) = this.active_view() {
                    repo.update(cx, |repo, cx| repo.toggle_guide(window, cx));
                }
            }))
            .on_action(cx.listener(|this, _: &NextChapter, _, cx| {
                if let Some(repo) = this.active_view() {
                    repo.update(cx, |repo, cx| repo.step_chapter(1, cx));
                }
            }))
            .on_action(cx.listener(|this, _: &PreviousChapter, _, cx| {
                if let Some(repo) = this.active_view() {
                    repo.update(cx, |repo, cx| repo.step_chapter(-1, cx));
                }
            }))
            .on_action(cx.listener(|this, _: &ToggleInspector, _, cx| {
                if let Some(repo) = this.active_view() {
                    repo.update(cx, |repo, cx| repo.toggle_inspector(cx));
                }
            }))
            .on_action(cx.listener(|this, _: &ToggleClaudeArea, _, cx| {
                if let Some(repo) = this.active_view() {
                    repo.update(cx, |repo, cx| repo.toggle_claude_area(cx));
                }
            }))
            .on_action(cx.listener(|this, _: &ToggleComments, window, cx| {
                if let Some(repo) = this.active_view() {
                    repo.update(cx, |repo, cx| repo.toggle_comments(window, cx));
                }
            }))
            .on_action(cx.listener(|this, _: &Folder1, window, cx| this.switch_to_index(0, window, cx)))
            .on_action(cx.listener(|this, _: &Folder2, window, cx| this.switch_to_index(1, window, cx)))
            .on_action(cx.listener(|this, _: &Folder3, window, cx| this.switch_to_index(2, window, cx)))
            .on_action(cx.listener(|this, _: &Folder4, window, cx| this.switch_to_index(3, window, cx)))
            .on_action(cx.listener(|this, _: &Folder5, window, cx| this.switch_to_index(4, window, cx)))
            .on_action(cx.listener(|this, _: &Folder6, window, cx| this.switch_to_index(5, window, cx)))
            .on_action(cx.listener(|this, _: &Folder7, window, cx| this.switch_to_index(6, window, cx)))
            .on_action(cx.listener(|this, _: &Folder8, window, cx| this.switch_to_index(7, window, cx)))
            .on_action(cx.listener(|this, _: &Folder9, window, cx| this.switch_to_index(8, window, cx)))
            .child(div().size_0().track_focus(&self.focus))
            .child(content)
            .children(dialog)
            .children(open_dialog)
            .when(self.benching, |this| this.child(bench::probe()));
        div()
            .id("workspace-root")
            .relative()
            .size_full()
            .on_action(cx.listener(|this, _: &ShowTour, window, cx| this.start_tour(window, cx)))
            .child(app)
            .children(tour)
    }
}
