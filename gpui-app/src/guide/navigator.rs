use crate::format::ago;
use crate::guide::session::{GuideEvent, GuideRun, GuideSession, Step};
use diffpilot_core::file_kind::file_kind;
use diffpilot_core::guide::Attention;
use diffpilot_core::guide::reading::{ChapterKey, phase_groups};
use diffpilot_ui_kit::components::tag;
use diffpilot_ui_kit::{Glyph, Theme};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, ClickEvent, Context, Entity, EventEmitter, FontWeight, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Window, div,
    px,
};
use gpui_component::{ActiveTheme, Icon, h_flex, v_flex};

const ROW_HEIGHT: f32 = 24.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NavigatorEvent {
    Go(Step),
    Reveal(String),
}

pub struct GuideNavigator {
    guide: Entity<GuideSession>,
    _guide: Subscription,
}

impl EventEmitter<NavigatorEvent> for GuideNavigator {}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as i64)
}

impl GuideNavigator {
    pub fn new(guide: Entity<GuideSession>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.subscribe(&guide, |_, _, _: &GuideEvent, cx| cx.notify());
        Self {
            guide,
            _guide: subscription,
        }
    }

    fn header(&self, cx: &gpui::App) -> AnyElement {
        let tokens = Theme::of(cx);
        let guide = self.guide.read(cx);
        let total = guide.reading().guided();
        let read = guide.read_count();
        let text = match (guide.stored(), guide.run()) {
            (_, GuideRun::Writing { .. }) => "Claude is writing the guide".to_string(),
            (None, _) => "No guide yet".to_string(),
            (Some(stored), _) => {
                let progress = if read == 0 {
                    "Not started".to_string()
                } else {
                    format!("{read} of {total} chapters read")
                };
                let when = ago(now_ms(), stored.finished_at_ms.unwrap_or(stored.created_at_ms));
                format!("{progress} \u{b7} Claude, {when}")
            }
        };
        let fraction = if total == 0 { 0.0 } else { read as f32 / total as f32 };
        v_flex()
            .gap(px(7.))
            .px_3()
            .pt(px(5.))
            .pb(px(10.))
            .child(
                div()
                    .id("guide-progress-text")
                    .text_size(px(12.))
                    .text_color(tokens.chrome.secondary)
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .child(text),
            )
            .child(
                div().h(px(4.)).w_full().rounded_full().bg(cx.theme().muted).child(
                    div()
                        .h_full()
                        .w(gpui::relative(fraction))
                        .rounded_full()
                        .bg(tokens.success),
                ),
            )
            .into_any_element()
    }
}

