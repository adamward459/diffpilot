use crate::icons::Glyph;
use crate::theme::Theme;
use diffpilot_core::review::Severity;
use gpui::prelude::*;
use gpui::{AnyElement, Div, Font, FontWeight, Hsla, div, px};
use gpui_component::Icon;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadgeTone {
    Plain,
    Uncommitted,
    OnAccent,
}

pub fn status_badge(code: &'static str, tone: BadgeTone, mono: Font, theme: &Theme) -> Div {
    let chrome = &theme.chrome;
    let (fill, text): (Hsla, Hsla) = match tone {
        BadgeTone::OnAccent => (chrome.badge_selected_fill, theme.on_accent_dim),
        BadgeTone::Uncommitted => (chrome.badge_uncommitted_fill, chrome.uncommitted),
        BadgeTone::Plain => (chrome.badge_fill, chrome.secondary),
    };
    div()
        .min_w(px(16.))
        .h(px(16.))
        .px(px(3.))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded(px(4.))
        .bg(fill)
        .text_color(text)
        .font(mono)
        .font_weight(FontWeight::SEMIBOLD)
        .text_size(px(10.5))
        .child(code)
}

pub fn severity_badge(severity: Severity, theme: &Theme, size: f32) -> AnyElement {
    let color = theme.severity(severity);
    let glyph = |text: &'static str, size: f32| {
        div()
            .absolute()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .text_color(gpui::white())
            .text_size(px(size))
            .font_weight(FontWeight::BOLD)
            .child(text)
    };
    let shape = match severity {
        Severity::Suggestion => div()
            .relative()
            .size(px(size))
            .child(Icon::new(Glyph::Triangle).size(px(size)).text_color(color))
            .child(
                div()
                    .absolute()
                    .top(px(size * 0.2))
                    .size_full()
                    .child(glyph("!", size * 0.6)),
            ),
        _ => {
            let text = match severity {
                Severity::MustFix => "×",
                Severity::Question => "?",
                _ => "",
            };
            div()
                .relative()
                .size(px(size))
                .rounded_full()
                .bg(color)
                .child(glyph(text, size * 0.75))
        }
    };
    shape.flex_none().into_any_element()
}

pub fn filled_check(color: Hsla, size: f32) -> AnyElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(size))
        .rounded_full()
        .bg(color)
        .child(
            Icon::new(Glyph::CheckThin)
                .size(px(size * 0.7))
                .text_color(gpui::white()),
        )
        .into_any_element()
}

pub fn avatar(author: &str, claude: bool, theme: &Theme, size: f32) -> AnyElement {
    let base = div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(size))
        .rounded_full()
        .text_color(gpui::white())
        .text_size(px(10.))
        .font_weight(FontWeight::BOLD);
    if claude {
        base.bg(theme.ui.claude)
            .child(Icon::new(Glyph::Sparkles).size(px(13.)))
            .into_any_element()
    } else {
        let initials: String = author.split(' ').filter_map(|word| word.chars().next()).collect();
        base.bg(theme.ui.user_avatar).child(initials).into_any_element()
    }
}
