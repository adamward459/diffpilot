use crate::guide::rich::{self, Body, OnRef};
use crate::guide::session::{GuideEvent, GuideRun, GuideSession, Step};
use crate::guide::text::{CodeRef, Inline};
use diffpilot_core::guide::reading::{ChapterKey, ReadingChapter, phase_groups};
use diffpilot_core::guide::{Attention, Diagram};
use diffpilot_core::settings::Settings;
use diffpilot_ui_kit::components::{Button, ChipTone, Look, card_shell, chip_label, glyph_icon, spinner, tag};
use diffpilot_ui_kit::{Glyph, Theme};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, ClickEvent, Context, Entity, EventEmitter, FontWeight, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Window, div,
    px,
};
use gpui_component::{ActiveTheme, h_flex, v_flex};
use std::rc::Rc;
use std::sync::Arc;

const ARTICLE_WIDTH: f32 = 780.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StageEvent {
    Open(CodeRef),
}

pub struct GuideStage {
    guide: Entity<GuideSession>,
    settings: Arc<Settings>,
    _guide: Subscription,
}

impl EventEmitter<StageEvent> for GuideStage {}

impl GuideStage {
    pub fn new(guide: Entity<GuideSession>, settings: Arc<Settings>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.subscribe(&guide, |_, _, _: &GuideEvent, cx| cx.notify());
        Self {
            guide,
            settings,
            _guide: subscription,
        }
    }

