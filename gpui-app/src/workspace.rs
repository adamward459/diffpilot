use crate::bench::{self, Bench, Mark};
use crate::glyph::Glyph;
use crate::repo_view::{Claude, RepoView};
use diffpilot_core::settings::Settings;
use diffpilot_core::viewed::ViewedStore;
use gpui::prelude::*;
use gpui::{
    App, Context, Entity, IntoElement, Menu, MenuItem, ParentElement, PathPromptOptions, Render, Styled, Window,
    WindowControlArea, actions, div, px,
};
use gpui_component::button::Button;
use gpui_component::{ActiveTheme, Icon, Sizable, StyledExt, h_flex, v_flex};
use std::path::PathBuf;
use std::sync::Arc;

actions!(diffpilot, [OpenFolder, ToggleSidebar, Quit]);

pub fn init(cx: &mut App) {
    cx.bind_keys([
        gpui::KeyBinding::new("cmd-o", OpenFolder, None),
        gpui::KeyBinding::new("cmd-q", Quit, None),
        gpui::KeyBinding::new("ctrl-cmd-s", ToggleSidebar, None),
    ]);
    crate::sidebar::bind_keys(cx);
    diffpilot_review_ui::init(cx);
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.set_menus(vec![
        Menu {
            name: "diffpilot".into(),
            items: vec![MenuItem::action("Quit diffpilot", Quit)],
        },
        Menu {
            name: "File".into(),
            items: vec![MenuItem::action("Open Folder…", OpenFolder)],
        },
        Menu {
            name: "View".into(),
            items: vec![MenuItem::action("Toggle Sidebar", ToggleSidebar)],
        },
    ]);
}

pub struct Workspace {
    settings: Arc<Settings>,
    viewed_store: Arc<ViewedStore>,
    repo: Option<Entity<RepoView>>,
    benching: bool,
}

impl Workspace {
    pub fn new(
        settings: Arc<Settings>,
        viewed_store: Arc<ViewedStore>,
        folder: Option<PathBuf>,
        benching: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self {
            settings,
            viewed_store,
            repo: None,
            benching,
        };
        if let Some(folder) = folder {
            this.show_folder(folder, window, cx);
        }
        if benching {
            this.watch_bench(window, cx);
        }
        this
    }

    fn show_folder(&mut self, folder: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let settings = self.settings.clone();
        let viewed_store = self.viewed_store.clone();
        let claude = Claude::from_env(&folder);
        self.repo = Some(cx.new(|cx| RepoView::new(folder, settings, viewed_store, claude, window, cx)));
        cx.notify();
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
            this.update_in(cx, |this, window, cx| {
                this.settings.remember_folder(&folder);
                this.show_folder(folder, window, cx);
            })
            .ok();
        })
        .detach();
    }

    fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        if let Some(repo) = &self.repo {
            repo.update(cx, |repo, cx| repo.toggle_sidebar(cx));
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
            .repo
            .as_ref()
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
        let content = match &self.repo {
            Some(repo) => repo.clone().into_any_element(),
            None => self.empty_state(cx).into_any_element(),
        };
        div()
            .id("workspace")
            .size_full()
            .on_action(cx.listener(|this, _: &OpenFolder, window, cx| this.open_folder(window, cx)))
            .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| this.toggle_sidebar(cx)))
            .child(content)
            .when(self.benching, |this| this.child(bench::probe()))
    }
}
