use diffpilot_core::file_kind::FileTint;
use diffpilot_core::review::Severity;
use diffpilot_core::settings::{Appearance, appearance_override};
use gpui::{
    App, Background, BoxShadow, Global, Hsla, Rgba, Window, WindowAppearance, linear_color_stop, linear_gradient,
    point, px, rgb, rgba,
};
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

/// Glass after Glassy UI's slab: a body lit from above, a rim, and a shadow that draws in as the slab lowers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glass {
    pub sheen: Hsla,
    pub rim_top: Hsla,
    pub rim_bottom: Hsla,
    pub shade: Hsla,
    pub shade_near: f32,
    pub shade_far: f32,
    pub hairline: Hsla,
}

/// How high a slab rests above the page. A pointer lowers it and a press sets it down.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Rise {
    Rest,
    Hover,
    Pressed,
}

impl Rise {
    fn height(self) -> f32 {
        match self {
            Rise::Rest => 1.0,
            Rise::Hover => 0.5,
            Rise::Pressed => 0.2,
        }
    }
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
    pub glass: Glass,
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

fn over(foreground: Rgba, background: Rgba) -> Rgba {
    mix(Rgba { a: 1.0, ..foreground }, foreground.a, background)
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
        let background: Hsla = pick(0xffffff, 0x1e1e1e);
        let text = pick(0x1d1d1f, 0xf2f2f7);
        let claude = pick(0xc15f3c, 0xe8916c);
        let ui = Ui {
            text,
            text2: pick(0x48484a, 0xc7c7cc),
            muted: pick(0x6e6e73, 0x98989d),
            faint: pick(0xbcbcbc, 0x636366),
            line: pick(0xe3e3e6, 0x3a3a3c),
            line2: pick(0xd1d1d6, 0x48484a),
            raised: pick(0xffffff, 0x2c2c2e),
            pop: pick(0xfbfbfc, 0x2a2a2c),
            cap: background.blend(text.opacity(0.07)),
            cap_border: text.opacity(0.13),
            claude,
            claude_bg: claude.opacity(0.11),
            claude_border: claude.opacity(0.38),
            green: pick(0x28cd41, 0x32d74b),
            user_avatar: rgb(0x616161).into(),
        };
        let chrome = Chrome {
            label: pick(0x000000, 0xffffff),
            secondary: pick(0x7d7d7d, 0x9a9a9a),
            tertiary: pick(0xbcbcbc, 0x585858),
            glass: pick(0xefeff2, 0x262628),
            glass_border: by_mode(rgba(0x0000001c).into(), rgba(0xffffff29).into()),
            glass_shadow: by_mode(rgba(0x0000001a).into(), rgba(0x00000066).into()),
            segment: pick(0xffffff, 0x47474b),
            panel: pick(0xf8f8f8, 0x1f1f1f),
            panel_border: by_mode(rgba(0x0000000f).into(), rgba(0xffffff1a).into()),
            gutter: pick(0xf3f3f4, 0x242424),
            folder: rgb(0xdcb67a).into(),
            uncommitted: pick(0xe07b00, 0xff9f0a),
            badge_fill: by_mode(rgba(0x00000012).into(), rgba(0xffffff1a).into()),
            badge_uncommitted_fill: by_mode(rgba(0xe07b0029).into(), rgba(0xff9f0a33).into()),
            badge_selected_fill: rgba(0xffffff38).into(),
        };
        let glass = Glass {
            sheen: by_mode(gpui::white().opacity(0.6), gpui::white().opacity(0.10)),
            rim_top: by_mode(gpui::white(), gpui::white().opacity(0.38)),
            rim_bottom: by_mode(gpui::white().opacity(0.7), gpui::white().opacity(0.14)),
            shade: by_mode(rgb(0x343e54).into(), gpui::black()),
            shade_near: if dark { 0.28 } else { 0.10 },
            shade_far: if dark { 0.34 } else { 0.14 },
            hairline: by_mode(rgba(0x00000008).into(), rgba(0x00000059).into()),
        };
        Self {
            dark,
            background,
            accent: pick(0x007aff, 0x0a84ff),
            accent_text: pick(0x007aff, 0x0a84ff),
            accent_hover: pick(0x0066d6, 0x3a9bff),
            accent_press: pick(0x0058b8, 0x0070e0),
            on_accent: rgb(0xffffff).into(),
            on_accent_dim: rgba(0xffffffcc).into(),
            success: pick(0x34c759, 0x30d158),
            danger: pick(0xff3b30, 0xff453a),
            amber: pick(0xff9500, 0xff9f0a),
            hover: by_mode(gpui::black().opacity(0.055), gpui::white().opacity(0.10)),
            press: by_mode(gpui::black().opacity(0.10), gpui::white().opacity(0.16)),
            lift_hover: gpui::white().opacity(0.14),
            lift_press: gpui::white().opacity(0.24),
            ui,
            chrome,
            glass,
            diff: Self::diff_colors(dark),
        }
    }

