use diffpilot_core::review::Severity;
use gpui::{AnyElement, App, FontWeight, Hsla, IntoElement, ParentElement, Styled, div, px, rgb};
use gpui_component::ActiveTheme;
use gpui_component::button::ButtonCustomVariant;

#[derive(Clone, Copy)]
pub struct Palette {
    pub claude: Hsla,
    pub claude_bg: Hsla,
    pub claude_border: Hsla,
    pub text: Hsla,
    pub muted: Hsla,
    pub line: Hsla,
    pub card: Hsla,
    pub field: Hsla,
    pub accent: Hsla,
    pub red: Hsla,
    pub amber: Hsla,
    pub green: Hsla,
    pub user_avatar: Hsla,
}

impl Palette {
    pub fn of(cx: &App) -> Self {
        let theme = cx.theme();
        let claude: Hsla = if theme.is_dark() { rgb(0xe8916c) } else { rgb(0xc15f3c) }.into();
        Self {
            claude,
            claude_bg: claude.opacity(0.11),
            claude_border: claude.opacity(0.38),
            text: theme.foreground,
            muted: theme.muted_foreground,
            line: theme.border,
            card: theme.popover,
            field: theme.background,
            accent: rgb(0x007aff).into(),
            red: rgb(0xff3b30).into(),
            amber: rgb(0xff9500).into(),
            green: rgb(0x28cd41).into(),
            user_avatar: rgb(0x616161).into(),
        }
    }

    pub fn severity(&self, severity: Severity) -> Hsla {
        match severity {
            Severity::MustFix => self.red,
            Severity::Suggestion => self.amber,
            Severity::Nit => self.muted,
            Severity::Question => self.accent,
        }
    }
}

pub fn severity_glyph(severity: Severity) -> &'static str {
    match severity {
        Severity::MustFix => "×",
        Severity::Suggestion => "!",
        Severity::Nit => "",
        Severity::Question => "?",
    }
}

pub fn severity_badge(severity: Severity, palette: Palette, size: f32) -> AnyElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(size))
        .rounded_full()
        .bg(palette.severity(severity))
        .text_color(gpui::white())
        .text_size(px(size * 0.73))
        .font_weight(FontWeight::BOLD)
        .child(severity_glyph(severity))
        .into_any_element()
}

pub fn accent_variant(cx: &App) -> ButtonCustomVariant {
    let accent: Hsla = rgb(0x007aff).into();
    ButtonCustomVariant::new(cx)
        .color(accent)
        .foreground(gpui::white())
        .border(accent)
        .hover(accent.opacity(0.9))
        .active(accent.opacity(0.8))
}
