use crate::folder_picker::pick_folders;
use crate::repo_view::tilde_path;
use diffpilot_core::guide::linked::{self, LinkError};
use diffpilot_ui_kit::components::{Button, Look, glyph_icon, tag};
use diffpilot_ui_kit::{Glyph, Theme};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AppContext as _, Context, Entity, EventEmitter, ExternalPaths, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, Styled as _, Subscription, Window, div, px,
};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{h_flex, v_flex};
use std::path::{Path, PathBuf};

pub fn folder_chip(tokens: &Theme, glyph: Glyph, name: &str) -> gpui::Div {
    h_flex()
        .h(px(22.))
        .px(px(8.))
        .gap(px(4.))
        .rounded_full()
        .bg(tokens.ui.cap)
        .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SMALL))
        .text_color(tokens.ui.text2)
        .whitespace_nowrap()
        .child(glyph_icon(glyph, 12., tokens.ui.text2))
        .child(SharedString::from(name.to_string()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkedEvent {
    Changed(Vec<PathBuf>),
}

pub struct LinkedFolders {
    repo: PathBuf,
    folders: Vec<PathBuf>,
    input: Entity<InputState>,
    error: Option<(LinkError, String)>,
    refused: Vec<(String, LinkError)>,
    _input: Subscription,
}

impl EventEmitter<LinkedEvent> for LinkedFolders {}

impl LinkedFolders {
    pub fn new(repo: PathBuf, folders: Vec<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Type a path or drop a folder"));
        let subscription = cx.subscribe_in(&input, window, |this, _, event: &InputEvent, window, cx| match event {
            InputEvent::PressEnter { .. } => this.submit(window, cx),
            InputEvent::Change => {
                let typed = this.input.read(cx).value();
                if this.error.as_ref().is_some_and(|(_, checked)| checked != typed.trim()) {
                    this.error = None;
                    cx.notify();
                }
            }
            _ => {}
        });
        Self {
            repo,
            folders,
            input,
            error: None,
            refused: Vec::new(),
            _input: subscription,
        }
    }

    pub fn set_repo(&mut self, repo: PathBuf, folders: Vec<PathBuf>, cx: &mut Context<Self>) {
        self.repo = repo;
        self.folders = folders;
        self.refused.clear();
        self.error = None;
        cx.notify();
    }

    pub fn focus_input(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| input.focus(window, cx));
    }

    fn changed(&self, cx: &mut Context<Self>) {
        cx.emit(LinkedEvent::Changed(self.folders.clone()));
        cx.notify();
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.input.read(cx).value().to_string();
        let trimmed = text.trim().to_string();
        if trimmed.len() != text.len() {
            let value = trimmed.clone();
            self.input.update(cx, |input, cx| input.set_value(value, window, cx));
        }
        match linked::check(&self.repo, &self.folders, &trimmed) {
            Ok(folder) => {
                self.folders.push(folder);
                self.input.update(cx, |input, cx| input.set_value("", window, cx));
                self.refused.clear();
                self.changed(cx);
            }
            Err(error) => {
                self.error = Some((error, trimmed));
                cx.notify();
            }
        }
    }

    fn unlink(&mut self, folder: &Path, cx: &mut Context<Self>) {
        self.folders.retain(|linked| linked != folder);
        self.changed(cx);
    }

    fn add_paths(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        let added = linked::add_all(&self.repo, &self.folders, paths);
        self.refused = added
            .refused
            .into_iter()
            .map(|(path, error)| (tilde_path(&path), error))
            .collect();
        self.folders = added.folders;
        self.changed(cx);
    }

    fn choose(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = pick_folders(true, cx);
        cx.spawn_in(window, async move |this, cx| {
            let Some(paths) = picked.await else { return };
            this.update(cx, |this, cx| this.add_paths(&paths, cx)).ok();
        })
        .detach();
    }

    fn locate(&mut self, missing: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let picked = pick_folders(false, cx);
        cx.spawn_in(window, async move |this, cx| {
            let Some(path) = picked.await.and_then(|paths| paths.into_iter().next()) else {
                return;
            };
            this.update(cx, |this, cx| {
                match linked::relink(&this.repo, &this.folders, &missing, &path) {
                    Ok(folders) => {
                        this.folders = folders;
                        this.refused.clear();
                    }
                    Err(error) => this.refused = vec![(tilde_path(&path), error)],
                }
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
        let small = diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SMALL);
        let secondary = diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SECONDARY);
        let mut section = v_flex()
            .id("linked-folders")
            .w_full()
            .gap(px(6.))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| this.add_paths(paths.paths(), cx)))
            .child(
                h_flex()
                    .gap(px(6.))
                    .child(
                        tag(div().flex_1().min_w_0(), || "linked-input")
                            .child(Input::new(&self.input).text_size(secondary)),
                    )
                    .child(tag(
                        Button::new("linked-choose")
                            .label("Choose…")
                            .on_click(cx.listener(|this, _, window, cx| this.choose(window, cx))),
                        || "linked-choose",
                    )),
            )
            .when_some(self.error.as_ref().map(|(error, _)| *error), |section, error| {
                section.child(
                    tag(h_flex(), || "linked-error")
                        .gap(px(6.))
                        .text_size(secondary)
                        .text_color(tokens.amber)
                        .child(glyph_icon(Glyph::TriangleAlert, 13., tokens.amber))
                        .child(SharedString::from(error.to_string())),
                )
            });
        for (position, folder) in self.folders.iter().enumerate() {
            let missing = !folder.is_dir();
            let color = if missing { tokens.amber } else { tokens.ui.text2 };
            let shown = tilde_path(folder);
            let target = folder.clone();
            let path = div()
                .flex_1()
                .min_w_0()
                .truncate()
                .font_family("Menlo")
                .text_size(small)
                .text_color(color)
                .child(SharedString::from(shown.clone()));
            let row = tag(h_flex(), || format!("linked-folder:{shown}"))
                .gap(px(6.))
                .text_size(secondary)
                .child(glyph_icon(
                    if missing { Glyph::TriangleAlert } else { Glyph::Folder },
                    13.,
                    color,
                ));
            section = section.child(if missing {
                let (locate, remove) = (target.clone(), target);
                v_flex()
                    .gap(px(2.))
                    .child(row.child(path))
                    .child(
                        h_flex()
                            .gap(px(6.))
                            .pl(px(19.))
                            .child(
                                tag(div(), || format!("linked-missing:{shown}"))
                                    .flex_1()
                                    .text_size(small)
                                    .text_color(tokens.amber)
                                    .child("Moved or deleted. Guide runs skip it."),
                            )
                            .child(tag(
                                Button::new(("linked-locate", position))
                                    .look(Look::Ghost)
                                    .label("Locate…")
                                    .on_click(
                                        cx.listener(move |this, _, window, cx| this.locate(locate.clone(), window, cx)),
                                    ),
                                || format!("linked-locate:{shown}"),
                            ))
                            .child(tag(
                                Button::new(("linked-remove", position))
                                    .look(Look::Ghost)
                                    .label("Remove")
                                    .on_click(cx.listener(move |this, _, _, cx| this.unlink(&remove, cx))),
                                || format!("linked-remove:{shown}"),
                            )),
                    )
                    .into_any_element()
            } else {
                row.child(path)
                    .child(tag(
                        Button::new(("linked-remove", position))
                            .look(Look::Ghost)
                            .icon(Glyph::X, 12.)
                            .tooltip(format!("Unlink {shown}"))
                            .on_click(cx.listener(move |this, _, _, cx| this.unlink(&target, cx))),
                        || format!("linked-remove:{shown}"),
                    ))
                    .into_any_element()
            });
        }
        for (position, (shown, error)) in self.refused.iter().enumerate() {
            section = section.child(
                tag(h_flex(), || format!("linked-refused:{shown}"))
                    .gap(px(6.))
                    .text_size(secondary)
                    .text_color(tokens.amber)
                    .child(glyph_icon(Glyph::TriangleAlert, 13., tokens.amber))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(SharedString::from(format!("{shown}: {error}"))),
                    )
                    .child(tag(
                        Button::new(("linked-dismiss", position))
                            .look(Look::Ghost)
                            .icon(Glyph::X, 12.)
                            .tooltip("Dismiss")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.refused.remove(position);
                                cx.notify();
                            })),
                        || format!("linked-dismiss:{shown}"),
                    )),
            );
        }
        section
    }
}
