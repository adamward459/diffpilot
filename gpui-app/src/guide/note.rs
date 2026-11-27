use crate::guide::rich::{self, Body, OnRef};
use crate::guide::text::{CodeRef, Inline};
use diffpilot_ui_kit::components::glyph_icon;
use diffpilot_ui_kit::{Glyph, Theme};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, Context, EventEmitter, FontWeight, IntoElement, ParentElement as _, Pixels, Render, Styled as _, Window, div,
    px,
};
use gpui_component::{h_flex, v_flex};
use std::rc::Rc;

pub mod metrics {
    pub const BORDER: f32 = 1.0;
    pub const PAD_X: f32 = 12.0;
    pub const PAD_Y: f32 = 8.0;
    pub const ICON: f32 = 14.0;
    pub const GAP: f32 = 8.0;
    pub const TEXT_SIZE: f32 = 13.0;
    pub const LINE: f32 = 19.0;
    pub const LABEL_LINE: f32 = 17.0;
}

use metrics as m;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoteEvent {
    Open(CodeRef),
}

pub struct NoteCard {
    inline: Inline,
    critical: bool,
}

impl EventEmitter<NoteEvent> for NoteCard {}

impl NoteCard {
    pub fn new(text: &str, critical: bool) -> Self {
        Self {
            inline: Inline::parse(text),
            critical,
        }
    }

    pub fn height(&self, width: Pixels, window: &Window, cx: &App) -> Pixels {
        let theme = Theme::of(cx);
        let body = Body::new(&theme, m::TEXT_SIZE, m::LINE);
        let wrap = f32::from(width) - 2.0 * (m::BORDER + m::PAD_X) - m::ICON - m::GAP;
        let lines = rich::line_count(&self.inline, body, px(wrap.max(1.0)), window, cx);
        let label = if self.critical { m::LABEL_LINE } else { 0.0 };
        px(2.0 * (m::BORDER + m::PAD_Y) + label + lines as f32 * m::LINE)
    }
}

impl Render for NoteCard {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let (tint, border, glyph) = if self.critical {
            (theme.amber, theme.amber.opacity(0.4), Glyph::TriangleAlert)
        } else {
            (theme.ui.claude, theme.ui.claude_border, Glyph::Sparkles)
        };
        let this = cx.entity();
        let on_ref: OnRef = Rc::new(move |code, _, cx| {
            this.update(cx, |_, cx| cx.emit(NoteEvent::Open(code.clone())));
        });
        let body = Body::new(&theme, m::TEXT_SIZE, m::LINE);
        h_flex()
            .size_full()
            .items_start()
            .gap(px(m::GAP))
            .px(px(m::PAD_X))
            .py(px(m::PAD_Y))
            .rounded(px(8.0))
            .border_1()
            .border_color(border)
            .bg(tint.opacity(0.1))
            .child(div().mt(px(2.0)).child(glyph_icon(glyph, m::ICON, tint)))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .when(self.critical, |column| {
                        column.child(
                            div()
                                .h(px(m::LABEL_LINE))
                                .text_size(diffpilot_ui_kit::text_px(12.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(tint)
                                .child("Take extra care"),
                        )
                    })
                    .child(rich::element("note-text", &self.inline, body, Some(on_ref), cx)),
            )
    }
}
