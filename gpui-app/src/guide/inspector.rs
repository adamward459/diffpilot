use crate::guide::diagram::diagram_card;
use crate::guide::rich::{self, Body, OnRef};
use crate::guide::session::{GuideEvent, GuideSession, Step};
use crate::guide::text::{CodeRef, Inline};
use crate::repo_view::tilde_path;
use diffpilot_core::guide::Attention;
use diffpilot_core::guide::reading::ChapterKey;
use diffpilot_ui_kit::components::{Button, ChipTone, Look, chip_label, glyph_icon, icon_button, tag};
use diffpilot_ui_kit::{Glyph, Theme};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, Context, Entity, EventEmitter, FontWeight, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Window, div,
    px,
};
use gpui_component::{h_flex, v_flex};
use std::rc::Rc;

pub const INSPECTOR_WIDTH: f32 = 320.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InspectorEvent {
    Open(CodeRef),
    MarkViewedAndContinue,
}

pub struct GuideInspector {
    guide: Entity<GuideSession>,
    _guide: Subscription,
}

impl EventEmitter<InspectorEvent> for GuideInspector {}

impl GuideInspector {
    pub fn new(guide: Entity<GuideSession>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.subscribe(&guide, |_, _, _: &GuideEvent, cx| cx.notify());
        Self {
            guide,
            _guide: subscription,
        }
    }

    fn text(id: impl Into<gpui::ElementId>, text: &str, body: Body, on_ref: &OnRef, cx: &App) -> AnyElement {
        rich::element(id, &Inline::parse(text), body, Some(on_ref.clone()), cx)
    }
}

fn heading(tokens: &Theme, text: &'static str) -> impl IntoElement {
    div()
        .mt(px(6.))
        .text_size(px(11.5))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(tokens.ui.muted)
        .child(text)
}

impl Render for GuideInspector {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Theme::of(cx);
        let step_key = format!("{:?}", self.guide.read(cx).step());
        let diagrams: Vec<AnyElement> = self
            .guide
            .read(cx)
            .chapter_diagrams()
            .iter()
            .enumerate()
            .map(|(index, diagram)| {
                diagram_card(
                    &format!("guide-chapter-diagram-{step_key}-{index}"),
                    diagram,
                    window,
                    cx,
                )
            })
            .collect();
        let on_ref: OnRef = {
            let this = cx.entity();
            Rc::new(move |code, _, cx| this.update(cx, |_, cx| cx.emit(InspectorEvent::Open(code.clone()))))
        };
        let small = Body::new(&tokens, 12.5, 18.);
        let body = Body::new(&tokens, 13., 19.);
        let guide = self.guide.read(cx);
        let (Some(chapter), Some(stored), Step::Chapter(index)) = (guide.chapter(), guide.stored(), guide.step())
        else {
            return div().into_any_element();
        };
        let total = guide.reading().guided();
        let written = match chapter.key {
            ChapterKey::Guide(at) => stored.guide.chapters.get(at),
            ChapterKey::Extra => None,
        };
        let step_label = match chapter.key {
            ChapterKey::Extra => "Not in the guide".to_string(),
            ChapterKey::Guide(_) => format!("Chapter {} of {total}", index + 1),
        };
        let (previous, next_guide, forward, mark) =
            (self.guide.clone(), self.guide.clone(), self.guide.clone(), cx.entity());
        let last = index + 1 >= guide.reading().chapters.len();
        let next_title = guide.reading().chapters.get(index + 1).map(|next| next.title.clone());

