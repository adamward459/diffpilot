use diffpilot_core::diff::FileDiff;
use diffpilot_ui_kit::components::glyph_icon;
use diffpilot_ui_kit::{Glyph, Theme};
use gpui::{AnyElement, FontWeight, HighlightStyle, IntoElement, ParentElement as _, Styled as _, StyledText, div, px};
use gpui::{Div, Font};

const MINIBAR_CELLS: usize = 5;

pub fn filled_cells(additions: u32, deletions: u32) -> usize {
    let total = additions + deletions;
    if total == 0 {
        return 0;
    }
    (additions as f32 / total as f32 * MINIBAR_CELLS as f32).round() as usize
}

pub fn minibar(theme: &Theme, additions: u32, deletions: u32) -> impl IntoElement {
    let filled = filled_cells(additions, deletions);
    let mut bar = div().flex().flex_none().gap(px(1.0));
    for cell in 0..MINIBAR_CELLS {
        let color = if cell < filled {
            theme.success
        } else if deletions > 0 {
            theme.danger
        } else {
            theme.diff.press
        };
        bar = bar.child(div().flex_none().size(px(6.0)).rounded(px(1.5)).bg(color));
    }
    bar
}

/// The path with its folder in the muted color and the file name in bold, as one ellipsized text.
pub fn path_text(theme: &Theme, file: &FileDiff) -> AnyElement {
    let (folder, name) = match file.path.rfind('/') {
        Some(index) => file.path.split_at(index + 1),
        None => ("", file.path.as_str()),
    };
    let mut text = String::new();
    let mut highlights = Vec::new();
    if let Some(old) = &file.old_path {
        text.push_str(old);
        text.push_str(" \u{2192} ");
        highlights.push((
            0..text.len(),
            HighlightStyle {
                color: Some(theme.diff.secondary),
                ..HighlightStyle::default()
            },
        ));
    }
    let folder_start = text.len();
    text.push_str(folder);
    if !folder.is_empty() {
        highlights.push((
            folder_start..text.len(),
            HighlightStyle {
                color: Some(theme.diff.secondary),
                ..HighlightStyle::default()
            },
        ));
    }
    let name_start = text.len();
    text.push_str(name);
    highlights.push((
        name_start..text.len(),
        HighlightStyle {
            font_weight: Some(FontWeight::SEMIBOLD),
            ..HighlightStyle::default()
        },
    ));
    div()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .text_size(diffpilot_ui_kit::text_px(12.5))
        .text_color(theme.diff.text)
        .child(StyledText::new(text).with_highlights(highlights))
        .into_any_element()
}

pub fn counts(theme: &Theme, mono: &Font, additions: u32, deletions: u32) -> impl IntoElement {
    let added = format!("+{additions}");
    let deleted = format!("\u{2212}{deletions}");
    let text = format!("{added} {deleted}");
    let highlights = [
        (
            0..added.len(),
            HighlightStyle {
                color: Some(theme.success),
                ..HighlightStyle::default()
            },
        ),
        (
            added.len() + 1..text.len(),
            HighlightStyle {
                color: Some(theme.danger),
                ..HighlightStyle::default()
            },
        ),
    ];
    div()
        .flex_none()
        .font(mono.clone())
        .text_size(diffpilot_ui_kit::text_px(11.5))
        .text_color(theme.diff.text)
        .child(StyledText::new(text).with_highlights(highlights))
}

pub fn thread_count(theme: &Theme, count: usize) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(2.0))
        .ml(px(4.0))
        .text_size(diffpilot_ui_kit::text_px(11.0))
        .text_color(theme.diff.muted)
        .child(glyph_icon(Glyph::Bubble, 12.0, theme.diff.muted))
        .child(count.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minibar_fills_cells_by_share_of_additions() {
        assert_eq!(filled_cells(0, 0), 0);
        assert_eq!(filled_cells(10, 0), 5);
        assert_eq!(filled_cells(0, 10), 0);
        assert_eq!(filled_cells(15, 5), 4);
        assert_eq!(filled_cells(1, 1), 3);
    }
}
