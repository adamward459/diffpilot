use crate::guide::diagram::diagram_card;
use crate::guide::rich::{self, Body, OnRef};
use crate::guide::session::{GuideEvent, GuideRun, GuideSession, Step};
use crate::guide::text::{CodeRef, Inline};
use crate::repo_view::tilde_path;
use diffpilot_core::guide::Attention;
use diffpilot_core::guide::linked::LinkError;
use diffpilot_core::guide::reading::{ChapterKey, ReadingChapter, phase_groups};
use diffpilot_core::settings::Settings;
use diffpilot_ui_kit::components::{Button, ChipTone, Look, card_shell, chip_label, glyph_icon, spinner, tag};
use diffpilot_ui_kit::{Glyph, Theme};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, AppContext as _, ClickEvent, Context, Entity, EventEmitter, FontWeight, InteractiveElement as _,
    IntoElement, ParentElement as _, PathPromptOptions, Render, SharedString, StatefulInteractiveElement as _,
    Styled as _, Subscription, Window, div, px,
};
use gpui_component::input::{Input, InputEvent, InputState};
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
    link_form: Option<LinkForm>,
    _guide: Subscription,
}

struct LinkForm {
    input: Entity<InputState>,
    error: Option<LinkError>,
    _input: Subscription,
}

impl EventEmitter<StageEvent> for GuideStage {}

impl GuideStage {
    pub fn new(guide: Entity<GuideSession>, settings: Arc<Settings>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.subscribe(&guide, |_, _, _: &GuideEvent, cx| cx.notify());
        Self {
            guide,
            settings,
            link_form: None,
            _guide: subscription,
        }
    }

