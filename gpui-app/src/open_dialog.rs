use crate::folder_picker::pick_folders;
use crate::linked_folders::{LinkedEvent, LinkedFolders};
use crate::repo_view::tilde_path;
use diffpilot_core::settings::Settings;
use diffpilot_ui_kit::components::{Button, Look, Presence, dialog, glyph_icon, tag};
use diffpilot_ui_kit::{Glyph, Theme};
use gpui::prelude::*;
use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, FontWeight, KeyBinding, SharedString, Subscription, Window,
    actions, div, px,
};
use gpui_component::{h_flex, v_flex};
use std::path::PathBuf;
use std::sync::Arc;

const WIDTH: f32 = 520.;
const HEIGHT: f32 = 480.;
const CONTEXT: &str = "OpenFolderDialog";

actions!(diffpilot_open, [ConfirmOpen, CancelOpen]);

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("enter", ConfirmOpen, Some(CONTEXT)),
        KeyBinding::new("escape", CancelOpen, Some(CONTEXT)),
    ]);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenDraft {
    pub folder: PathBuf,
    pub linked: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenEvent {
    Open(OpenDraft),
    Cancel,
}

pub struct OpenFolderDialog {
    settings: Arc<Settings>,
    draft: OpenDraft,
    editor: Entity<LinkedFolders>,
    presence: Presence,
    focus: FocusHandle,
    _editor: Subscription,
}

impl EventEmitter<OpenEvent> for OpenFolderDialog {}

impl OpenFolderDialog {
    pub fn new(settings: Arc<Settings>, folder: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let folder = folder.canonicalize().unwrap_or(folder);
        let linked = settings.linked_folders(&folder);
        let editor = {
            let (folder, linked) = (folder.clone(), linked.clone());
            cx.new(|cx| LinkedFolders::pinned(folder, linked, window, cx))
        };
        let edits = cx.subscribe(&editor, |this, _, LinkedEvent::Changed(folders), _| {
            this.draft.linked = folders.clone();
        });
        let focus = cx.focus_handle();
        window.focus(&focus);
        let mut presence = Presence::new(false);
        presence.set_open(true, cx, |this: &mut Self| &mut this.presence);
        Self {
            settings,
            draft: OpenDraft { folder, linked },
            editor,
            presence,
            focus,
            _editor: edits,
        }
    }

    pub fn is_mounted(&self) -> bool {
        self.presence.is_mounted()
    }

    fn open(&mut self, cx: &mut Context<Self>) {
        cx.emit(OpenEvent::Open(self.draft.clone()));
    }

    fn cancel(&mut self, cx: &mut Context<Self>) {
        if !self.presence.is_open() {
            return;
        }
        self.presence.set_open(false, cx, |this| &mut this.presence);
        cx.emit(OpenEvent::Cancel);
    }

    fn change(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = pick_folders(false, cx);
        cx.spawn_in(window, async move |this, cx| {
            let Some(folder) = picked.await.and_then(|paths| paths.into_iter().next()) else {
                return;
            };
            this.update(cx, |this, cx| {
                let folder = folder.canonicalize().unwrap_or(folder);
                let linked = this.settings.linked_folders(&folder);
                this.draft = OpenDraft {
                    folder: folder.clone(),
                    linked: linked.clone(),
                };
                this.editor.update(cx, |editor, cx| editor.set_repo(folder, linked, cx));
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

impl Render for OpenFolderDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Theme::of(cx);
        let name = self
            .draft
            .folder
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let shown = tilde_path(&self.draft.folder);
        let content = v_flex()
            .id("open-folder-content")
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .size_full()
            .p(px(20.))
            .gap(px(14.))
            .on_action(cx.listener(|this, _: &ConfirmOpen, _, cx| this.open(cx)))
            .on_action(cx.listener(|this, _: &CancelOpen, _, cx| this.cancel(cx)))
            .child(
                div()
                    .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::TITLE))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Open Folder"),
            )
            .child(
                h_flex()
                    .gap(px(10.))
                    .child(glyph_icon(Glyph::Folder, 18., tokens.ui.text2))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                tag(div().font_weight(FontWeight::MEDIUM), || "open-folder-name")
                                    .child(SharedString::from(name)),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .font_family("Menlo")
                                    .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SMALL))
                                    .text_color(tokens.ui.muted)
                                    .child(SharedString::from(shown)),
                            ),
                    )
                    .child(tag(
                        Button::new("open-folder-change")
                            .label("Change…")
                            .on_click(cx.listener(|this, _, window, cx| this.change(window, cx))),
                        || "open-folder-change",
                    )),
            )
            .child(
                div()
                    .id("open-folder-linked")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(self.editor.clone()),
            )
            .child(
                h_flex()
                    .gap(px(8.))
                    .justify_end()
                    .child(tag(
                        Button::new("open-folder-cancel")
                            .label("Cancel")
                            .on_click(cx.listener(|this, _, _, cx| this.cancel(cx))),
                        || "open-folder-cancel",
                    ))
                    .child(tag(
                        Button::new("open-folder-confirm")
                            .look(Look::Primary)
                            .label("Open")
                            .on_click(cx.listener(|this, _, _, cx| this.open(cx))),
                        || "open-folder-confirm",
                    )),
            );
        dialog("open-folder", &self.presence)
            .tags("open-folder:backdrop", "open-folder:dialog")
            .size(WIDTH, HEIGHT)
            .on_backdrop_click(cx.listener(|this, _, _, cx| this.cancel(cx)))
            .child(content)
    }
}
