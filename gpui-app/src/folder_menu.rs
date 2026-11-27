use diffpilot_core::comparison;
use diffpilot_core::folder_status::{FolderStatus, folder_status};
use diffpilot_core::settings::Settings;
use diffpilot_ui_kit::components::menu::{MenuItem, MenuSection, menu_button};
use diffpilot_ui_kit::components::{BadgeTone, Button, Look, status_badge, tag};
use diffpilot_ui_kit::{Glyph, Theme};
use gpui::prelude::*;
use gpui::{
    AnyElement, Context, EventEmitter, FontWeight, IntoElement, ParentElement, Render, SharedString, Styled, Task,
    WeakEntity, Window, div, font, px,
};
use gpui_component::{ActiveTheme, h_flex};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const MENU_WIDTH: f32 = 420.0;
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
    refresh_task: Option<Task<()>>,
}

impl EventEmitter<FolderMenuEvent> for FolderMenu {}

impl FolderMenu {
    pub fn new(settings: Arc<Settings>) -> Self {
        Self {
            settings,
            rows: Vec::new(),
            active: None,
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

    fn pick(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        cx.emit(FolderMenuEvent::Select(path));
    }

    fn add(&mut self, cx: &mut Context<Self>) {
        cx.emit(FolderMenuEvent::Add);
    }

    fn others_changed(&self) -> usize {
        self.rows
            .iter()
            .filter(|row| Some(&row.path) != self.active.as_ref() && row.has_changes())
            .count()
    }
}

fn stats(status: &FolderStatus, theme: &Theme, mono: SharedString, highlighted: bool) -> AnyElement {
    let muted = if highlighted {
        theme.on_accent_dim
    } else {
        theme.ui.muted
    };
    if status.files == 0 {
        return div()
            .text_size(px(11.5))
            .text_color(muted)
            .child("no changes")
            .into_any_element();
    }
    let (added, removed) = if highlighted {
        (theme.on_accent, theme.on_accent)
    } else {
        (theme.success, theme.danger)
    };
    let noun = if status.files == 1 { "file" } else { "files" };
    h_flex()
        .gap_1()
        .text_size(px(11.5))
        .text_color(muted)
        .child(format!("{} {noun} ·", status.files))
        .child(
            h_flex()
                .gap_1()
                .font_family(mono)
                .child(div().text_color(added).child(format!("+{}", status.added)))
                .child(div().text_color(removed).child(format!("−{}", status.removed))),
        )
        .into_any_element()
}

fn row_item(
    index: usize,
    row: &FolderRow,
    active: bool,
    theme: Theme,
    mono: SharedString,
    menu: WeakEntity<FolderMenu>,
) -> MenuItem {
    let path = row.path.clone();
    let status = row.status.clone();
    let item = MenuItem::new(row.name.clone(), move |_, cx| {
        menu.update(cx, |menu, cx| menu.pick(path.clone(), cx)).ok();
    })
    .icon(Glyph::FolderFilled, Some(theme.chrome.folder))
    .checked(active)
    .accessory(move |highlighted| {
        let Some(status) = &status else {
            return div().into_any_element();
        };
        h_flex()
            .gap(px(8.))
            .child(stats(status, &theme, mono.clone(), highlighted))
            .when(status.dirty, |this| {
                this.child(status_badge("M", BadgeTone::Uncommitted, font(mono.clone()), &theme))
            })
            .into_any_element()
    });
    let item = match row.status.as_ref().and_then(|status| status.branch.clone()) {
        Some(branch) => item.detail(branch),
        None => item,
    };
    if index < SHORTCUT_SLOTS {
        item.shortcut(format!("⌃⌘{}", index + 1))
    } else {
        item
    }
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

        let trigger = tag(
            Button::new("folder-title")
                .look(Look::Ghost)
                .regular()
                .rounded_full()
                .h(px(30.))
                .gap(px(6.))
                .text_size(px(15.))
                .font_weight(FontWeight::SEMIBOLD)
                .label(name.clone())
                .caret(true),
            || "folder-title",
        );

        let refresh = menu.clone();
        let popover = menu_button("folder", trigger, move |_, _| {
            let folders = rows
                .iter()
                .enumerate()
                .map(|(index, row)| {
                    let is_active = active.as_ref() == Some(&row.path);
                    row_item(index, row, is_active, theme, mono.clone(), menu.clone()).into()
                })
                .collect();
            let add = {
                let menu = menu.clone();
                MenuItem::new("Add Folder…", move |_, cx| {
                    menu.update(cx, |menu, cx| menu.add(cx)).ok();
                })
                .shortcut("⌘O")
            };
            vec![MenuSection::new(folders), MenuSection::new(vec![add.into()])]
        })
        .min_width(MENU_WIDTH)
        .arrow()
        .on_open_change(move |open, _, cx| {
            if open {
                refresh.update(cx, |menu, cx| menu.refresh(cx)).ok();
            }
        });

        tag(h_flex(), || format!("folder-active:{name}"))
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
