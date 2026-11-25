use diffpilot_core::file_kind::FileTint;
use diffpilot_core::settings::{Appearance, appearance_override};
use gpui::{App, Hsla, Window, rgb, rgba};
use gpui_component::{ActiveTheme, Theme, ThemeMode};

pub fn install(window: &mut Window, cx: &mut App) {
    match appearance_override() {
        Some(appearance) => set_mode(mode_of(appearance), Some(window), cx),
        None => {
            let mode = ThemeMode::from(window.appearance());
            set_mode(mode, Some(window), cx);
            window
                .observe_window_appearance(|window, cx| {
                    let mode = ThemeMode::from(window.appearance());
                    set_mode(mode, Some(window), cx);
                })
                .detach();
        }
    }
}

fn mode_of(appearance: Appearance) -> ThemeMode {
    match appearance {
        Appearance::Light => ThemeMode::Light,
        Appearance::Dark => ThemeMode::Dark,
    }
}

pub fn set_mode(mode: ThemeMode, window: Option<&mut Window>, cx: &mut App) {
    Theme::change(mode, None, cx);
    apply_macos_colors(cx);
    if let Some(window) = window {
        window.refresh();
    }
}

fn apply_macos_colors(cx: &mut App) {
    let dark = cx.theme().is_dark();
    let pick = |light: u32, dark_value: u32| -> Hsla { rgb(if dark { dark_value } else { light }).into() };
    let theme = Theme::global_mut(cx);
    theme.font_size = gpui::px(13.);
    theme.radius = gpui::px(6.);
    theme.radius_lg = gpui::px(10.);
    theme.background = pick(0xffffff, 0x1e1e1e);
    theme.foreground = pick(0x1d1d1f, 0xf2f2f7);
    theme.border = pick(0xe3e3e6, 0x3a3a3c);
    theme.input = pick(0xdcdce0, 0x48484a);
    theme.muted = pick(0xf0f0f2, 0x2c2c2e);
    theme.muted_foreground = pick(0x6e6e73, 0x98989d);
    theme.popover = pick(0xfbfbfc, 0x2a2a2c);
    theme.popover_foreground = pick(0x1d1d1f, 0xf2f2f7);
    theme.accent = pick(0xe6e6ea, 0x3a3a3c);
    theme.accent_foreground = pick(0x1d1d1f, 0xf2f2f7);
    theme.primary = pick(0x0a7aff, 0x0a84ff);
    theme.primary_hover = pick(0x0066d6, 0x3a9bff);
    theme.primary_active = pick(0x0058b8, 0x0070e0);
    theme.primary_foreground = rgb(0xffffff).into();
    theme.ring = pick(0x0a7aff, 0x0a84ff);
    theme.secondary = pick(0xf0f0f2, 0x2c2c2e);
    theme.secondary_hover = pick(0xe6e6ea, 0x3a3a3c);
    theme.secondary_active = pick(0xdcdce0, 0x48484a);
    theme.secondary_foreground = pick(0x1d1d1f, 0xf2f2f7);
    theme.list_hover = pick(0xececef, 0x2e2e30);
    theme.list_active = pick(0x0a7aff, 0x0a6bd8);
    theme.sidebar = pick(0xf3f3f5, 0x252527);
    theme.sidebar_border = pick(0xe3e3e6, 0x38383a);
    theme.sidebar_foreground = pick(0x1d1d1f, 0xf2f2f7);
    theme.title_bar = pick(0xffffff, 0x1e1e1e);
    theme.title_bar_border = pick(0xe3e3e6, 0x38383a);
    theme.scrollbar_thumb = pick(0xb0b0b6, 0x6a6a6e);
    theme.scrollbar_thumb_hover = pick(0x8e8e93, 0x8a8a8e);
    theme.success = pick(0x28a745, 0x32d74b);
    theme.danger = pick(0xe5483f, 0xff6961);
}

pub struct Palette {
    pub success: Hsla,
    pub danger: Hsla,
    pub accent: Hsla,
    pub folder: Hsla,
    pub orange: Hsla,
    pub on_accent: Hsla,
    pub on_accent_dim: Hsla,
    pub badge_fill: Hsla,
    pub badge_uncommitted_fill: Hsla,
    pub badge_selected_fill: Hsla,
}

pub fn palette(cx: &App) -> Palette {
    let dark = cx.theme().is_dark();
    let pick = |light: u32, dark_value: u32| -> Hsla { rgb(if dark { dark_value } else { light }).into() };
    Palette {
        success: pick(0x1f9d3a, 0x32d74b),
        danger: pick(0xd8352c, 0xff6961),
        accent: pick(0x0a7aff, 0x0a84ff),
        folder: rgb(0xdcb67a).into(),
        orange: pick(0xe07b00, 0xff9f0a),
        on_accent: rgb(0xffffff).into(),
        on_accent_dim: rgba(0xffffffcc).into(),
        badge_fill: if dark {
            rgba(0xffffff1a).into()
        } else {
            rgba(0x00000012).into()
        },
        badge_uncommitted_fill: if dark {
            rgba(0xff9f0a33).into()
        } else {
            rgba(0xe07b0029).into()
        },
        badge_selected_fill: rgba(0xffffff38).into(),
    }
}

pub fn tint(tint: FileTint, cx: &App) -> Hsla {
    let dark = cx.theme().is_dark();
    let pick = |light: u32, dark_value: u32| -> Hsla { rgb(if dark { dark_value } else { light }).into() };
    match tint {
        FileTint::Blue => pick(0x0a7aff, 0x0a84ff),
        FileTint::Teal => pick(0x30b0c7, 0x40c8e0),
        FileTint::Orange => pick(0xff9500, 0xff9f0a),
        FileTint::Yellow => pick(0xe0b400, 0xffd60a),
        FileTint::Purple => pick(0xaf52de, 0xbf5af2),
        FileTint::Secondary => cx.theme().muted_foreground,
    }
}
