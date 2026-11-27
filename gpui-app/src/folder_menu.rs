use crate::linked_folders::{LinkedEvent, LinkedFolders};
use crate::repo_view::tilde_path;
use diffpilot_core::comparison::{self, Comparison};
use diffpilot_core::folder_status::{FolderStatus, folder_status};
use diffpilot_core::refs::Refs;
use diffpilot_core::settings::Settings;
use diffpilot_ui_kit::components::menu::{MenuItem, MenuSection, menu_button};
use diffpilot_ui_kit::components::{BadgeTone, Button, Look, popover, status_badge, tag};
use diffpilot_ui_kit::{Glyph, Theme};
use gpui::prelude::*;
use gpui::{
    AnyElement, Context, Corner, Entity, EventEmitter, FontWeight, IntoElement, ParentElement, Render, SharedString,
    Styled, Subscription, Task, WeakEntity, Window, div, font, px,
};
use gpui_component::{ActiveTheme, h_flex, v_flex};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const MENU_WIDTH: f32 = 420.0;
const LINKED_WIDTH: f32 = 380.0;
pub const SHORTCUT_SLOTS: usize = 9;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderMenuEvent {
    Select(PathBuf),
    Open,
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

pub(crate) struct FolderSnapshot {
    pub refs: Refs,
    pub comparison: Comparison,
    pub status: Option<FolderStatus>,
}

pub(crate) fn snapshot(path: &Path, settings: &Settings) -> FolderSnapshot {
    let (refs, comparison) = comparison::load_comparison(path, settings);
    let status = folder_status(path, &comparison).ok();
    FolderSnapshot {
        refs,
        comparison,
        status,
    }
}

pub(crate) fn folder_name(path: &Path) -> String {
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
    editor: Entity<LinkedFolders>,
    linked_open: bool,
    _editor: Subscription,
}

impl EventEmitter<FolderMenuEvent> for FolderMenu {}

impl FolderMenu {
    pub fn new(settings: Arc<Settings>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let editor = cx.new(|cx| LinkedFolders::new(PathBuf::new(), Vec::new(), window, cx));
        let edits = cx.subscribe(&editor, |this, _, LinkedEvent::Changed(folders), cx| {
            if let Some(active) = &this.active {
                this.settings.set_linked_folders(active, folders.clone());
            }
            cx.notify();
        });
        Self {
            settings,
            rows: Vec::new(),
            active: None,
            refresh_task: None,
            editor,
            linked_open: false,
            _editor: edits,
        }
    }

    pub fn open_linked(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_linked_open(true, window, cx);
    }

    fn set_linked_open(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.linked_open == open {
            return;
        }
        let Some(active) = self.active.clone().filter(|_| open || self.linked_open) else {
            return;
        };
        self.linked_open = open;
        if open {
            let linked = self.settings.linked_folders(&active);
            self.editor.update(cx, |editor, cx| {
                editor.set_repo(active, linked, cx);
                editor.focus_input(window, cx);
            });
        }
        cx.notify();
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
        if self.active.as_deref() != Some(active) {
            self.linked_open = false;
        }
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
                            let status = snapshot(&path, &settings).status;
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

    fn open(&mut self, cx: &mut Context<Self>) {
        cx.emit(FolderMenuEvent::Open);
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
            .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SMALL))
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
        .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SMALL))
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

fn detail_line(branch: Option<String>, links: &[String]) -> Option<String> {
    let links = (!links.is_empty()).then(|| format!("links {}", links.join(", ")));
    match (branch, links) {
        (Some(branch), Some(links)) => Some(format!("{branch} · {links}")),
        (branch, links) => branch.or(links),
    }
}

fn row_item(
    index: usize,
    row: &FolderRow,
    links: &[String],
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
    let branch = row.status.as_ref().and_then(|status| status.branch.clone());
    let item = match detail_line(branch, links) {
        Some(detail) => item.detail(detail),
        None => item,
    };
    if index < SHORTCUT_SLOTS {
        item.shortcut(format!("⌃⌘{}", index + 1))
    } else {
        item
    }
}

