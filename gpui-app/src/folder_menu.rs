use diffpilot_core::comparison;
use diffpilot_core::folder_status::{FolderStatus, folder_status};
use diffpilot_core::settings::Settings;
use diffpilot_ui_kit::components::{BadgeTone, Button, Look, status_badge};
use diffpilot_ui_kit::{Glyph, Theme};
use gpui::prelude::*;
use gpui::{
    AnyElement, Context, Corner, EventEmitter, FontWeight, IntoElement, ParentElement, Render, SharedString, Styled,
    Task, WeakEntity, Window, div, font, px,
};
use gpui_component::popover::Popover;
use gpui_component::{ActiveTheme, Icon, h_flex, v_flex};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const MENU_WIDTH: f32 = 420.0;
const ROW_HEIGHT: f32 = 28.0;
pub const SHORTCUT_SLOTS: usize = 9;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderMenuEvent {
    Select(PathBuf),
    Add,
}

#[derive(Debug, Clone)]
struct FolderRow {
    path: PathBuf,
    name: SharedString,
    status: Option<FolderStatus>,
}

impl FolderRow {
    fn new(path: PathBuf) -> Self {
        Self {
            name: folder_name(&path).into(),
            path,
            status: None,
        }
    }

    fn has_changes(&self) -> bool {
        self.status.as_ref().is_some_and(|s| s.files > 0 || s.dirty)
    }
}

fn folder_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}

pub struct FolderMenu {
    settings: Arc<Settings>,
    rows: Vec<FolderRow>,
    active: Option<PathBuf>,
    open: bool,
    refresh_task: Option<Task<()>>,
}

impl EventEmitter<FolderMenuEvent> for FolderMenu {}

impl FolderMenu {
    pub fn new(settings: Arc<Settings>) -> Self {
        Self {
            settings,
            rows: Vec::new(),
            active: None,
            open: false,
            refresh_task: None,
        }
    }

    pub fn set_folders(&mut self, folders: Vec<PathBuf>, active: &Path, cx: &mut Context<Self>) {
        let rows = folders
            .into_iter()
            .map(|path| {
                self.rows
                    .iter()
                    .find(|row| row.path == path)
                    .cloned()
                    .unwrap_or_else(|| FolderRow::new(path))
            })
            .collect();
        self.rows = rows;
        self.active = Some(active.to_path_buf());
        self.refresh(cx);
        cx.notify();
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        let paths: Vec<PathBuf> = self.rows.iter().map(|row| row.path.clone()).collect();
        let settings = self.settings.clone();
        self.refresh_task = Some(cx.spawn(async move |this, cx| {
            let statuses = cx
                .background_executor()
                .spawn(async move {
                    paths
                        .into_iter()
                        .map(|path| {
                            let (_, comparison) = comparison::load_comparison(&path, &settings);
                            let status = folder_status(&path, &comparison).ok();
                            (path, status)
                        })
                        .collect::<Vec<_>>()
                })
                .await;
            this.update(cx, |this, cx| {
                for (path, status) in statuses {
                    if let Some(row) = this.rows.iter_mut().find(|row| row.path == path) {
                        row.status = status;
                    }
                }
                cx.notify();
            })
            .ok();
        }));
    }

    fn set_open(&mut self, open: bool, cx: &mut Context<Self>) {
        if self.open == open {
            return;
        }
        self.open = open;
        if open {
            self.refresh(cx);
        }
        cx.notify();
    }

    fn pick(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.open = false;
        cx.emit(FolderMenuEvent::Select(path));
        cx.notify();
    }

    fn add(&mut self, cx: &mut Context<Self>) {
        self.open = false;
        cx.emit(FolderMenuEvent::Add);
        cx.notify();
    }

    fn others_changed(&self) -> usize {
        self.rows
            .iter()
            .filter(|row| Some(&row.path) != self.active.as_ref() && row.has_changes())
            .count()
    }
}

fn stats(status: &FolderStatus, theme: &Theme, mono: SharedString) -> AnyElement {
    if status.files == 0 {
        return div()
            .text_size(px(11.5))
            .text_color(theme.ui.muted)
            .child("no changes")
            .into_any_element();
    }
    let noun = if status.files == 1 { "file" } else { "files" };
    h_flex()
        .gap_1()
        .text_size(px(11.5))
        .text_color(theme.ui.muted)
        .child(format!("{} {noun} ·", status.files))
        .child(
            h_flex()
                .gap_1()
                .font_family(mono)
                .child(div().text_color(theme.success).child(format!("+{}", status.added)))
                .child(div().text_color(theme.danger).child(format!("−{}", status.removed))),
        )
        .into_any_element()
}

