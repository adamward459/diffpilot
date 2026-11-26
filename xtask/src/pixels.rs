use image::{Rgba, RgbaImage};
use std::fmt;

#[derive(Debug, PartialEq, Eq)]
pub struct PixelDiff {
    pub image: String,
    pub changed: u64,
    pub total: u64,
    pub max_delta: u8,
}

impl fmt::Display for PixelDiff {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let share = 100.0 * self.changed as f64 / self.total as f64;
        write!(
            f,
            "{}: {} pixels changed ({}%), max channel delta {}",
            self.image,
            self.changed,
            three_digits(share),
            self.max_delta
        )
    }
}

fn three_digits(value: f64) -> String {
    if value == 0.0 {
        return "0".into();
    }
    let decimals = (2 - value.abs().log10().floor() as i32).max(0) as usize;
    let text = format!("{value:.decimals$}");
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        text
    }
}

/// `None` when the images are equal. A size change counts every pixel of `new` as changed and has
/// no marked image. Otherwise the marked image is `new` dimmed, with each changed pixel in red.
pub fn diff_images(old: &RgbaImage, new: &RgbaImage, image: &str) -> Option<(PixelDiff, Option<RgbaImage>)> {
    let total = u64::from(new.width()) * u64::from(new.height());
    if old.dimensions() != new.dimensions() {
        let name = format!(
            "{image} (size {}x{} -> {}x{})",
            old.width(),
            old.height(),
            new.width(),
            new.height()
        );
        let diff = PixelDiff {
            image: name,
            changed: total,
            total,
            max_delta: 255,
        };
        return Some((diff, None));
    }
    if old.as_raw() == new.as_raw() {
        return None;
    }
    let (mut changed, mut max_delta) = (0, 0);
    let mut marked = RgbaImage::new(new.width(), new.height());
    for ((a, b), out) in old.pixels().zip(new.pixels()).zip(marked.pixels_mut()) {
        *out = if a == b {
            Rgba([b[0] / 3, b[1] / 3, b[2] / 3, 255])
        } else {
            changed += 1;
            max_delta = max_delta.max((0..4).map(|i| a[i].abs_diff(b[i])).max().unwrap_or(0));
            Rgba([255, 0, 0, 255])
        };
    }
    let diff = PixelDiff {
        image: image.to_owned(),
        changed,
        total,
        max_delta,
    };
    Some((diff, Some(marked)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(width: u32, height: u32, color: [u8; 4]) -> RgbaImage {
        RgbaImage::from_pixel(width, height, Rgba(color))
    }

    #[test]
    fn the_same_image_has_no_diff() {
        let image = flat(4, 3, [10, 20, 30, 255]);
        assert_eq!(diff_images(&image, &image.clone(), "a.png"), None);
    }

    #[test]
    fn one_changed_pixel_is_counted_and_marked_red() {
        let old = flat(4, 3, [10, 20, 30, 255]);
        let mut new = old.clone();
        new.put_pixel(2, 1, Rgba([10, 70, 30, 255]));
        let (diff, marked) = diff_images(&old, &new, "a.png").unwrap();
        assert_eq!(
            diff,
            PixelDiff {
                image: "a.png".into(),
                changed: 1,
                total: 12,
                max_delta: 50
            }
        );
        let marked = marked.unwrap();
        assert_eq!(marked.get_pixel(2, 1), &Rgba([255, 0, 0, 255]));
        assert_eq!(marked.get_pixel(0, 0), &Rgba([3, 6, 10, 255]));
        assert_eq!(
            diff.to_string(),
            "a.png: 1 pixels changed (8.33%), max channel delta 50"
        );
    }

    #[test]
    fn the_alpha_channel_counts_toward_the_delta() {
        let old = flat(1, 1, [1, 2, 3, 255]);
        let new = flat(1, 1, [1, 2, 3, 200]);
        assert_eq!(diff_images(&old, &new, "a.png").unwrap().0.max_delta, 55);
    }

    #[test]
    fn a_size_change_counts_every_new_pixel() {
        let (diff, marked) = diff_images(&flat(2, 2, [0; 4]), &flat(3, 2, [0; 4]), "a.png").unwrap();
        assert_eq!(diff.image, "a.png (size 2x2 -> 3x2)");
        assert_eq!((diff.changed, diff.total, diff.max_delta), (6, 6, 255));
        assert!(marked.is_none());
    }

    #[test]
    fn shares_print_with_three_significant_digits() {
        assert_eq!(three_digits(100.0), "100");
        assert_eq!(three_digits(32.0409), "32");
        assert_eq!(three_digits(0.5), "0.5");
        assert_eq!(three_digits(0.01234), "0.0123");
    }
}