        let mut column = v_flex().gap(px(8.)).p(px(14.)).child(
            h_flex()
                .gap(px(6.))
                .child(glyph_icon(Glyph::BookOpen, 13., tokens.ui.claude))
                .child(
                    div()
                        .text_size(px(12.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(SharedString::from(step_label)),
                )
                .child(div().flex_1())
                .child(
                    icon_button("guide-prev", Glyph::ChevronLeft, tokens.ui.text2)
                        .on_click(move |_, _, cx| previous.update(cx, |guide, cx| guide.previous(cx))),
                )
                .child(
                    icon_button("guide-next", Glyph::ChevronRight, tokens.ui.text2)
                        .on_click(move |_, _, cx| next_guide.update(cx, |guide, cx| guide.next(cx))),
                ),
        );
        let mut chips = h_flex().gap(px(6.));
        if let Some(phase) = &chapter.phase {
            chips = chips.child(chip_label(&tokens, phase.clone(), ChipTone::Blue));
        }
        match chapter.attention {
            Attention::High => chips = chips.child(chip_label(&tokens, "Read every line", ChipTone::Claude)),
            Attention::Low => chips = chips.child(chip_label(&tokens, "Skim", ChipTone::Blue)),
            Attention::Normal => {}
        }
        column = column
            .child(chips)
            .child(
                div()
                    .id("guide-chapter-title")
                    .text_size(px(17.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(SharedString::from(chapter.title.clone())),
            )
            .child(Self::text("guide-chapter-summary", &chapter.summary, body, &on_ref, cx));
        if let Some(written) = written {
            for (position, point) in written.points.iter().enumerate() {
                column = column.child(
                    h_flex()
                        .gap(px(6.))
                        .items_start()
                        .child(div().mt(px(1.)).text_color(tokens.ui.muted).child("\u{2022}"))
                        .child(
                            div()
                                .flex_1()
                                .child(Self::text(("guide-point", position), point, body, &on_ref, cx)),
                        ),
                );
            }
        }
        column = column.children(diagrams);
        let critical: Vec<_> = chapter.notes.iter().filter(|note| note.critical).collect();
        if !critical.is_empty() {
            let mut care = v_flex()
                .id("guide-extra-care")
                .gap(px(6.))
                .p(px(10.))
                .rounded(px(8.))
                .border_1()
                .border_color(tokens.amber.opacity(0.4))
                .bg(tokens.amber.opacity(0.12))
                .child(
                    h_flex()
                        .gap(px(6.))
                        .text_size(px(12.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(tokens.amber)
                        .child(glyph_icon(Glyph::TriangleAlert, 14., tokens.amber))
                        .child("Take extra care"),
                );
            for (position, note) in critical.into_iter().enumerate() {
                let target = CodeRef {
                    path: note.path.clone(),
                    line: note.span.as_ref().map(|span| span.line),
                };
                let open = cx.entity();
                care = care.child(
                    div()
                        .id(("guide-care-note", position))
                        .cursor_pointer()
                        .on_click(move |_, _, cx| {
                            open.update(cx, |_, cx| cx.emit(InspectorEvent::Open(target.clone())))
                        })
                        .child(Self::text(
                            ("guide-care-text", position),
                            &note.text,
                            small,
                            &on_ref,
                            cx,
                        )),
                );
            }
            column = column.child(care);
        }
        if let Some(written) = written.filter(|written| !written.focus.is_empty()) {
            column = column.child(heading(&tokens, "While reading, check"));
            for (position, item) in written.focus.iter().enumerate() {
                column = column.child(
                    h_flex()
                        .gap(px(6.))
                        .items_start()
                        .child(div().mt(px(1.)).text_color(tokens.ui.muted).child("\u{2022}"))
                        .child(
                            div()
                                .flex_1()
                                .child(Self::text(("guide-focus", position), item, small, &on_ref, cx)),
                        ),
                );
            }
        }
        if let Some(written) = written.filter(|written| !written.impact.is_empty()) {
            column = column.child(heading(&tokens, "May break outside this diff"));
            for (position, impact) in written.impact.iter().enumerate() {
                let target = CodeRef {
                    path: match &impact.folder {
                        Some(folder) => folder.join(&impact.path).to_string_lossy().into_owned(),
                        None => impact.path.clone(),
                    },
                    line: impact.line,
                };
                let site = match impact.line {
                    Some(line) => format!("{}:{line}", impact.path),
                    None => impact.path.clone(),
                };
                let open = cx.entity();
                column = column.child(
                    v_flex()
                        .id(("guide-impact", position))
                        .cursor_pointer()
                        .on_click(move |_, _, cx| {
                            open.update(cx, |_, cx| cx.emit(InspectorEvent::Open(target.clone())))
                        })
                        .when_some(impact.folder.as_deref(), |row, folder| {
                            row.child(
                                h_flex()
                                    .gap(px(4.))
                                    .text_size(px(11.))
                                    .text_color(tokens.ui.muted)
                                    .child(glyph_icon(Glyph::Folder, 11., tokens.ui.muted))
                                    .child(SharedString::from(tilde_path(folder))),
                            )
                        })
                        .child(
                            h_flex()
                                .gap(px(4.))
                                .font_family("Menlo")
                                .text_size(px(11.5))
                                .child(SharedString::from(site))
                                .when(impact.folder.is_some(), |row| {
                                    row.child(glyph_icon(Glyph::ExternalLink, 11., tokens.ui.muted))
                                }),
                        )
                        .child(Self::text(
                            ("guide-impact-text", position),
                            &impact.text,
                            small,
                            &on_ref,
                            cx,
                        )),
                );
            }
        }
        let mut actions = v_flex().gap(px(6.)).mt(px(14.));
        if let Some(title) = next_title.filter(|_| !last) {
            actions = actions.child(tag(
                Button::new("guide-next-chapter")
                    .look(Look::Primary)
                    .regular()
                    .label(format!("Next: {title}"))
                    .on_click(move |_, _, cx| forward.update(cx, |guide, cx| guide.next(cx))),
                || "guide-next-chapter",
            ));
        }
        actions = actions.child(tag(
            Button::new("guide-mark-continue")
                .regular()
                .label(if last {
                    "Mark Viewed"
                } else {
                    "Mark Viewed and Continue"
                })
                .on_click(move |_, _, cx| mark.update(cx, |_, cx| cx.emit(InspectorEvent::MarkViewedAndContinue))),
            || "guide-mark-continue",
        ));
        column = column.child(actions);
        div()
            .id("guide-inspector")
            .w(px(INSPECTOR_WIDTH))
            .h_full()
            .flex_none()
            .overflow_y_scroll()
            .border_l_1()
            .border_color(tokens.ui.line)
            .child(column)
            .into_any_element()
    }
}
