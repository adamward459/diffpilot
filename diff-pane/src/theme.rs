use gpui::{Hsla, Rgba, WindowAppearance, rgb};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub dark: bool,
    pub text: Hsla,
    pub muted: Hsla,
    pub faint: Hsla,
    pub secondary: Hsla,
    pub line: Hsla,
    pub accent: Hsla,
    pub on_accent: Hsla,
    pub editor: Hsla,
    pub ok: Hsla,
    pub err: Hsla,
    pub cap: Hsla,
    pub hunk: Hsla,
    pub add_bg: Hsla,
    pub add_number: Hsla,
    pub add_ribbon: Hsla,
    pub add_word: Hsla,
    pub del_bg: Hsla,
    pub del_number: Hsla,
    pub del_ribbon: Hsla,
    pub del_word: Hsla,
    pub press: Hsla,
    pub tints: [Hsla; 6],
}

pub fn is_dark(appearance: WindowAppearance) -> bool {
    matches!(appearance, WindowAppearance::Dark | WindowAppearance::VibrantDark)
}

fn rgba(r: u8, g: u8, b: u8, a: f32) -> Rgba {
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

/// `foreground` laid over an opaque `background`.
fn over(foreground: Rgba, background: Rgba) -> Rgba {
    mix(Rgba { a: 1.0, ..foreground }, foreground.a, background)
}

const LIGHT_GLASS_SHARE: f32 = 0.85;
const DARK_GLASS_SHARE: f32 = 0.5;

impl Palette {
    /// A pinned bar's color over `backdrop`, standing in for the page's blurred glass.
    pub fn glass(&self, backdrop: Hsla) -> Hsla {
        let share = if self.dark { DARK_GLASS_SHARE } else { LIGHT_GLASS_SHARE };
        mix(backdrop.into(), share, self.editor.into()).into()
    }

    pub fn new(dark: bool) -> Self {
        let (text, muted, faint, line, editor, accent) = if dark {
            (
                rgba(255, 255, 255, 0.85),
                rgba(255, 255, 255, 0.55),
                rgba(255, 255, 255, 0.25),
                rgba(255, 255, 255, 0.1),
                rgba(30, 30, 30, 1.0),
                rgba(10, 132, 255, 1.0),
            )
        } else {
            (
                rgba(0, 0, 0, 0.85),
                rgba(0, 0, 0, 0.5),
                rgba(0, 0, 0, 0.25),
                rgba(0, 0, 0, 0.1),
                rgba(255, 255, 255, 1.0),
                rgba(0, 122, 255, 1.0),
            )
        };
        let (ok, err, teal, orange, yellow, purple) = if dark {
            (
                rgba(48, 209, 88, 1.0),
                rgba(255, 69, 58, 1.0),
                rgba(64, 200, 224, 1.0),
                rgba(255, 159, 10, 1.0),
                rgba(255, 214, 10, 1.0),
                rgba(191, 90, 242, 1.0),
            )
        } else {
            (
                rgba(52, 199, 89, 1.0),
                rgba(255, 59, 48, 1.0),
                rgba(48, 176, 199, 1.0),
                rgba(255, 149, 0, 1.0),
                rgba(255, 204, 0, 1.0),
                rgba(175, 82, 222, 1.0),
            )
        };
        let label = over(text, editor);
        let secondary = over(muted, editor);
        Self {
            dark,
            text: text.into(),
            muted: muted.into(),
            faint: faint.into(),
            secondary: secondary.into(),
            line: line.into(),
            accent: accent.into(),
            on_accent: rgba(255, 255, 255, 1.0).into(),
            editor: editor.into(),
            ok: ok.into(),
            err: err.into(),
            cap: mix(label, 0.07, editor).into(),
            hunk: mix(accent, 0.07, editor).into(),
            add_bg: mix(ok, 0.13, editor).into(),
            add_number: mix(ok, 0.26, editor).into(),
            add_ribbon: ok.into(),
            add_word: mix(ok, 0.36, editor).into(),
            del_bg: mix(err, 0.13, editor).into(),
            del_number: mix(err, 0.23, editor).into(),
            del_ribbon: err.into(),
            del_word: mix(err, 0.32, editor).into(),
            press: mix(label, 0.11, editor).into(),
            tints: [accent, teal, orange, yellow, purple, secondary].map(Into::into),
        }
    }
}

pub fn hex(color: u32) -> Hsla {
    rgb(color).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.01
    }

    #[test]
    fn mixes_like_css_color_mix() {
        let white = rgba(255, 255, 255, 1.0);
        let green = rgba(52, 199, 89, 1.0);
        let out = mix(green, 0.13, white);
        assert!(close(out.r, 1.0 - 0.13 * (1.0 - 52.0 / 255.0)));
        assert!(close(out.a, 1.0));
    }

    #[test]
    fn appearance_maps_to_dark_variants() {
        assert!(is_dark(WindowAppearance::Dark));
        assert!(is_dark(WindowAppearance::VibrantDark));
        assert!(!is_dark(WindowAppearance::Light));
        assert!(!is_dark(WindowAppearance::VibrantLight));
    }

    #[test]
    fn dark_palette_has_the_macos_dark_editor() {
        let dark = Palette::new(true);
        let light = Palette::new(false);
        let rgba: Rgba = dark.editor.into();
        assert!(close(rgba.r, 30.0 / 255.0));
        let rgba: Rgba = light.editor.into();
        assert!(close(rgba.r, 1.0));
        assert_ne!(dark.add_bg, light.add_bg);
    }
}
