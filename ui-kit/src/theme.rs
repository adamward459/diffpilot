use diffpilot_core::file_kind::FileTint;
use diffpilot_core::review::Severity;
use diffpilot_core::settings::{Appearance, appearance_override};
use gpui::{App, BoxShadow, Global, Hsla, Rgba, Window, WindowAppearance, point, px, rgb, rgba};
use gpui_component::{Theme as ComponentTheme, ThemeMode};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ui {
    pub text: Hsla,
    pub text2: Hsla,
    pub muted: Hsla,
    pub faint: Hsla,
    pub line: Hsla,
    pub line2: Hsla,
    pub raised: Hsla,
    pub pop: Hsla,
    pub cap: Hsla,
    pub cap_border: Hsla,
    pub claude: Hsla,
    pub claude_bg: Hsla,
    pub claude_border: Hsla,
    pub green: Hsla,
    pub user_avatar: Hsla,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Chrome {
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
    pub folder: Hsla,
    pub uncommitted: Hsla,
    pub badge_fill: Hsla,
    pub badge_uncommitted_fill: Hsla,
    pub badge_selected_fill: Hsla,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Diff {
    pub text: Hsla,
    pub muted: Hsla,
    pub faint: Hsla,
    pub secondary: Hsla,
    pub line: Hsla,
    pub cap: Hsla,
    pub hunk: Hsla,
    pub add_bg: Hsla,
    pub add_number: Hsla,
    pub add_word: Hsla,
    pub del_bg: Hsla,
    pub del_number: Hsla,
    pub del_word: Hsla,
    pub press: Hsla,
    teal: Hsla,
    yellow: Hsla,
    purple: Hsla,
}

/// Every color the UI draws with, for one appearance. `ui` holds the card and button colors, `chrome` the
/// sidebar and toolbar glass, `diff` the diff pane's content colors.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    pub dark: bool,
    pub background: Hsla,
    pub accent: Hsla,
    pub accent_text: Hsla,
    pub accent_hover: Hsla,
    pub accent_press: Hsla,
    pub on_accent: Hsla,
    pub on_accent_dim: Hsla,
    pub success: Hsla,
    pub danger: Hsla,
    pub amber: Hsla,
    pub hover: Hsla,
    pub press: Hsla,
    pub lift_hover: Hsla,
    pub lift_press: Hsla,
    pub ui: Ui,
    pub chrome: Chrome,
    pub diff: Diff,
}

impl Global for Theme {}

fn channel(r: u8, g: u8, b: u8, a: f32) -> Rgba {
    Rgba {
        r: f32::from(r) / 255.0,
        g: f32::from(g) / 255.0,
        b: f32::from(b) / 255.0,
        a,
    }
}

/// `color-mix(in srgb, foreground share%, background)` with an opaque background.
pub fn mix(foreground: Rgba, share: f32, background: Rgba) -> Rgba {
    let blend = |f: f32, b: f32| f * share + b * (1.0 - share);
    Rgba {
        r: blend(foreground.r, background.r),
        g: blend(foreground.g, background.g),
        b: blend(foreground.b, background.b),
        a: 1.0,
    }
}

/// `top` laid over `base`, both with alpha.
pub fn composite(base: Hsla, top: Hsla) -> Hsla {
    let (base, top): (Rgba, Rgba) = (base.into(), top.into());
    let alpha = top.a + base.a * (1.0 - top.a);
    if alpha <= 0.0 {
        return gpui::transparent_black();
    }
    let channel = |t: f32, b: f32| (t * top.a + b * base.a * (1.0 - top.a)) / alpha;
    Rgba {
        r: channel(top.r, base.r),
        g: channel(top.g, base.g),
        b: channel(top.b, base.b),
        a: alpha,
    }
    .into()
}

const LIGHT_GLASS_SHARE: f32 = 0.85;
const DARK_GLASS_SHARE: f32 = 0.5;

impl Theme {
    pub fn of(cx: &App) -> Self {
        *cx.global::<Self>()
    }

    pub fn new(dark: bool) -> Self {
        let pick = |light: u32, dark_value: u32| -> Hsla { rgb(if dark { dark_value } else { light }).into() };
        let by_mode = |light: Hsla, dark_value: Hsla| if dark { dark_value } else { light };
        let background: Hsla = pick(0xffffff, 0x1f1f1f);
        let text = pick(0x3b3b3b, 0xcccccc);
        let claude = pick(0xc15f3c, 0xe8916c);
        let side: Hsla = pick(0xf8f8f8, 0x181818);
        let line2 = pick(0xcecece, 0x3c3c3c);
        let (shade, cap_share) = if dark {
            (Rgba::from(gpui::white()), 0.07)
        } else {
            (Rgba::from(gpui::black()), 0.03)
        };
        let editor: Rgba = background.into();
        let claude_tint = by_mode(rgb(0xd97757).into(), claude);
        let ui = Ui {
            text,
            text2: pick(0x555555, 0xb4b4b4),
            muted: pick(0x6e7681, 0x9d9d9d),
            faint: pick(0x9e9e9e, 0x6e7681),
            line: pick(0xe5e5e5, 0x2b2b2b),
            line2,
            raised: pick(0xffffff, 0x252526),
            pop: pick(0xf8f8f8, 0x202020),
            cap: mix(shade, cap_share, editor).into(),
            cap_border: line2,
            claude,
            claude_bg: mix(claude_tint.into(), if dark { 0.12 } else { 0.10 }, editor).into(),
            claude_border: claude.opacity(0.35),
            green: pick(0x2ea043, 0x2ea043),
            user_avatar: rgb(0x616161).into(),
        };
        let chrome = Chrome {
            label: text,
            secondary: ui.muted,
            tertiary: ui.faint,
            glass: side,
            glass_border: ui.line,
            glass_shadow: by_mode(rgba(0x0000001a).into(), rgba(0x00000066).into()),
            segment: pick(0xffffff, 0x3c3c3c),
            panel: side,
            panel_border: ui.line,
            gutter: pick(0xf3f3f3, 0x1f1f1f),
            folder: pick(0xc09553, 0xdcb67a),
            uncommitted: pick(0xbf8803, 0xcca700),
            badge_fill: by_mode(rgba(0x00000012).into(), rgba(0xffffff1a).into()),
            badge_uncommitted_fill: by_mode(rgba(0xbf880329).into(), rgba(0xcca70033).into()),
            badge_selected_fill: rgba(0xffffff38).into(),
        };
        Self {
            dark,
            background,
            accent: pick(0x005fb8, 0x0078d4),
            accent_text: pick(0x005fb8, 0x4daafc),
            accent_hover: pick(0x0a6cc7, 0x1a8ae6),
            accent_press: pick(0x004e99, 0x006abf),
            on_accent: rgb(0xffffff).into(),
            on_accent_dim: rgba(0xffffffcc).into(),
            success: pick(0x2ea043, 0x2ea043),
            danger: pick(0xe51400, 0xf14c4c),
            amber: pick(0xbf8803, 0xcca700),
            hover: by_mode(gpui::black().opacity(0.04), gpui::white().opacity(0.05)),
            press: by_mode(gpui::black().opacity(0.08), gpui::white().opacity(0.10)),
            lift_hover: gpui::white().opacity(0.14),
            lift_press: gpui::white().opacity(0.24),
            ui,
            chrome,
            diff: Self::diff_colors(dark),
        }
    }

    fn diff_colors(dark: bool) -> Diff {
        let (text, muted, faint, line, editor) = if dark {
            (
                channel(204, 204, 204, 1.0),
                channel(157, 157, 157, 1.0),
                channel(110, 118, 129, 1.0),
                channel(43, 43, 43, 1.0),
                channel(31, 31, 31, 1.0),
            )
        } else {
            (
                channel(59, 59, 59, 1.0),
                channel(110, 118, 129, 1.0),
                channel(158, 158, 158, 1.0),
                channel(229, 229, 229, 1.0),
                channel(255, 255, 255, 1.0),
            )
        };
        let accent = if dark {
            channel(0, 120, 212, 1.0)
        } else {
            channel(0, 95, 184, 1.0)
        };
        let (add, add_word, red) = (
            channel(155, 185, 85, 1.0),
            channel(156, 204, 44, 1.0),
            channel(255, 0, 0, 1.0),
        );
        let (teal, yellow, purple) = if dark {
            (
                channel(78, 201, 176, 1.0),
                channel(220, 220, 170, 1.0),
                channel(197, 134, 192, 1.0),
            )
        } else {
            (
                channel(38, 127, 153, 1.0),
                channel(121, 94, 38, 1.0),
                channel(175, 0, 219, 1.0),
            )
        };
        let (add_share, add_number, add_word_share) = if dark { (0.15, 0.25, 0.30) } else { (0.20, 0.32, 0.40) };
        let (del_share, del_number, del_word_share) = if dark { (0.12, 0.20, 0.30) } else { (0.10, 0.18, 0.22) };
        Diff {
            text: text.into(),
            muted: muted.into(),
            faint: faint.into(),
            secondary: muted.into(),
            line: line.into(),
            cap: mix(text, 0.07, editor).into(),
            hunk: mix(accent, if dark { 0.08 } else { 0.06 }, editor).into(),
            add_bg: mix(add, add_share, editor).into(),
            add_number: mix(add, add_number, editor).into(),
            add_word: mix(add_word, add_word_share, editor).into(),
            del_bg: mix(red, del_share, editor).into(),
            del_number: mix(red, del_number, editor).into(),
            del_word: mix(red, del_word_share, editor).into(),
            press: mix(text, 0.11, editor).into(),
            teal: teal.into(),
            yellow: yellow.into(),
            purple: purple.into(),
        }
    }

    pub fn severity(&self, severity: Severity) -> Hsla {
        match severity {
            Severity::MustFix => self.danger,
            Severity::Suggestion => self.amber,
            Severity::Nit => self.ui.muted,
            Severity::Question => self.accent,
        }
    }

    pub fn tint(&self, tint: FileTint) -> Hsla {
        match tint {
            FileTint::Blue => self.accent,
            FileTint::Teal => self.diff.teal,
            FileTint::Orange => self.amber,
            FileTint::Yellow => self.diff.yellow,
            FileTint::Purple => self.diff.purple,
            FileTint::Secondary => self.diff.secondary,
        }
    }

    /// A pinned bar's color over `backdrop`, standing in for the page's blurred glass.
    pub fn pinned_glass(&self, backdrop: Hsla) -> Hsla {
        let share = if self.dark { DARK_GLASS_SHARE } else { LIGHT_GLASS_SHARE };
        mix(backdrop.into(), share, self.background.into()).into()
    }

    /// The control fill with the soft highlight a hovered glass control gets. It follows the fill's own shape.
    pub fn hovered(&self, fill: Hsla) -> Hsla {
        composite(fill, self.hover)
    }

    pub fn pressed(&self, fill: Hsla) -> Hsla {
        composite(fill, self.press)
    }

    /// The same highlight for an accent-colored fill, which brightens instead of darkening.
    pub fn lifted(&self, fill: Hsla, pressed: bool) -> Hsla {
        composite(fill, if pressed { self.lift_press } else { self.lift_hover })
    }

    pub fn glass_shadow(&self) -> Vec<BoxShadow> {
        vec![BoxShadow {
            color: self.chrome.glass_shadow,
            offset: point(px(0.), px(2.)),
            blur_radius: px(10.),
            spread_radius: px(0.),
        }]
    }
}

/// Sets the theme global and gpui-component's theme from the `DIFFPILOT_APPEARANCE` override, else the system.
pub fn init(cx: &mut App) {
    gpui_component::init(cx);
    let dark = match appearance_override() {
        Some(appearance) => appearance == Appearance::Dark,
        None => is_dark(cx.window_appearance()),
    };
    set_dark(dark, None, cx);
}

/// Makes `window` follow the system appearance unless `DIFFPILOT_APPEARANCE` pins one.
pub fn install(window: &mut Window, cx: &mut App) {
    if appearance_override().is_some() {
        return;
    }
    set_dark(is_dark(window.appearance()), Some(window), cx);
    window
        .observe_window_appearance(|window, cx| set_dark(is_dark(window.appearance()), Some(window), cx))
        .detach();
}

pub fn is_dark(appearance: WindowAppearance) -> bool {
    matches!(appearance, WindowAppearance::Dark | WindowAppearance::VibrantDark)
}

pub fn set_dark(dark: bool, window: Option<&mut Window>, cx: &mut App) {
    ComponentTheme::change(if dark { ThemeMode::Dark } else { ThemeMode::Light }, None, cx);
    let theme = Theme::new(dark);
    cx.set_global(theme);
    apply_to_components(&theme, cx);
    if let Some(window) = window {
        window.refresh();
    }
}

fn apply_to_components(theme: &Theme, cx: &mut App) {
    let dark = theme.dark;
    let pick = |light: u32, dark_value: u32| -> Hsla { rgb(if dark { dark_value } else { light }).into() };
    let target = ComponentTheme::global_mut(cx);
    target.font_size = px(13.);
    target.radius = px(6.);
    target.radius_lg = px(10.);
    target.background = theme.background;
    target.foreground = theme.ui.text;
    target.border = theme.ui.line;
    target.input = theme.ui.line2;
    target.muted = theme.ui.cap;
    target.muted_foreground = theme.ui.muted;
    target.popover = theme.ui.pop;
    target.popover_foreground = theme.ui.text;
    target.accent = theme.ui.cap;
    target.accent_foreground = theme.ui.text;
    target.primary = theme.accent;
    target.primary_hover = theme.accent_hover;
    target.primary_active = theme.accent_press;
    target.primary_foreground = theme.on_accent;
    target.ring = theme.accent;
    target.secondary = theme.ui.cap;
    target.secondary_hover = theme.pressed(theme.ui.cap);
    target.secondary_active = theme.pressed(theme.ui.cap);
    target.secondary_foreground = theme.ui.text;
    target.list_hover = theme.hovered(theme.background);
    target.list_active = theme.accent;
    target.sidebar = theme.chrome.panel;
    target.sidebar_border = theme.ui.line;
    target.sidebar_foreground = theme.ui.text;
    target.title_bar = theme.chrome.glass;
    target.title_bar_border = theme.ui.line;
    target.scrollbar_thumb = pick(0xc1c1c1, 0x424242);
    target.scrollbar_thumb_hover = pick(0x929292, 0x4f4f4f);
    target.success = theme.success;
    target.danger = theme.danger;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.01
    }

    #[test]
    fn mixes_like_css_color_mix() {
        let white = channel(255, 255, 255, 1.0);
        let green = channel(52, 199, 89, 1.0);
        let out = mix(green, 0.13, white);
        assert!(close(out.r, 1.0 - 0.13 * (1.0 - 52.0 / 255.0)));
        assert!(close(out.a, 1.0));
    }

    fn hex(color: Hsla) -> u32 {
        let rgba: Rgba = color.into();
        let byte = |value: f32| (value * 255.0).round() as u32;
        (byte(rgba.r) << 16) | (byte(rgba.g) << 8) | byte(rgba.b)
    }

    #[test]
    fn dark_plus_surfaces_and_accents() {
        let dark = Theme::new(true);
        assert_eq!(hex(dark.background), 0x1f1f1f);
        assert_eq!(hex(dark.chrome.panel), 0x181818);
        assert_eq!(hex(dark.ui.raised), 0x252526);
        assert_eq!(hex(dark.ui.pop), 0x202020);
        assert_eq!(hex(dark.ui.line), 0x2b2b2b);
        assert_eq!(hex(dark.ui.line2), 0x3c3c3c);
        assert_eq!(hex(dark.ui.text), 0xcccccc);
        assert_eq!(hex(dark.ui.muted), 0x9d9d9d);
        assert_eq!(hex(dark.accent), 0x0078d4);
        assert_eq!(hex(dark.accent_text), 0x4daafc);
        assert_eq!(hex(dark.ui.claude), 0xe8916c);
        assert_eq!(hex(dark.danger), 0xf14c4c);
        assert_eq!(hex(dark.amber), 0xcca700);
    }

    #[test]
    fn light_plus_surfaces_and_accents() {
        let light = Theme::new(false);
        assert_eq!(hex(light.background), 0xffffff);
        assert_eq!(hex(light.chrome.panel), 0xf8f8f8);
        assert_eq!(hex(light.ui.pop), 0xf8f8f8);
        assert_eq!(hex(light.ui.line), 0xe5e5e5);
        assert_eq!(hex(light.ui.line2), 0xcecece);
        assert_eq!(hex(light.ui.text), 0x3b3b3b);
        assert_eq!(hex(light.ui.muted), 0x6e7681);
        assert_eq!(hex(light.accent), 0x005fb8);
        assert_eq!(hex(light.ui.claude), 0xc15f3c);
        assert_eq!(hex(light.danger), 0xe51400);
        assert_eq!(hex(light.amber), 0xbf8803);
    }

    #[test]
    fn diff_colors_blend_over_the_editor() {
        let dark = Theme::new(true).diff;
        let expected = mix(channel(155, 185, 85, 1.0), 0.15, channel(31, 31, 31, 1.0));
        assert_eq!(hex(dark.add_bg), hex(expected.into()));
        assert_ne!(Theme::new(true).diff.add_bg, Theme::new(false).diff.add_bg);
    }

    #[test]
    fn composite_lays_one_color_over_another() {
        let white: Hsla = rgb(0xffffff).into();
        let over = composite(white, gpui::black().opacity(0.5));
        let rgba: Rgba = over.into();
        assert!(close(rgba.r, 0.5) && close(rgba.a, 1.0));
        let on_clear = composite(gpui::transparent_black(), gpui::black().opacity(0.25));
        assert!(close(on_clear.a, 0.25));
    }

    #[test]
    fn appearance_maps_to_dark_variants() {
        assert!(is_dark(WindowAppearance::Dark));
        assert!(is_dark(WindowAppearance::VibrantDark));
        assert!(!is_dark(WindowAppearance::Light));
        assert!(!is_dark(WindowAppearance::VibrantLight));
    }
}