    fn diff_colors(dark: bool) -> Diff {
        let (text, muted, faint, line, editor) = if dark {
            (
                channel(255, 255, 255, 0.85),
                channel(255, 255, 255, 0.55),
                channel(255, 255, 255, 0.25),
                channel(255, 255, 255, 0.1),
                channel(30, 30, 30, 1.0),
            )
        } else {
            (
                channel(0, 0, 0, 0.85),
                channel(0, 0, 0, 0.5),
                channel(0, 0, 0, 0.25),
                channel(0, 0, 0, 0.1),
                channel(255, 255, 255, 1.0),
            )
        };
        let accent = if dark {
            channel(10, 132, 255, 1.0)
        } else {
            channel(0, 122, 255, 1.0)
        };
        let (ok, err, teal, yellow, purple) = if dark {
            (
                channel(48, 209, 88, 1.0),
                channel(255, 69, 58, 1.0),
                channel(64, 200, 224, 1.0),
                channel(255, 214, 10, 1.0),
                channel(191, 90, 242, 1.0),
            )
        } else {
            (
                channel(52, 199, 89, 1.0),
                channel(255, 59, 48, 1.0),
                channel(48, 176, 199, 1.0),
                channel(255, 204, 0, 1.0),
                channel(175, 82, 222, 1.0),
            )
        };
        let label = over(text, editor);
        Diff {
            text: text.into(),
            muted: muted.into(),
            faint: faint.into(),
            secondary: over(muted, editor).into(),
            line: line.into(),
            cap: mix(label, 0.07, editor).into(),
            hunk: mix(accent, 0.07, editor).into(),
            add_bg: mix(ok, 0.13, editor).into(),
            add_number: mix(ok, 0.26, editor).into(),
            add_word: mix(ok, 0.36, editor).into(),
            del_bg: mix(err, 0.13, editor).into(),
            del_number: mix(err, 0.23, editor).into(),
            del_word: mix(err, 0.32, editor).into(),
            press: mix(label, 0.11, editor).into(),
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

    pub fn menu_shadow(&self) -> Vec<BoxShadow> {
        vec![
            BoxShadow {
                color: self.chrome.glass_shadow,
                offset: point(px(0.), px(8.)),
                blur_radius: px(24.),
                spread_radius: px(0.),
            },
            BoxShadow {
                color: self.chrome.glass_shadow,
                offset: point(px(0.), px(1.)),
                blur_radius: px(3.),
                spread_radius: px(0.),
            },
        ]
    }

    /// `base` lit from above: the sheen fades out by 60% of the height, as on a Glassy UI button.
    /// A see-through fill is laid over the page first, so the slab shadow never shows through the glass.
    pub fn glass_body(&self, base: Hsla) -> Background {
        let base = composite(self.background, base);
        linear_gradient(
            180.,
            linear_color_stop(composite(base, self.glass.sheen.opacity(self.glass.sheen.a * 0.8)), 0.),
            linear_color_stop(base, 0.6),
        )
    }

    /// Glassy UI's slab shadow: a sharp contact shadow and a soft one further out, both scaled by the rise.
    pub fn slab_shadow(&self, rise: Rise) -> Vec<BoxShadow> {
        let h = rise.height();
        let g = &self.glass;
        vec![
            BoxShadow {
                color: g.hairline,
                offset: point(px(0.), px(0.)),
                blur_radius: px(0.),
                spread_radius: px(0.5),
            },
            BoxShadow {
                color: g.shade.opacity(g.shade_near),
                offset: point(px(0.), px(1.5 * h)),
                blur_radius: px(3. * h),
                spread_radius: px(-h),
            },
            BoxShadow {
                color: g.shade.opacity(g.shade_far),
                offset: point(px(0.), px(6. * h)),
                blur_radius: px(18. * h),
                spread_radius: px(-4. * h),
            },
        ]
    }

    pub fn glass_shadow(&self) -> Vec<BoxShadow> {
        self.slab_shadow(Rise::Rest)
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
    target.font_size = px(14.);
    target.radius = px(6.);
    target.radius_lg = px(10.);
    target.background = theme.background;
    target.foreground = theme.ui.text;
    target.border = theme.ui.line;
    target.input = pick(0xdcdce0, 0x48484a);
    target.muted = pick(0xf0f0f2, 0x2c2c2e);
    target.muted_foreground = theme.ui.muted;
    target.popover = pick(0xfbfbfc, 0x2a2a2c);
    target.popover_foreground = theme.ui.text;
    target.accent = pick(0xe6e6ea, 0x3a3a3c);
    target.accent_foreground = theme.ui.text;
    target.primary = theme.accent;
    target.primary_hover = theme.accent_hover;
    target.primary_active = theme.accent_press;
    target.primary_foreground = theme.on_accent;
    target.ring = theme.accent;
    target.secondary = pick(0xf0f0f2, 0x2c2c2e);
    target.secondary_hover = pick(0xe6e6ea, 0x3a3a3c);
    target.secondary_active = pick(0xdcdce0, 0x48484a);
    target.secondary_foreground = theme.ui.text;
    target.list_hover = pick(0xececef, 0x2e2e30);
    target.list_active = pick(0x0a7aff, 0x0a6bd8);
    target.sidebar = pick(0xf3f3f5, 0x252527);
    target.sidebar_border = pick(0xe3e3e6, 0x38383a);
    target.sidebar_foreground = theme.ui.text;
    target.title_bar = theme.background;
    target.title_bar_border = pick(0xe3e3e6, 0x38383a);
    target.scrollbar_thumb = pick(0xb0b0b6, 0x6a6a6e);
    target.scrollbar_thumb_hover = pick(0x8e8e93, 0x8a8a8e);
    target.success = pick(0x28a745, 0x32d74b);
    target.danger = pick(0xe5483f, 0xff6961);
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

    #[test]
    fn dark_theme_has_the_macos_dark_editor() {
        let rgba: Rgba = Theme::new(true).background.into();
        assert!(close(rgba.r, 30.0 / 255.0));
        let rgba: Rgba = Theme::new(false).background.into();
        assert!(close(rgba.r, 1.0));
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
