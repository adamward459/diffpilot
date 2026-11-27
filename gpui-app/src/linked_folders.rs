use crate::folder_picker::pick_folders;
use crate::repo_view::tilde_path;
use diffpilot_core::guide::linked::{self, LinkError};
use diffpilot_ui_kit::components::{Button, Look, glyph_icon, tag};
use diffpilot_ui_kit::{Glyph, Theme};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AppContext as _, Context, Entity, EventEmitter, FontWeight, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, Styled as _, Subscription, Window, div, px,
};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{h_flex, v_flex};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkedEvent {
    Changed(Vec<PathBuf>),
}

pub struct LinkedFolders {
    repo: PathBuf,
    folders: Vec<PathBuf>,
    pinned: bool,
    form: Option<LinkForm>,
    refused: Vec<(String, LinkError)>,
}

struct LinkForm {
    input: Entity<InputState>,
    error: Option<LinkError>,
    _input: Subscription,
}

impl EventEmitter<LinkedEvent> for LinkedFolders {}

impl LinkedFolders {
    pub fn new(repo: PathBuf, folders: Vec<PathBuf>) -> Self {
        Self {
            repo,
            folders,
            pinned: false,
            form: None,
            refused: Vec::new(),
        }
    }

    pub fn pinned(repo: PathBuf, folders: Vec<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut this = Self::new(repo, folders);
        this.pinned = true;
        this.form = Some(this.new_form(window, cx));
        this
    }

    pub fn set_folders(&mut self, folders: Vec<PathBuf>, cx: &mut Context<Self>) {
        if self.folders != folders {
            self.folders = folders;
            cx.notify();
        }
    }

    pub fn set_repo(&mut self, repo: PathBuf, folders: Vec<PathBuf>, cx: &mut Context<Self>) {
        self.repo = repo;
        self.folders = folders;
        self.refused.clear();
        cx.notify();
    }

    fn new_form(&self, window: &mut Window, cx: &mut Context<Self>) -> LinkForm {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("~/Developer/shared-lib"));
        let subscription = cx.subscribe_in(&input, window, |this, _, event: &InputEvent, window, cx| match event {
            InputEvent::PressEnter { .. } => this.submit_link(window, cx),
            InputEvent::Change => {
                if let Some(form) = this.form.as_mut() {
                    form.error = None;
                }
                cx.notify();
            }
            _ => {}
        });
        LinkForm {
            input,
            error: None,
            _input: subscription,
        }
    }

    fn open_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let form = self.new_form(window, cx);
        form.input.update(cx, |input, cx| input.focus(window, cx));
        self.form = Some(form);
        cx.notify();
    }

    fn close_form(&mut self, cx: &mut Context<Self>) {
        self.form = None;
        cx.notify();
    }

    fn changed(&self, cx: &mut Context<Self>) {
        cx.emit(LinkedEvent::Changed(self.folders.clone()));
        cx.notify();
    }

    fn submit_link(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.form.as_mut() else { return };
        let text = form.input.read(cx).value().to_string();
        match linked::check(&self.repo, &self.folders, &text) {
            Ok(folder) => {
                self.folders.push(folder);
                if self.pinned {
                    form.input.update(cx, |input, cx| input.set_value("", window, cx));
                } else {
                    self.form = None;
                }
                self.refused.clear();
                self.changed(cx);
            }
            Err(error) => {
                form.error = Some(error);
                cx.notify();
            }
        }
    }

    fn unlink(&mut self, folder: &PathBuf, cx: &mut Context<Self>) {
        self.folders.retain(|linked| linked != folder);
        self.refused.clear();
        self.changed(cx);
    }

    fn add_folders(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = pick_folders(true, cx);
        cx.spawn_in(window, async move |this, cx| {
            let Some(paths) = picked.await else { return };
            this.update(cx, |this, cx| {
                let added = linked::add_all(&this.repo, &this.folders, &paths);
                this.refused = added
                    .refused
                    .into_iter()
                    .map(|(path, error)| (tilde_path(&path), error))
                    .collect();
                this.folders = added.folders;
                this.changed(cx);
            })
            .ok();
        })
        .detach();
    }
}

