use diffpilot_core::review::Severity;
use gpui::{App, Hsla, rgb};
use gpui_component::ActiveTheme;

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
    pub accent_hover: Hsla,
    pub accent_press: Hsla,
    pub cap: Hsla,
    pub cap_border: Hsla,
    pub red: Hsla,
    pub amber: Hsla,
    pub green: Hsla,
    pub user_avatar: Hsla,
}

impl Palette {
    pub fn of(cx: &App) -> Self {
        let theme = cx.theme();
        let dark = theme.is_dark();
        let pick = |light: u32, dark_value: u32| -> Hsla { rgb(if dark { dark_value } else { light }).into() };
        let claude = pick(0xc15f3c, 0xe8916c);
        Self {
            claude,
            claude_bg: claude.opacity(0.11),
            claude_border: claude.opacity(0.38),
            text: theme.foreground,
            muted: theme.muted_foreground,
            line: theme.border,
            card: theme.background,
            field: theme.background,
            accent: theme.primary,
            accent_hover: theme.primary_hover,
            accent_press: theme.primary_active,
            cap: theme.background.blend(theme.foreground.opacity(0.07)),
            cap_border: theme.foreground.opacity(0.13),
            red: pick(0xff3b30, 0xff453a),
            amber: pick(0xff9500, 0xff9f0a),
            green: pick(0x28cd41, 0x32d74b),
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
