use crate::diff_pane::PaneNote;
use crate::guide::note::metrics as m;
use crate::guide::rich::{self, Body, OnRef};
use crate::guide::text::{CodeRef, Inline};
use diffpilot_core::guide::StopKind;
use diffpilot_ui_kit::components::{Button, CARD_DELAY, Look, rise, tag};
use diffpilot_ui_kit::{Glyph, Theme, text_px, text_size};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, Context, EventEmitter, FontWeight, Hsla, IntoElement, ParentElement as _, Pixels, Render, SharedString,
    Styled as _, Window, div, px,
};
use gpui_component::{h_flex, v_flex};
use std::rc::Rc;

const PAD: f32 = 12.0;
const GAP: f32 = 8.0;
const TAGS: f32 = 22.0;
const BUTTONS: f32 = 26.0;
const CHECK_PAD: f32 = 8.0;
const CHECK_ICON: f32 = 14.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopEvent {
    Open(CodeRef),
    LooksGood,
    Comment,
    Skip,
}

/// The card of the current stop: why it matters, what to check, and the three ways to go on.
pub struct StopCard {
    why: Inline,
    check: Option<(Inline, SharedString)>,
    kind: StopKind,
    critical: bool,
    number: usize,
    total: usize,
    node: Option<String>,
    comment: bool,
}

impl EventEmitter<StopEvent> for StopCard {}

fn kind_label(kind: StopKind) -> &'static str {
    match kind {
        StopKind::Read => "Read closely",
        StopKind::Check => "Check one thing",
        StopKind::Skim => "Skim",
    }
}

impl StopCard {
    pub fn new(note: &PaneNote) -> Self {
        Self {
            why: Inline::parse(&note.text),
            check: note.check.as_deref().map(|check| {
                (
                    Inline::parse(check),
                    SharedString::from(format!("pane:stop-check:{check}")),
                )
            }),
            kind: note.kind,
            critical: note.critical,
            number: note.number,
            total: note.total,
            node: note.node.clone(),
            comment: true,
        }
    }

    /// For a stop that has no diff line to anchor a comment to.
    pub fn without_comment(mut self) -> Self {
        self.comment = false;
        self
    }

    pub fn height(&self, width: Pixels, window: &Window, cx: &App) -> Pixels {
        let theme = Theme::of(cx);
        let body = Body::new(&theme, m::TEXT_SIZE, m::LINE);
        let inner = f32::from(width) - 2.0 * (m::BORDER + PAD);
        let lines = |inline: &Inline, wrap: f32| rich::line_count(inline, body, px(wrap.max(1.0)), window, cx) as f32;
        let mut height = 2.0 * (m::BORDER + PAD) + TAGS + GAP + lines(&self.why, inner) * m::LINE + GAP + BUTTONS;
        if let Some((check, _)) = &self.check {
            let wrap = inner - 2.0 * CHECK_PAD - CHECK_ICON - GAP;
            height += GAP + 2.0 * CHECK_PAD + lines(check, wrap) * m::LINE;
        }
        px(height)
    }
}

fn chip(label: impl Into<SharedString>, color: Hsla, fill: Hsla) -> gpui::Div {
    div()
        .flex_none()
        .px(px(8.0))
        .py(px(2.0))
        .rounded_full()
        .bg(fill)
        .text_size(text_px(text_size::SMALL))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(color)
        .child(label.into())
}

impl Render for StopCard {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let (tint, border) = if self.critical {
            (theme.danger, theme.danger.opacity(0.4))
        } else {
            (theme.ui.claude, theme.ui.claude_border)
        };
        let kind_tint = match self.kind {
            StopKind::Read => theme.ui.claude,
            StopKind::Check => theme.accent,
            StopKind::Skim => theme.ui.muted,
        };
        let this = cx.entity();
        let open = this.clone();
        let on_ref: OnRef = Rc::new(move |code, _, cx| {
            open.update(cx, |_, cx| cx.emit(StopEvent::Open(code.clone())));
        });
        let body = Body::new(&theme, m::TEXT_SIZE, m::LINE);
        let send = |event: StopEvent| {
            let this = this.clone();
            move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
                this.update(cx, |_, cx| cx.emit(event.clone()));
            }
        };
        let tags = h_flex()
            .h(px(TAGS))
            .flex_none()
            .items_center()
            .gap(px(GAP))
            .overflow_hidden()
            .child(tag(
                chip(kind_label(self.kind), kind_tint, kind_tint.opacity(0.14)),
                || "pane:stop-kind",
            ))
            .when(self.critical, |row| {
                row.child(tag(
                    chip("Take care", theme.danger, theme.danger.opacity(0.14)),
                    || "pane:stop-critical",
                ))
            })
            .child(
                div()
                    .flex_none()
                    .ml(px(4.0))
                    .text_size(text_px(text_size::SECONDARY))
                    .text_color(theme.ui.muted)
                    .child(format!("Stop {} of {}", self.number, self.total)),
            )
            .when_some(self.node.clone(), |row, node| {
                row.child(
                    h_flex()
                        .ml_auto()
                        .flex_none()
                        .items_center()
                        .gap(px(6.0))
                        .text_size(text_px(text_size::SECONDARY))
                        .text_color(theme.ui.muted)
                        .child("On the map")
                        .child(tag(
                            div()
                                .px(px(7.0))
                                .py(px(1.0))
                                .rounded(px(5.0))
                                .border_1()
                                .border_color(theme.ui.claude)
                                .bg(theme.background)
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.ui.text)
                                .child(node.clone()),
                            move || format!("pane:stop-node:{node}"),
                        )),
                )
            });
        let check = self.check.as_ref().map(|(check, name)| {
            let name = name.to_string();
            tag(
                h_flex()
                    .items_start()
                    .gap(px(GAP))
                    .p(px(CHECK_PAD))
                    .rounded(px(8.0))
                    .bg(theme.background.opacity(0.7))
                    .child(div().mt(px(2.0)).child(diffpilot_ui_kit::components::glyph_icon(
                        Glyph::CircleCheck,
                        CHECK_ICON,
                        theme.ui.text2,
                    )))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(rich::element("stop-check", check, body, None, cx)),
                    ),
                move || name.clone(),
            )
        });
        let number = self.number;
        let card = tag(
            v_flex()
                .size_full()
                .gap(px(GAP))
                .p(px(PAD))
                .rounded(px(8.0))
                .border_1()
                .border_color(border)
                .bg(tint.opacity(0.1))
                .child(tags)
                .child(rich::element("stop-why", &self.why, body, Some(on_ref), cx))
                .children(check)
                .child(
                    h_flex()
                        .h(px(BUTTONS))
                        .flex_none()
                        .items_center()
                        .gap(px(GAP))
                        .child(tag(
                            Button::new("stop-looks-good")
                                .label("Looks good")
                                .look(Look::Primary)
                                .icon(Glyph::Check, 12.0)
                                .on_click(send(StopEvent::LooksGood)),
                            || "pane:stop-looks-good",
                        ))
                        .children(self.comment.then(|| {
                            tag(
                                Button::new("stop-comment")
                                    .label("Comment")
                                    .on_click(send(StopEvent::Comment)),
                                || "pane:stop-comment",
                            )
                        }))
                        .child(tag(
                            Button::new("stop-skip")
                                .label("Skip for now")
                                .look(Look::Ghost)
                                .on_click(send(StopEvent::Skip)),
                            || "pane:stop-skip",
                        )),
                ),
            move || format!("pane:stop-card:{number}"),
        );
        rise(("stop-card", number), CARD_DELAY, card)
    }
}