impl Render for LinkedFolders {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Theme::of(cx);
        let mut section = v_flex()
            .id("guide-linked")
            .gap(px(6.))
            .child(
                div()
                    .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SMALL))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(tokens.ui.muted)
                    .child("LINKED FOLDERS"),
            )
            .child(
                div()
                    .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SECONDARY))
                    .text_color(tokens.ui.text2)
                    .child("Claude can read these for context. They are not reviewed."),
            );
        for (position, folder) in self.folders.iter().enumerate() {
            let missing = !folder.is_dir();
            let color = if missing { tokens.amber } else { tokens.ui.text2 };
            let shown = tilde_path(folder);
            let target = folder.clone();
            section = section.child(
                tag(h_flex(), || format!("guide-linked-folder:{shown}"))
                    .gap(px(6.))
                    .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SECONDARY))
                    .child(glyph_icon(
                        if missing { Glyph::TriangleAlert } else { Glyph::Folder },
                        13.,
                        color,
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_family("Menlo")
                            .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SMALL))
                            .text_color(color)
                            .child(SharedString::from(shown.clone())),
                    )
                    .when(missing, |row| {
                        row.child(div().text_color(tokens.amber).child("Not found. Skipped."))
                    })
                    .child(
                        Button::new(("guide-unlink", position))
                            .look(Look::Ghost)
                            .label("Unlink")
                            .tooltip(format!("Unlink {shown}"))
                            .on_click(cx.listener(move |this, _, _, cx| this.unlink(&target, cx))),
                    ),
            );
        }
        for (shown, error) in &self.refused {
            section = section.child(
                tag(h_flex(), || format!("guide-link-refused:{shown}"))
                    .gap(px(6.))
                    .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SECONDARY))
                    .text_color(tokens.amber)
                    .child(glyph_icon(Glyph::TriangleAlert, 13., tokens.amber))
                    .child(SharedString::from(format!("{shown}: {error}"))),
            );
        }
        let add = tag(
            Button::new("guide-add-folders")
                .label("Add Folders…")
                .on_click(cx.listener(|this, _, window, cx| this.add_folders(window, cx))),
            || "guide-add-folders",
        );
        let Some(form) = &self.form else {
            return section
                .child(
                    h_flex().gap(px(6.)).child(add).child(tag(
                        Button::new("guide-link-folder")
                            .label("+ Link by Path…")
                            .on_click(cx.listener(|this, _, window, cx| this.open_form(window, cx))),
                        || "guide-link-folder",
                    )),
                )
                .into_any_element();
        };
        let empty = form.input.read(cx).value().trim().is_empty();
        let pinned = self.pinned;
        section
            .child(tag(div().w_full(), || "guide-link-input").child(
                Input::new(&form.input).text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SECONDARY)),
            ))
            .when_some(form.error, |section, error| {
                section.child(
                    tag(h_flex(), || "guide-link-error")
                        .gap(px(6.))
                        .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SECONDARY))
                        .text_color(tokens.amber)
                        .child(glyph_icon(Glyph::TriangleAlert, 13., tokens.amber))
                        .child(SharedString::from(error.to_string())),
                )
            })
            .child(
                h_flex()
                    .gap(px(6.))
                    .child(add)
                    .child(div().flex_1())
                    .when(!pinned, |row| {
                        row.child(tag(
                            Button::new("guide-link-cancel")
                                .label("Cancel")
                                .on_click(cx.listener(|this, _, _, cx| this.close_form(cx))),
                            || "guide-link-cancel",
                        ))
                    })
                    .child(tag(
                        Button::new("guide-link-submit")
                            .look(if pinned { Look::Plain } else { Look::Primary })
                            .label("Link")
                            .disabled(empty || form.error.is_some())
                            .on_click(cx.listener(|this, _, window, cx| this.submit_link(window, cx))),
                        || "guide-link-submit",
                    )),
            )
            .into_any_element()
    }
}