    fn write(&self) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
        let guide = self.guide.clone();
        move |_, _, cx| guide.update(cx, |guide, cx| guide.write(cx))
    }

    fn centered(card: impl IntoElement) -> AnyElement {
        div()
            .id("guide-stage")
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(div().w(px(440.)).child(card))
            .into_any_element()
    }

    fn start_card(&self, failure: Option<&str>, cx: &mut Context<Self>) -> AnyElement {
        let tokens = Theme::of(cx);
        let model = self.settings.model_for(diffpilot_core::run::RunKind::Guide).title();
        let retry = failure.is_some();
        Self::centered(
            card_shell(&tokens, tokens.ui.line)
                .p(px(22.))
                .gap(px(12.))
                .child(glyph_icon(Glyph::BookOpen, 22., tokens.ui.claude))
                .child(
                    div()
                        .text_size(px(17.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Read it chapter by chapter"),
                )
                .child(
                    div()
                        .text_size(px(13.))
                        .text_color(tokens.ui.text2)
                        .child("Claude explains what this change does, then splits it into chapters in the order to read them, with notes under the lines that matter."),
                )
                .when_some(failure.map(str::to_owned), |card, message| {
                    card.child(
                        h_flex()
                            .id("guide-failure")
                            .gap(px(6.))
                            .text_size(px(12.5))
                            .text_color(tokens.amber)
                            .child(glyph_icon(Glyph::TriangleAlert, 14., tokens.amber))
                            .child(SharedString::from(message)),
                    )
                })
                .child(
                    h_flex()
                        .gap(px(10.))
                        .child(tag(
                            Button::new("guide-write")
                                .look(Look::Claude)
                                .regular()
                                .icon(Glyph::Sparkles, 14.)
                                .label(if retry { "Try Again" } else { "Write Guide" })
                                .on_click(self.write()),
                            "guide-write",
                        ))
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(tokens.ui.muted)
                                .child(SharedString::from(model)),
                        ),
                ),
            )
    }

    fn writing_card(&self, chapters: usize, cx: &mut Context<Self>) -> AnyElement {
        let tokens = Theme::of(cx);
        let guide = self.guide.clone();
        let progress = match chapters {
            0 => "Reading the diff".to_string(),
            1 => "1 chapter so far".to_string(),
            count => format!("{count} chapters so far"),
        };
        Self::centered(
            card_shell(&tokens, tokens.ui.claude_border)
                .p(px(22.))
                .gap(px(12.))
                .child(
                    h_flex()
                        .gap(px(8.))
                        .child(spinner("guide-writing", 16., tokens.ui.claude))
                        .child(
                            div()
                                .id("guide-writing")
                                .text_size(px(15.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Claude is writing the guide"),
                        ),
                )
                .child(
                    div()
                        .text_size(px(13.))
                        .text_color(tokens.ui.text2)
                        .child(SharedString::from(progress)),
                )
                .child(tag(
                    Button::new("guide-stop")
                        .regular()
                        .label("Stop")
                        .on_click(move |_, _, cx| guide.update(cx, |guide, cx| guide.stop(cx))),
                    "guide-stop",
                )),
        )
    }

    fn on_ref(cx: &mut Context<Self>) -> OnRef {
        let this = cx.entity();
        Rc::new(move |code, _, cx| this.update(cx, |_, cx| cx.emit(StageEvent::Open(code.clone()))))
    }

    fn text(id: &'static str, text: &str, body: Body, on_ref: &OnRef, cx: &App) -> AnyElement {
        rich::element(id, &Inline::parse(text), body, Some(on_ref.clone()), cx)
    }

    fn diagram(diagram: &Diagram, tokens: &Theme, index: usize) -> AnyElement {
        let kind = match &diagram.kind {
            diffpilot_core::guide::DiagramKind::Flowchart => "Flowchart".to_string(),
            diffpilot_core::guide::DiagramKind::Sequence => "Sequence".to_string(),
            diffpilot_core::guide::DiagramKind::Other(name) => name.clone(),
        };
        card_shell(tokens, tokens.ui.line)
            .id(("guide-diagram", index))
            .gap(px(8.))
            .p(px(12.))
            .child(
                h_flex()
                    .gap(px(8.))
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(SharedString::from(
                                diagram.title.clone().unwrap_or_else(|| "Diagram".into()),
                            )),
                    )
                    .child(chip_label(tokens, kind, ChipTone::Blue))
                    .child(div().text_size(px(12.)).text_color(tokens.ui.muted).child("Source")),
            )
            .child(
                div()
                    .rounded(px(8.))
                    .bg(tokens.ui.raised)
                    .p(px(10.))
                    .font_family("Menlo")
                    .text_size(px(12.))
                    .text_color(tokens.ui.text2)
                    .child(SharedString::from(diagram.source.clone())),
            )
            .into_any_element()
    }

    fn chapter_row(&self, index: usize, chapter: &ReadingChapter, tokens: &Theme, cx: &App) -> AnyElement {
        let guide = self.guide.read(cx);
        let read = guide.is_read(chapter);
        let mut title = h_flex().gap(px(8.)).flex_wrap().child(
            div()
                .text_size(px(13.5))
                .font_weight(FontWeight::MEDIUM)
                .child(SharedString::from(chapter.title.clone())),
        );
        match chapter.attention {
            Attention::High => title = title.child(chip_label(tokens, "Read every line", ChipTone::Claude)),
            Attention::Low => {
                title = title.child(
                    div()
                        .h(px(20.))
                        .px(px(7.))
                        .flex()
                        .items_center()
                        .rounded_full()
                        .bg(tokens.ui.raised)
                        .text_size(px(12.))
                        .text_color(tokens.ui.muted)
                        .child("Skim"),
                )
            }
            Attention::Normal => {}
        }
        if chapter.critical > 0 {
            let label = match chapter.critical {
                1 => "1 risky spot".to_string(),
                count => format!("{count} risky spots"),
            };
            title = title.child(chip_label(tokens, label, ChipTone::Amber));
        }
        let guide_entity = self.guide.clone();
        h_flex()
            .id(("guide-overview-chapter", index))
            .w_full()
            .gap(px(10.))
            .py(px(8.))
            .items_start()
            .cursor_pointer()
            .on_click(move |_, _, cx| guide_entity.update(cx, |guide, cx| guide.go(Step::Chapter(index), cx)))
            .child(
                div()
                    .mt(px(1.))
                    .size(px(20.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(if read { tokens.success } else { cx.theme().muted })
                    .text_size(px(11.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(if read { gpui::white() } else { tokens.ui.text2 })
                    .child(if read {
                        glyph_icon(Glyph::Check, 11., gpui::white()).into_any_element()
                    } else if chapter.key == ChapterKey::Extra {
                        div().child("+").into_any_element()
                    } else {
                        div().child((index + 1).to_string()).into_any_element()
                    }),
            )
            .child(
                v_flex().flex_1().min_w_0().gap(px(2.)).child(title).child(
                    div()
                        .text_size(px(12.5))
                        .text_color(tokens.ui.muted)
                        .child(SharedString::from(chapter.summary.clone())),
                ),
            )
            .child(
                h_flex()
                    .gap(px(5.))
                    .flex_none()
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_size(px(11.5))
                    .child(
                        div()
                            .text_color(tokens.success)
                            .child(format!("+{}", chapter.additions)),
                    )
                    .child(
                        div()
                            .text_color(tokens.danger)
                            .child(format!("\u{2212}{}", chapter.deletions)),
                    ),
            )
            .into_any_element()
    }

    fn overview(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = Theme::of(cx);
        let on_ref = Self::on_ref(cx);
        let guide = self.guide.read(cx);
        let Some(stored) = guide.stored() else {
            return div().into_any_element();
        };
        let overview = &stored.guide.overview;
        let chapters = &guide.reading().chapters;
        let mut article = v_flex()
            .w_full()
            .max_w(px(ARTICLE_WIDTH))
            .gap(px(14.))
            .px(px(24.))
            .py(px(24.))
            .child(
                h_flex()
                    .gap(px(5.))
                    .text_size(px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(tokens.ui.claude)
                    .child(glyph_icon(Glyph::Sparkles, 13., tokens.ui.claude))
                    .child("Guide \u{b7} Overview"),
            )
            .child(
                div()
                    .text_size(px(24.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(SharedString::from(overview.title.clone())),
            )
            .child(Self::text(
                "guide-lead",
                &overview.summary,
                Body::new(&tokens, 15., 22.),
                &on_ref,
                cx,
            ));
        if overview.before.is_some() || overview.after.is_some() {
            let side = |label: &'static str, text: &Option<String>, accent: bool| {
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(4.))
                    .p(px(12.))
                    .rounded(px(10.))
                    .border_1()
                    .border_color(if accent {
                        tokens.ui.claude_border
                    } else {
                        tokens.ui.line
                    })
                    .bg(if accent { tokens.ui.claude_bg } else { tokens.ui.raised })
                    .child(
                        div()
                            .text_size(px(11.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(if accent { tokens.ui.claude } else { tokens.ui.muted })
                            .child(label),
                    )
                    .child(Self::text(
                        "guide-side",
                        text.as_deref().unwrap_or_default(),
                        Body::new(&tokens, 13., 19.),
                        &on_ref,
                        cx,
                    ))
            };
            article = article.child(
                h_flex()
                    .gap(px(10.))
                    .items_start()
                    .child(side("BEFORE", &overview.before, false))
                    .child(side("AFTER", &overview.after, true)),
            );
        }
        for (index, diagram) in stored.guide.diagrams.iter().enumerate() {
            article = article.child(Self::diagram(diagram, &tokens, index));
        }
        let count = guide.reading().guided();
        article = article.child(
            h_flex()
                .mt(px(10.))
                .gap(px(8.))
                .items_baseline()
                .child(
                    div()
                        .text_size(px(15.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(match count {
                            1 => "Read it in 1 chapter".to_string(),
                            count => format!("Read it in {count} chapters"),
                        }),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(tokens.ui.muted)
                        .child("core change first, what to skim last"),
                ),
        );
        for (phase, range) in phase_groups(chapters) {
            if let Some(phase) = phase {
                article = article.child(
                    div()
                        .pt(px(6.))
                        .text_size(px(11.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(tokens.ui.muted)
                        .child(SharedString::from(phase.to_uppercase())),
                );
            }
            for index in range {
                article = article.child(self.chapter_row(index, &chapters[index], &tokens, cx));
            }
        }
        let banner = banner(&self.guide, &tokens, cx);
        let failure = match guide.run() {
            GuideRun::Failed(message) => Some(message.clone()),
            _ => None,
        };
        div()
            .id("guide-stage")
            .size_full()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .items_center()
            .children(banner.map(|banner| {
                div()
                    .w_full()
                    .max_w(px(ARTICLE_WIDTH))
                    .px(px(24.))
                    .pt(px(14.))
                    .child(banner)
            }))
            .children(failure.map(|message| {
                h_flex()
                    .id("guide-failure")
                    .w_full()
                    .max_w(px(ARTICLE_WIDTH))
                    .px(px(24.))
                    .pt(px(14.))
                    .gap(px(6.))
                    .text_size(px(12.5))
                    .text_color(tokens.amber)
                    .child(glyph_icon(Glyph::TriangleAlert, 14., tokens.amber))
                    .child(SharedString::from(message))
            }))
            .child(article)
            .into_any_element()
    }
}

pub fn banner(guide: &Entity<GuideSession>, tokens: &Theme, cx: &App) -> Option<AnyElement> {
    let session = guide.read(cx);
    if !session.out_of_date() {
        return None;
    }
    let (keep, rewrite) = (guide.clone(), guide.clone());
    Some(
        tag(h_flex().id("guide-out-of-date"), "guide-out-of-date")
            .w_full()
            .gap(px(8.))
            .px(px(12.))
            .py(px(8.))
            .rounded(px(8.))
            .border_1()
            .border_color(tokens.amber.opacity(0.4))
            .bg(tokens.amber.opacity(0.12))
            .text_size(px(12.5))
            .child(glyph_icon(Glyph::TriangleAlert, 15., tokens.amber))
            .child(div().font_weight(FontWeight::SEMIBOLD).child("Out of date."))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_color(tokens.ui.text2)
                    .child(SharedString::from(session.out_of_date_text())),
            )
            .child(tag(
                Button::new("guide-keep-reading")
                    .label("Keep Reading")
                    .on_click(move |_, _, cx| keep.update(cx, |guide, cx| guide.keep_reading(cx))),
                "guide-keep-reading",
            ))
            .child(tag(
                Button::new("guide-banner-rewrite")
                    .look(Look::Primary)
                    .icon(Glyph::Sparkles, 12.)
                    .label("Rewrite")
                    .on_click(move |_, _, cx| rewrite.update(cx, |guide, cx| guide.write(cx))),
                "guide-banner-rewrite",
            ))
            .into_any_element(),
    )
}

impl Render for GuideStage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (run, has_guide) = {
            let guide = self.guide.read(cx);
            (guide.run().clone(), guide.stored().is_some())
        };
        match run {
            GuideRun::Writing { chapters, .. } => self.writing_card(chapters, cx),
            GuideRun::Failed(message) if !has_guide => self.start_card(Some(&message), cx),
            _ if !has_guide => self.start_card(None, cx),
            _ => self.overview(cx),
        }
    }
}