impl Render for GuideNavigator {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Theme::of(cx);
        let header = self.header(cx);
        let guide = self.guide.read(cx);
        let step = guide.step();
        let mut list = v_flex()
            .id("guide-rows")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(6.))
            .pb(px(8.));
        if guide.stored().is_none() {
            return v_flex()
                .size_full()
                .child(header)
                .child(
                    div()
                        .px_3()
                        .pt(px(6.))
                        .text_size(px(12.))
                        .text_color(tokens.chrome.tertiary)
                        .child("Claude has not written a guide for this change."),
                )
                .into_any_element();
        }
        let row = |id: &'static str, index: usize, selected: bool| {
            h_flex()
                .id((id, index))
                .w_full()
                .min_h(px(ROW_HEIGHT))
                .pl(px(8.))
                .pr(px(10.))
                .py(px(2.))
                .gap(px(6.))
                .items_start()
                .rounded(px(8.))
                .cursor_pointer()
                .text_color(if selected {
                    tokens.on_accent
                } else {
                    tokens.chrome.label
                })
                .when(selected, |this| this.bg(tokens.accent))
                .when(!selected, |this| this.hover(|style| style.bg(tokens.hover)))
        };
        list = list.child(
            tag(row("guide-overview-row", 0, step == Step::Overview), "guide-overview")
                .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(NavigatorEvent::Go(Step::Overview))))
                .child(
                    div()
                        .mt(px(5.))
                        .child(
                            Icon::new(Glyph::FileText)
                                .size(px(15.))
                                .text_color(if step == Step::Overview {
                                    tokens.on_accent
                                } else {
                                    tokens.chrome.secondary
                                }),
                        ),
                )
                .child(div().flex_1().child("Overview")),
        );
        for (phase, range) in phase_groups(&guide.reading().chapters) {
            if let Some(phase) = phase {
                list = list.child(
                    div()
                        .px(px(8.))
                        .pt(px(10.))
                        .pb(px(3.))
                        .text_size(px(11.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(tokens.chrome.tertiary)
                        .child(SharedString::from(phase)),
                );
            }
            for index in range {
                let chapter = &guide.reading().chapters[index];
                let current = step == Step::Chapter(index);
                let read = guide.is_read(chapter);
                let dim = chapter.attention == Attention::Low && !current;
                let badge_bg = match (current, read) {
                    (true, _) => tokens.on_accent.opacity(0.25),
                    (false, true) => tokens.success,
                    (false, false) => cx.theme().muted,
                };
                let badge = div()
                    .mt(px(3.))
                    .size(px(16.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(badge_bg)
                    .text_size(px(10.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(if read && !current {
                        gpui::white()
                    } else if current {
                        tokens.on_accent
                    } else {
                        tokens.chrome.secondary
                    })
                    .child(if read {
                        Icon::new(Glyph::Check).size(px(10.)).into_any_element()
                    } else if chapter.key == ChapterKey::Extra {
                        div().child("+").into_any_element()
                    } else {
                        div().child((index + 1).to_string()).into_any_element()
                    });
                let mut item = tag(
                    row("guide-chapter-row", index, current).when(dim, |this| this.opacity(0.6)),
                    format!("guide-chapter-{}", index + 1),
                )
                .on_click(
                    cx.listener(move |_, _: &ClickEvent, _, cx| cx.emit(NavigatorEvent::Go(Step::Chapter(index)))),
                )
                .child(
                    div().mt(px(5.)).child(
                        Icon::new(if current {
                            Glyph::ChevronDown
                        } else {
                            Glyph::ChevronRight
                        })
                        .size(px(11.))
                        .text_color(if current {
                            tokens.on_accent
                        } else {
                            tokens.chrome.tertiary
                        }),
                    ),
                )
                .child(badge)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(SharedString::from(chapter.title.clone())),
                );
                if chapter.critical > 0 {
                    item = item.child(
                        div()
                            .mt(px(8.))
                            .size(px(7.))
                            .flex_none()
                            .rounded_full()
                            .bg(tokens.amber),
                    );
                }
                list = list.child(item);
                if current {
                    for (position, path) in chapter.files.iter().enumerate() {
                        let name = path.rsplit('/').next().unwrap_or(path).to_string();
                        let target = path.clone();
                        let viewed = guide.viewed().contains(path);
                        list = list.child(
                            h_flex()
                                .id(("guide-file-row", index * 1000 + position))
                                .w_full()
                                .h(px(ROW_HEIGHT))
                                .pl(px(40.))
                                .pr(px(10.))
                                .gap(px(6.))
                                .rounded(px(8.))
                                .cursor_pointer()
                                .hover(|style| style.bg(tokens.hover))
                                .text_color(if viewed {
                                    tokens.chrome.secondary
                                } else {
                                    tokens.chrome.label
                                })
                                .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                                    cx.emit(NavigatorEvent::Reveal(target.clone()))
                                }))
                                .child(
                                    Icon::new(Glyph::for_file(&name))
                                        .size(px(13.))
                                        .text_color(tokens.tint(file_kind(&name).tint())),
                                )
                                .child(div().min_w_0().truncate().child(SharedString::from(name))),
                        );
                    }
                }
            }
        }
        v_flex().size_full().child(header).child(list).into_any_element()
    }
}
