use crate::bench::{self, Bench, Mark};
use crate::folder_menu::{FolderMenu, FolderMenuEvent, SHORTCUT_SLOTS};
use crate::remote::SyncKind;
use crate::repo_view::{Nav, RepoView};
use crate::settings::{self, OpenSettings, PageEvent, Section, SettingsPage};
use diffpilot_core::diff::DiffMode;
use diffpilot_core::settings::Settings;
use diffpilot_core::store::{DataScope, Store};
use diffpilot_core::viewed::ViewedStore;
use diffpilot_ui_kit::Glyph;
use gpui::prelude::*;
use gpui::{
    App, Context, Entity, FocusHandle, IntoElement, Menu, MenuItem, ParentElement, PathPromptOptions, Render, Styled,
    Subscription, Window, WindowControlArea, actions, div, px,
};
use gpui_component::button::Button;
use gpui_component::{ActiveTheme, Icon, Sizable, StyledExt, h_flex, v_flex};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const MAX_LIVE_VIEWS: usize = 4;

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
        ToggleClaudeArea
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
        gpui::KeyBinding::new("]", NextChapter, Some("!Input")),
        gpui::KeyBinding::new("[", PreviousChapter, Some("!Input")),
    ]);
    crate::sidebar::bind_keys(cx);
    crate::diff_pane::bind_keys(cx);
    crate::review::init(cx);
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
            ],
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
    focus: FocusHandle,
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
            focus: cx.focus_handle(),
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
        let folder = folder.canonicalize().unwrap_or(folder);
        self.settings.remember_folder(&folder);
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
        self.settings_page.as_ref().map(|(page, _)| page)
    }

    fn active_view(&self) -> Option<&Entity<RepoView>> {
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
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: None,
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else { return };
            let Some(folder) = paths.into_iter().next() else { return };
            this.update_in(cx, |this, window, cx| this.show_folder(folder, window, cx))
                .ok();
        })
        .detach();
    }

    pub fn open_settings(&mut self, section: Option<Section>, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((page, _)) = &self.settings_page {
            page.update(cx, |page, cx| page.focus_search(window, cx));
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
        cx.notify();
    }

    // The menu bar and every shortcut reach this view through the focused element, so focus must never rest on
    // an element that left the window.
    fn restore_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match &self.settings_page {
            Some((page, _)) => page.update(cx, |page, cx| page.focus_search(window, cx)),
            None => window.focus(&self.focus),
        }
    }

    fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings_page.take().is_none() {
            return;
        }
        window.focus(&self.focus);
        if let Some(repo) = self.active_view() {
            repo.read(cx).focus_sidebar(window, cx);
        }
        cx.notify();
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
                            .child(div().text_size(px(17.)).font_semibold().child("No Folder"))
                            .child(
                                div()
                                    .text_size(px(13.))
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match (&self.settings_page, self.active_view()) {
            (Some((page, _)), _) => page.clone().into_any_element(),
            (None, Some(repo)) => repo.clone().into_any_element(),
            (None, None) => self.empty_state(cx).into_any_element(),
        };
        div()
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
            .when(self.benching, |this| this.child(bench::probe()))
    }
}