    fn open_link_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("~/Developer/shared-lib"));
        let subscription = cx.subscribe_in(&input, window, |this, _, event: &InputEvent, _, cx| match event {
            InputEvent::PressEnter { .. } => this.submit_link(cx),
            InputEvent::Change => {
                if let Some(form) = this.link_form.as_mut() {
                    form.error = None;
                }
                cx.notify();
            }
            _ => {}
        });
        input.update(cx, |input, cx| input.focus(window, cx));
        self.link_form = Some(LinkForm {
            input,
            error: None,
            _input: subscription,
        });
        cx.notify();
    }

    fn submit_link(&mut self, cx: &mut Context<Self>) {
        let Some(form) = self.link_form.as_mut() else { return };
        let text = form.input.read(cx).value().to_string();
        match self.guide.update(cx, |guide, cx| guide.link(&text, cx)) {
            Ok(()) => self.link_form = None,
            Err(error) => form.error = Some(error),
        }
        cx.notify();
    }

    fn browse(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
                if let Some(form) = this.link_form.as_mut() {
                    form.error = None;
                    form.input
                        .update(cx, |input, cx| input.set_value(tilde_path(&folder), window, cx));
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn linked_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = Theme::of(cx);
        let folders = self.guide.read(cx).linked(cx);
        let mut section = v_flex()
            .id("guide-linked")
            .gap(px(6.))
            .child(
                div()
                    .text_size(diffpilot_ui_kit::text_px(11.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(tokens.ui.muted)
                    .child("LINKED FOLDERS"),
            )
            .child(
                div()
                    .text_size(diffpilot_ui_kit::text_px(12.))
                    .text_color(tokens.ui.text2)
                    .child("Claude can read these for context. They are not reviewed."),
            );
        for (position, folder) in folders.into_iter().enumerate() {
            let missing = !folder.is_dir();
            let color = if missing { tokens.amber } else { tokens.ui.text2 };
            let guide = self.guide.clone();
            let shown = tilde_path(&folder);
            section = section.child(
                tag(h_flex(), || format!("guide-linked-folder:{shown}"))
                    .gap(px(6.))
                    .text_size(diffpilot_ui_kit::text_px(12.5))
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
                            .text_size(diffpilot_ui_kit::text_px(11.5))
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
                            .on_click(move |_, _, cx| guide.update(cx, |guide, cx| guide.unlink(&folder, cx))),
                    ),
            );
        }
        let Some(form) = &self.link_form else {
            return section
                .child(
                    h_flex().child(tag(
                        Button::new("guide-link-folder")
                            .label("+ Link Folder…")
                            .on_click(cx.listener(|this, _, window, cx| this.open_link_form(window, cx))),
                        || "guide-link-folder",
                    )),
                )
                .into_any_element();
        };
        let empty = form.input.read(cx).value().trim().is_empty();
        section
            .child(
                h_flex()
                    .gap(px(6.))
                    .child(
                        div()
                            .flex_1()
                            .child(Input::new(&form.input).text_size(diffpilot_ui_kit::text_px(12.5))),
                    )
                    .child(tag(
                        Button::new("guide-link-browse")
                            .label("Browse…")
                            .on_click(cx.listener(|this, _, window, cx| this.browse(window, cx))),
                        || "guide-link-browse",
                    )),
            )
            .when_some(form.error, |section, error| {
                section.child(
                    tag(h_flex(), || "guide-link-error")
                        .gap(px(6.))
                        .text_size(diffpilot_ui_kit::text_px(12.))
                        .text_color(tokens.amber)
                        .child(glyph_icon(Glyph::TriangleAlert, 13., tokens.amber))
                        .child(SharedString::from(error.to_string())),
                )
            })
            .child(
                h_flex()
                    .gap(px(6.))
                    .justify_end()
                    .child(tag(
                        Button::new("guide-link-cancel")
                            .label("Cancel")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.link_form = None;
                                cx.notify();
                            })),
                        || "guide-link-cancel",
                    ))
                    .child(tag(
                        Button::new("guide-link-submit")
                            .look(Look::Primary)
                            .label("Link")
                            .disabled(empty || form.error.is_some())
                            .on_click(cx.listener(|this, _, _, cx| this.submit_link(cx))),
                        || "guide-link-submit",
                    )),
            )
            .into_any_element()
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
        let model = diffpilot_core::run::RunKind::Guide
            .activity()
            .model(&self.settings)
            .title();
        let retry = failure.is_some();
        Self::centered(
            card_shell(&tokens, tokens.ui.line)
                .p(px(22.))
                .gap(px(12.))
                .child(glyph_icon(Glyph::BookOpen, 22., tokens.ui.claude))
                .child(
                    div()
                        .text_size(diffpilot_ui_kit::text_px(17.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Read it chapter by chapter"),
                )
                .child(
                    div()
                        .text_size(diffpilot_ui_kit::text_px(13.))
                        .text_color(tokens.ui.text2)
                        .child("Claude explains what this change does, then splits it into chapters in the order to read them, with notes under the lines that matter."),
                )
                .child(self.linked_section(cx))
                .when_some(failure.map(str::to_owned), |card, message| {
                    card.child(
                        h_flex()
                            .id("guide-failure")
                            .gap(px(6.))
                            .text_size(diffpilot_ui_kit::text_px(12.5))
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
                            || "guide-write",
                        ))
                        .child(
                            div()
                                .text_size(diffpilot_ui_kit::text_px(12.))
                                .text_color(tokens.ui.muted)
                                .child(SharedString::from(model)),
                        ),
                ),
            )
    }

    fn writing_card(&self, chapters: usize, cx: &mut Context<Self>) -> AnyElement {
        let tokens = Theme::of(cx);
        let guide = self.guide.clone();
        let paused = guide.read(cx).paused(cx);
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
                        .child(if paused {
                            glyph_icon(Glyph::Pause, 16., tokens.ui.claude).into_any_element()
                        } else {
                            spinner("guide-writing", 16., tokens.ui.claude).into_any_element()
                        })
                        .child(
                            div()
                                .id("guide-writing")
                                .text_size(diffpilot_ui_kit::text_px(15.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(if paused {
                                    "The guide is paused"
                                } else {
                                    "Claude is writing the guide"
                                }),
                        ),
                )
                .child(
                    div()
                        .text_size(diffpilot_ui_kit::text_px(13.))
                        .text_color(tokens.ui.text2)
                        .child(SharedString::from(progress)),
                )
                .child(tag(
                    Button::new("guide-stop")
                        .regular()
                        .label("Stop")
                        .on_click(move |_, _, cx| guide.update(cx, |guide, cx| guide.stop(cx))),
                    || "guide-stop",
                )),
        )
    }

    fn on_ref(cx: &mut Context<Self>) -> OnRef {
        let this = cx.entity();
        Rc::new(move |code, _, cx| this.update(cx, |_, cx| cx.emit(StageEvent::Open(code.clone()))))
    }

    fn text(id: &'static str, text: &str, body: Body, on_ref: &OnRef, cx: &App) -> AnyElement {
        tag(div().w_full().min_w_0(), || id)
            .child(rich::element(id, &Inline::parse(text), body, Some(on_ref.clone()), cx))
            .into_any_element()
    }

    fn chapter_row(&self, index: usize, chapter: &ReadingChapter, tokens: &Theme, cx: &App) -> AnyElement {
        let guide = self.guide.read(cx);
        let read = guide.is_read(chapter);
        let mut title = h_flex().gap(px(8.)).flex_wrap().child(
            div()
                .text_size(diffpilot_ui_kit::text_px(13.5))
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
                        .text_size(diffpilot_ui_kit::text_px(12.))
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
                    .text_size(diffpilot_ui_kit::text_px(11.))
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
            .child(v_flex().flex_1().w_0().min_w_0().gap(px(2.)).child(title).child(
                tag(div().w_full().min_w_0(), || format!("guide-chapter-summary:{index}")).child(rich::element(
                    ("guide-overview-summary", index),
                    &Inline::parse(&chapter.summary),
                    Body::new(tokens, 12.5, 18.).colored(tokens.ui.muted),
                    None,
                    cx,
                )),
            ))
            .child(
                tag(h_flex(), || format!("guide-chapter-stat:{index}"))
                    .gap(px(5.))
                    .flex_none()
                    .whitespace_nowrap()
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_size(diffpilot_ui_kit::text_px(11.5))
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

    fn overview(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let tokens = Theme::of(cx);
        let diagrams: Vec<AnyElement> = self
            .guide
            .read(cx)
            .overview_diagrams()
            .iter()
            .enumerate()
            .map(|(index, diagram)| diagram_card(&format!("guide-diagram-{index}"), diagram, window, cx))
            .collect();
        let on_ref = Self::on_ref(cx);
        let guide = self.guide.read(cx);
        let Some(stored) = guide.stored() else {
            return div().into_any_element();
        };
        let overview = &stored.guide.overview;
        let chapters = &guide.reading().chapters;
        let mut article = tag(v_flex(), || "guide-article")
            .w_full()
            .max_w(px(ARTICLE_WIDTH))
            .gap(px(14.))
            .px(px(24.))
            .py(px(24.))
            .child(
                h_flex()
                    .gap(px(5.))
                    .text_size(diffpilot_ui_kit::text_px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(tokens.ui.claude)
                    .child(glyph_icon(Glyph::Sparkles, 13., tokens.ui.claude))
                    .child("Guide \u{b7} Overview"),
            )
            .child(
                div()
                    .text_size(diffpilot_ui_kit::text_px(24.))
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
            let side =
                |label: &'static str, id: &'static str, text_id: &'static str, text: &Option<String>, accent: bool| {
                    tag(v_flex(), || id)
                        .flex_1()
                        .w_0()
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
                                .text_size(diffpilot_ui_kit::text_px(11.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(if accent { tokens.ui.claude } else { tokens.ui.muted })
                                .child(label),
                        )
                        .child(Self::text(
                            text_id,
                            text.as_deref().unwrap_or_default(),
                            Body::new(&tokens, 13., 19.),
                            &on_ref,
                            cx,
                        ))
                };
            article = article.child(
                h_flex()
                    .w_full()
                    .gap(px(10.))
                    .items_start()
                    .child(side(
                        "BEFORE",
                        "guide-before",
                        "guide-before-text",
                        &overview.before,
                        false,
                    ))
                    .child(side("AFTER", "guide-after", "guide-after-text", &overview.after, true)),
            );
        }
        article = article.children(diagrams);
        let count = guide.reading().guided();
        article = article.child(
            h_flex()
                .mt(px(10.))
                .gap(px(8.))
                .items_baseline()
                .child(
                    div()
                        .text_size(diffpilot_ui_kit::text_px(15.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(match count {
                            1 => "Read it in 1 chapter".to_string(),
                            count => format!("Read it in {count} chapters"),
                        }),
                )
                .child(
                    div()
                        .text_size(diffpilot_ui_kit::text_px(12.))
                        .text_color(tokens.ui.muted)
                        .child("core change first, what to skim last"),
                ),
        );
        for (phase, range) in phase_groups(chapters) {
            if let Some(phase) = phase {
                article = article.child(
                    div()
                        .pt(px(6.))
                        .text_size(diffpilot_ui_kit::text_px(11.))
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
                    .text_size(diffpilot_ui_kit::text_px(12.5))
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
        tag(h_flex().id("guide-out-of-date"), || "guide-out-of-date")
            .w_full()
            .gap(px(8.))
            .px(px(12.))
            .py(px(8.))
            .rounded(px(8.))
            .border_1()
            .border_color(tokens.amber.opacity(0.4))
            .bg(tokens.amber.opacity(0.12))
            .text_size(diffpilot_ui_kit::text_px(12.5))
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
                || "guide-keep-reading",
            ))
            .child(tag(
                Button::new("guide-banner-rewrite")
                    .look(Look::Primary)
                    .icon(Glyph::Sparkles, 12.)
                    .label("Rewrite")
                    .on_click(move |_, _, cx| rewrite.update(cx, |guide, cx| guide.write(cx))),
                || "guide-banner-rewrite",
            ))
            .into_any_element(),
    )
}

impl Render for GuideStage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (run, has_guide) = {
            let guide = self.guide.read(cx);
            (guide.run().clone(), guide.stored().is_some())
        };
        match run {
            GuideRun::Writing { chapters, .. } => self.writing_card(chapters, cx),
            GuideRun::Failed(message) if !has_guide => self.start_card(Some(&message), cx),
            _ if !has_guide => self.start_card(None, cx),
            _ => self.overview(window, cx),
        }
    }
}