impl FolderMenu {
    fn linked_pill(&self, name: &str, count: usize, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let muted = count == 0;
        let color = if muted { theme.ui.muted } else { theme.ui.text2 };
        let label = if muted {
            "Link folders".to_string()
        } else {
            format!("{count} linked")
        };
        let text = SharedString::from(label.clone());
        let trigger = tag(
            Button::new("linked-pill")
                .look(Look::Ghost)
                .rounded_full()
                .h(px(20.))
                .px(px(6.))
                .gap(px(4.))
                .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SMALL))
                .icon_color(Glyph::Link, 11., color)
                .text_color(color)
                .child(tag(div(), move || format!("linked-count:{label}")).child(text)),
            || "linked-pill",
        );
        let menu = cx.entity().downgrade();
        let editor = self.editor.clone();
        let (title, theme) = (format!("Linked to {name}"), *theme);
        popover("linked-popover")
            .anchor(Corner::TopLeft)
            .open(self.linked_open)
            .on_open_change(move |open, window, cx| {
                let open = *open;
                menu.update(cx, |menu, cx| menu.set_linked_open(open, window, cx)).ok();
            })
            .trigger(trigger)
            .content(move |_, _, _| {
                v_flex()
                    .w(px(LINKED_WIDTH))
                    .gap(px(10.))
                    .text_color(theme.ui.text)
                    .child(tag(
                        div()
                            .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::BODY))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title.clone()),
                        || "linked-title",
                    ))
                    .child(
                        div()
                            .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SECONDARY))
                            .text_color(theme.ui.text2)
                            .child("Claude can read these for context. They are not reviewed."),
                    )
                    .child(editor.clone())
                    .child(
                        div()
                            .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SMALL))
                            .text_color(theme.ui.muted)
                            .child("Saved as you edit. A Guide run that is running keeps its links; the next run uses the new ones."),
                    )
            })
            .into_any_element()
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
        let link_names: Vec<Vec<String>> = rows
            .iter()
            .map(|row| {
                self.settings
                    .linked_folders(&row.path)
                    .iter()
                    .map(|folder| folder_name(folder))
                    .collect()
            })
            .collect();
        let linked_count = active
            .as_deref()
            .map_or(0, |active| self.settings.linked_folders(active).len());
        let linked_pill = active
            .is_some()
            .then(|| self.linked_pill(&name, linked_count, &theme, cx));
        let has_active = active.is_some();

        let trigger = tag(
            Button::new("folder-title")
                .look(Look::Ghost)
                .regular()
                .rounded_full()
                .h(px(30.))
                .gap(px(6.))
                .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::TITLE))
                .font_weight(FontWeight::SEMIBOLD)
                .label(name.clone())
                .caret(true)
                .when_some(active.as_deref(), |button, path| button.tooltip(tilde_path(path))),
            || "folder-title",
        );

        let refresh = menu.clone();
        let menu_name = name.clone();
        let popover = menu_button("folder", trigger, move |_, _| {
            let folders = rows
                .iter()
                .enumerate()
                .map(|(index, row)| {
                    let is_active = active.as_ref() == Some(&row.path);
                    row_item(
                        index,
                        row,
                        &link_names[index],
                        is_active,
                        theme,
                        mono.clone(),
                        menu.clone(),
                    )
                    .into()
                })
                .collect();
            let open = {
                let menu = menu.clone();
                MenuItem::new("Open Folder…", move |_, cx| {
                    menu.update(cx, |menu, cx| menu.open(cx)).ok();
                })
                .shortcut("⌘O")
            };
            let mut actions = vec![open.into()];
            if has_active {
                let menu = menu.clone();
                actions.push(
                    MenuItem::new(format!("Linked Folders for {menu_name}…"), move |window, cx| {
                        menu.update(cx, |menu, cx| menu.open_linked(window, cx)).ok();
                    })
                    .shortcut("⌥⌘L")
                    .into(),
                );
            }
            vec![MenuSection::new(folders), MenuSection::new(actions)]
        })
        .min_width(MENU_WIDTH)
        .arrow()
        .on_open_change(move |open, _, cx| {
            if open {
                refresh.update(cx, |menu, cx| menu.refresh(cx)).ok();
            }
        });

        let changed = (others > 0).then(|| {
            h_flex()
                .gap(px(5.))
                .items_center()
                .flex_none()
                .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SMALL))
                .text_color(theme.ui.muted)
                .child(div().size(px(6.)).rounded_full().bg(theme.chrome.uncommitted))
                .child(if others == 1 {
                    "1 other changed".to_string()
                } else {
                    format!("{others} others changed")
                })
        });
        let separator = (linked_pill.is_some() && changed.is_some()).then(|| {
            div()
                .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SMALL))
                .text_color(theme.ui.muted)
                .child("·")
        });
        let has_extras = linked_pill.is_some() || changed.is_some();
        tag(v_flex(), || format!("folder-active:{name}"))
            .min_w_0()
            .child(popover)
            .when(has_extras, |this| {
                this.child(
                    h_flex()
                        .gap(px(4.))
                        .items_center()
                        .min_w_0()
                        .overflow_hidden()
                        .children(linked_pill)
                        .children(separator)
                        .children(changed),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::detail_line;

    #[test]
    fn the_detail_line_joins_the_branch_and_the_linked_folder_names() {
        let links = ["api".to_string(), "docs".to_string()];
        assert_eq!(
            detail_line(Some("main".into()), &links).as_deref(),
            Some("main · links api, docs")
        );
        assert_eq!(detail_line(None, &links[..1]).as_deref(), Some("links api"));
        assert_eq!(detail_line(Some("main".into()), &[]).as_deref(), Some("main"));
        assert_eq!(detail_line(None, &[]), None);
    }
}
