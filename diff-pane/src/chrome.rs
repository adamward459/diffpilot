use crate::icons;
use crate::theme::Palette;
use diffpilot_core::changes::FileStatus;
use diffpilot_core::diff::FileDiff;
use diffpilot_core::file_kind::{FileTint, file_kind};
use gpui::{
    AnyElement, FontWeight, HighlightStyle, InteractiveElement as _, IntoElement, ParentElement as _, Styled as _,
    StyledText, div, px,
};
use gpui::{Div, Hsla, Stateful};

const MINIBAR_CELLS: usize = 5;

pub fn tint(palette: &Palette, file: &FileDiff) -> Hsla {
    let index = match file_kind(&file.path).tint() {
        FileTint::Blue => 0,
        FileTint::Teal => 1,
        FileTint::Orange => 2,
        FileTint::Yellow => 3,
        FileTint::Purple => 4,
        FileTint::Secondary => 5,
    };
    palette.tints[index]
}

pub fn status_letter(status: FileStatus) -> &'static str {
    status.code()
}

pub fn filled_cells(additions: u32, deletions: u32) -> usize {
    let total = additions + deletions;
    if total == 0 {
        return 0;
    }
    (additions as f32 / total as f32 * MINIBAR_CELLS as f32).round() as usize
}

pub fn minibar(palette: &Palette, additions: u32, deletions: u32) -> impl IntoElement {
    let filled = filled_cells(additions, deletions);
    let mut bar = div().flex().flex_none().gap(px(1.0));
    for cell in 0..MINIBAR_CELLS {
        let color = if cell < filled {
            palette.add_ribbon
        } else if deletions > 0 {
            palette.del_ribbon
        } else {
            palette.press
        };
        bar = bar.child(div().size(px(6.0)).rounded(px(1.5)).bg(color));
    }
    bar
}

/// The path with its folder in the muted color and the file name in bold, as one ellipsized text.
pub fn path_text(palette: &Palette, file: &FileDiff) -> AnyElement {
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
                color: Some(palette.muted),
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
                color: Some(palette.muted),
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
        .text_size(px(12.5))
        .text_color(palette.text)
        .child(StyledText::new(text).with_highlights(highlights))
        .into_any_element()
}

pub fn status_badge(palette: &Palette, status: FileStatus) -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .h(px(16.0))
        .min_w(px(16.0))
        .px(px(3.0))
        .rounded(px(4.0))
        .bg(palette.cap)
        .text_color(palette.muted)
        .text_size(px(10.5))
        .font_weight(FontWeight::SEMIBOLD)
        .child(status_letter(status))
}

pub fn counts(palette: &Palette, additions: u32, deletions: u32) -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .gap(px(4.0))
        .text_size(px(11.5))
        .child(div().text_color(palette.ok).child(format!("+{additions}")))
        .child(div().text_color(palette.err).child(format!("\u{2212}{deletions}")))
}

pub fn viewed_toggle(palette: &Palette, id: impl Into<gpui::ElementId>, viewed: bool) -> Stateful<Div> {
    let mut checkbox = div()
        .flex()
        .items_center()
        .justify_center()
        .size(px(14.0))
        .rounded(px(3.0))
        .border_1()
        .border_color(if viewed { palette.accent } else { palette.line });
    if viewed {
        checkbox = checkbox.bg(palette.accent).child(icons::check(10.0, palette.on_accent));
    }
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.0))
        .ml_auto()
        .cursor_pointer()
        .text_size(px(11.0))
        .text_color(palette.muted)
        .child(checkbox)
        .child("Viewed")
}

pub fn thread_count(palette: &Palette, count: usize) -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(2.0))
        .ml(px(4.0))
        .text_size(px(11.0))
        .text_color(palette.muted)
        .child(icons::bubble(12.0, palette.muted))
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