fn row_element(
    index: usize,
    row: &FolderRow,
    active: bool,
    theme: &Theme,
    mono: &SharedString,
    menu: WeakEntity<FolderMenu>,
) -> AnyElement {
    let path = row.path.clone();
    h_flex()
        .id(("folder-row", index))
        .w_full()
        .h(px(ROW_HEIGHT))
        .px(px(8.))
        .gap(px(8.))
        .rounded(px(8.))
        .cursor_pointer()
        .text_size(px(13.))
        .text_color(theme.ui.text)
        .when(active, |this| this.bg(theme.press))
        .when(!active, |this| {
            let hover = theme.hover;
            this.hover(move |style| style.bg(hover))
        })
        .on_click(move |_, _, cx| {
            menu.update(cx, |menu, cx| menu.pick(path.clone(), cx)).ok();
        })
        .child(
            Icon::new(Glyph::FolderFilled)
                .size(px(16.))
                .flex_none()
                .text_color(theme.chrome.folder),
        )
        .child(div().flex_shrink().min_w_0().truncate().child(row.name.clone()))
        .children(
            row.status
                .as_ref()
                .and_then(|status| status.branch.clone())
                .map(|branch| {
                    div()
                        .flex_shrink()
                        .min_w_0()
                        .truncate()
                        .text_size(px(11.5))
                        .text_color(theme.ui.muted)
                        .child(branch)
                }),
        )
        .child(div().flex_1())
        .children(row.status.as_ref().map(|status| stats(status, theme, mono.clone())))
        .children(
            row.status
                .as_ref()
                .filter(|status| status.dirty)
                .map(|_| status_badge("M", BadgeTone::Uncommitted, font(mono.clone()), theme)),
        )
        .children((index < SHORTCUT_SLOTS).then(|| {
            div()
                .w(px(24.))
                .text_size(px(11.5))
                .text_color(theme.ui.muted)
                .child(format!("⌃⌘{}", index + 1))
        }))
        .into_any_element()
}

impl Render for FolderMenu {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let mono: SharedString = cx.theme().mono_font_family.clone();
        let menu = cx.entity().downgrade();
        let name = self.active.as_deref().map_or_else(String::new, folder_name);
        let others = self.others_changed();
        let rows = self.rows.clone();
        let active = self.active.clone();

        let trigger = Button::new("folder-title")
            .look(Look::Ghost)
            .regular()
            .rounded_full()
            .h(px(24.))
            .gap(px(6.))
            .text_size(px(15.))
            .font_weight(FontWeight::SEMIBOLD)
            .label(name)
            .caret(true);

        let popover = Popover::new("folder-popover")
            .anchor(Corner::TopLeft)
            .open(self.open)
            .on_open_change(cx.listener(|this, open: &bool, _, cx| this.set_open(*open, cx)))
            .trigger(trigger)
            .content(move |_, _, _| {
                let list = rows.iter().enumerate().map(|(index, row)| {
                    let is_active = active.as_ref() == Some(&row.path);
                    row_element(index, row, is_active, &theme, &mono, menu.clone())
                });
                let add = {
                    let menu = menu.clone();
                    h_flex()
                        .id("folder-add")
                        .w_full()
                        .h(px(ROW_HEIGHT))
                        .px(px(8.))
                        .rounded(px(8.))
                        .cursor_pointer()
                        .text_size(px(13.))
                        .text_color(theme.ui.text)
                        .hover({
                            let hover = theme.hover;
                            move |style| style.bg(hover)
                        })
                        .on_click(move |_, _, cx| {
                            menu.update(cx, |menu, cx| menu.add(cx)).ok();
                        })
                        .child("Add Folder…")
                        .child(div().flex_1())
                        .child(div().text_size(px(11.5)).text_color(theme.ui.muted).child("⌘O"))
                };
                v_flex()
                    .w(px(MENU_WIDTH))
                    .p(px(4.))
                    .gap(px(2.))
                    .children(list)
                    .child(div().h(px(1.)).bg(theme.ui.line).my(px(4.)))
                    .child(add)
            });

        h_flex()
            .gap(px(8.))
            .items_center()
            .child(popover)
            .when(others > 0, |this| {
                this.child(
                    h_flex()
                        .gap(px(5.))
                        .items_center()
                        .flex_none()
                        .text_size(px(11.5))
                        .text_color(theme.ui.muted)
                        .child(div().size(px(6.)).rounded_full().bg(theme.chrome.uncommitted))
                        .child(if others == 1 {
                            "1 other changed".to_string()
                        } else {
                            format!("{others} others changed")
                        }),
                )
            })
    }
}
