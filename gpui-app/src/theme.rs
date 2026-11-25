use diffpilot_core::file_kind::FileTint;
use gpui::{App, Hsla, rgb, rgba};
use gpui_component::ActiveTheme;

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
    pub label: Hsla,
    pub secondary: Hsla,
    pub tertiary: Hsla,
    pub glass: Hsla,
    pub glass_border: Hsla,
    pub glass_shadow: Hsla,
    pub segment: Hsla,
    pub panel: Hsla,
    pub panel_border: Hsla,
    pub gutter: Hsla,
}

pub fn palette(cx: &App) -> Palette {
    let dark = cx.theme().is_dark();
    let pick = |light: u32, dark_value: u32| -> Hsla { rgb(if dark { dark_value } else { light }).into() };
    Palette {
        success: pick(0x34c759, 0x30d158),
        danger: pick(0xff3b30, 0xff453a),
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
        label: pick(0x000000, 0xffffff),
        secondary: pick(0x7d7d7d, 0x9a9a9a),
        tertiary: pick(0xbcbcbc, 0x585858),
        glass: pick(0xffffff, 0x1a1a1a),
        glass_border: if dark {
            rgba(0xffffff1f).into()
        } else {
            rgba(0x0000000d).into()
        },
        glass_shadow: if dark {
            rgba(0x00000066).into()
        } else {
            rgba(0x0000001a).into()
        },
        segment: pick(0xe4e4e4, 0x3e3e3e),
        panel: pick(0xf8f8f8, 0x1f1f1f),
        panel_border: if dark {
            rgba(0xffffff1a).into()
        } else {
            rgba(0x0000000f).into()
        },
        gutter: pick(0xf3f3f4, 0x242424),
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

pub fn glass_shadow(palette: &Palette) -> Vec<gpui::BoxShadow> {
    vec![gpui::BoxShadow {
        color: palette.glass_shadow,
        offset: gpui::point(gpui::px(0.), gpui::px(2.)),
        blur_radius: gpui::px(10.),
        spread_radius: gpui::px(0.),
    }]
}
