use crate::diff_pane::PaneNote;
use crate::diff_pane::layout::{CODE_PADDING, FILE_HEADER, HUNK, LINE, NUMBER_COLUMN, RIBBON};
use crate::diff_pane::lines::{Band, CODE_SIZE, Fonts, NUMBER_SIZE};
use crate::guide::stop_card::StopCard;
use diffpilot_core::file_kind::file_kind;
use diffpilot_ui_kit::components::{glyph_icon, rise, scroll_y, tag};
use diffpilot_ui_kit::{Glyph, Theme, text_px, text_size};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, Entity, FontWeight, InteractiveElement as _, IntoElement, ParentElement as _, ScrollHandle,
    SharedString, Styled as _, Subscription, div, px,
};
use gpui_component::{h_flex, v_flex};
use std::ops::RangeInclusive;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

static EPOCH: AtomicU64 = AtomicU64::new(0);
const CONTEXT: u32 = 4;
const MAX_LINE_BYTES: usize = 4096;
const CARD_INSET: (f32, f32, f32, f32) = (NUMBER_COLUMN + RIBBON + 3.0, 16.0, 8.0, 10.0);

/// A stop in a linked folder: the note, the folder it is relative to, and the text its lines had when the guide
/// was written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkedStop {
    pub note: PaneNote,
    pub folder: PathBuf,
    pub anchor_text: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
enum Body {
    Gone,
    Lines(Lines),
}

#[derive(Debug, PartialEq, Eq)]
struct Lines {
    first: u32,
    text: Vec<SharedString>,
    band: Option<RangeInclusive<u32>>,
    moved: bool,
}

/// The lines shown around `line..=last`, 1-based and clamped to a file of `total` lines.
fn window(total: u32, line: u32, last: u32) -> RangeInclusive<u32> {
    let start = line.saturating_sub(CONTEXT).max(1);
    start..=last.saturating_add(CONTEXT).min(total)
}

fn read(content: &str, line: Option<u32>, end_line: Option<u32>, anchor: Option<&str>) -> Lines {
    let all: Vec<&str> = content.lines().collect();
    let total = all.len() as u32;
    let (shown, band, moved) = match line.filter(|line| *line > 0) {
        None => (1..=total.min(2 * CONTEXT + 1), None, false),
        Some(line) => {
            let last = end_line.filter(|end| *end >= line).unwrap_or(line);
            let fits = last <= total;
            let band = (line <= total).then(|| line..=last.min(total));
            let same = anchor.is_none_or(|anchor| fits && all[line as usize - 1..last as usize].join("\n") == anchor);
            (window(total, line.min(total), last.min(total)), band, !same)
        }
    };
    let text = shown
        .clone()
        .map(|number| display(all[number as usize - 1]))
        .collect::<Vec<_>>();
    Lines {
        first: *shown.start(),
        text,
        band,
        moved,
    }
}

fn display(line: &str) -> SharedString {
    let line = &line[..line.floor_char_boundary(MAX_LINE_BYTES)];
    line.replace('\t', "    ").into()
}

/// The excerpt of a linked stop, read from disk once when the stop became current.
pub struct Excerpt {
    stop: LinkedStop,
    place: SharedString,
    folder_name: SharedString,
    body: Body,
    card: Entity<StopCard>,
    scroll: ScrollHandle,
    epoch: u64,
    _card: Subscription,
}

impl Excerpt {
    pub fn new(stop: LinkedStop, card: Entity<StopCard>, subscription: Subscription) -> Self {
        let body = match std::fs::read_to_string(stop.folder.join(&stop.note.path)) {
            Ok(content) => Body::Lines(read(
                &content,
                stop.note.line,
                stop.note.end_line,
                stop.anchor_text.as_deref(),
            )),
            Err(_) => Body::Gone,
        };
        Self {
            place: crate::repo_view::tilde_path(&stop.folder).into(),
            folder_name: stop
                .folder
                .file_name()
                .map_or_else(
                    || stop.folder.display().to_string(),
                    |name| name.to_string_lossy().into_owned(),
                )
                .into(),
            stop,
            body,
            card,
            scroll: ScrollHandle::new(),
            epoch: EPOCH.fetch_add(1, Ordering::Relaxed),
            _card: subscription,
        }
    }

    pub fn stop(&self) -> &LinkedStop {
        &self.stop
    }

    pub fn element(&self, theme: &Theme, fonts: &Fonts) -> AnyElement {
        let path = self.stop.note.path.clone();
        let header = h_flex()
            .h(px(FILE_HEADER))
            .flex_none()
            .w_full()
            .items_center()
            .gap(px(8.0))
            .px(px(10.0))
            .child(glyph_icon(Glyph::FileLines, 14.0, theme.tint(file_kind(&path).tint())))
            .child(
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(path),
            )
            .child(tag(
                div()
                    .flex_none()
                    .px(px(7.0))
                    .py(px(1.0))
                    .rounded_full()
                    .border_1()
                    .border_dashed()
                    .border_color(theme.diff.muted)
                    .text_size(text_px(text_size::SMALL))
                    .text_color(theme.diff.secondary)
                    .child(format!("Linked \u{b7} {}", self.folder_name)),
                || "pane:linked-pill",
            ))
            .child(div().flex_1())
            .child(
                div()
                    .flex_none()
                    .text_size(text_px(text_size::SECONDARY))
                    .text_color(theme.diff.muted)
                    .child("Context only, nothing to diff"),
            );
        let context = tag(
            h_flex()
                .items_center()
                .h(px(HUNK))
                .flex_none()
                .w_full()
                .pl(px(12.0))
                .pr(px(8.0))
                .border_y_1()
                .border_dashed()
                .border_color(theme.diff.line)
                .bg(theme.diff.cap)
                .font(fonts.mono.clone())
                .text_size(text_px(text_size::SMALL))
                .text_color(theme.diff.secondary),
            || "pane:linked-context",
        )
        .child(div().flex_none().child("Not in the diff \u{b7} linked folder\u{a0}"))
        .child(div().min_w_0().truncate().child(self.place.clone()))
        .child(div().flex_none().child("\u{a0}\u{b7} read-only"));
        let body = match &self.body {
            Body::Gone => v_flex()
                .child(
                    tag(
                        div()
                            .py(px(24.0))
                            .px(px(16.0))
                            .text_size(text_px(text_size::BODY))
                            .text_color(theme.diff.muted),
                        || "pane:linked-gone",
                    )
                    .child("This file is gone."),
                )
                .child(self.card())
                .into_any_element(),
            Body::Lines(lines) => self.lines(lines, theme, fonts),
        };
        let block = v_flex()
            .id("linked-excerpt")
            .size_full()
            .bg(theme.background)
            .text_color(theme.diff.text)
            .text_size(text_px(text_size::BODY))
            .child(rise(
                ("linked-excerpt", self.epoch),
                Duration::ZERO,
                v_flex().w_full().child(header).child(context).child(body),
            ));
        scroll_y(&self.scroll, block).size_full().into_any_element()
    }

    fn card(&self) -> gpui::Div {
        let (left, right, top, bottom) = CARD_INSET;
        div()
            .flex_none()
            .ml(px(left))
            .mr(px(right))
            .mt(px(top))
            .mb(px(bottom))
            .child(self.card.clone())
    }

    fn lines(&self, lines: &Lines, theme: &Theme, fonts: &Fonts) -> AnyElement {
        let colors = Band::of(&self.stop.note).map(|band| band.colors(theme));
        let mut column = v_flex().flex_none().w_full();
        let mut carded = false;
        for (offset, text) in lines.text.iter().enumerate() {
            let number = lines.first + offset as u32;
            let banded = lines.band.as_ref().is_some_and(|band| band.contains(&number));
            let row = h_flex()
                .h(px(LINE))
                .flex_none()
                .w_full()
                .items_center()
                .font(fonts.mono.clone())
                .when_some(colors.filter(|_| banded), |row, colors| row.bg(colors.tint));
            let row = if banded {
                let line = text.clone();
                tag(row, move || format!("pane:linked-band:{number}:{line}"))
            } else {
                tag(row, move || format!("pane:linked-line:{number}"))
            };
            column = column.child(
                row.child(
                    div()
                        .flex_none()
                        .w(px(NUMBER_COLUMN))
                        .pr(px(6.0))
                        .text_right()
                        .text_size(px(NUMBER_SIZE))
                        .text_color(theme.diff.faint)
                        .child(number.to_string()),
                )
                .child(
                    div()
                        .flex_none()
                        .w(px(RIBBON))
                        .h_full()
                        .when_some(colors.filter(|_| banded), |bar, colors| bar.bg(colors.bar)),
                )
                .child(
                    div()
                        .min_w_0()
                        .pl(px(CODE_PADDING))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_size(px(CODE_SIZE))
                        .text_color(theme.diff.text)
                        .child(text.clone()),
                ),
            );
            if lines.band.as_ref().is_some_and(|band| *band.end() == number) {
                column = column.child(self.card());
                carded = true;
            }
        }
        if !carded {
            column = column.child(self.card());
        }
        v_flex()
            .when(lines.moved, |body| {
                body.child(
                    tag(
                        div()
                            .px(px(16.0))
                            .py(px(8.0))
                            .text_size(text_px(text_size::SECONDARY))
                            .text_color(theme.diff.muted),
                        || "pane:linked-moved",
                    )
                    .child("The code moved since the guide was written."),
                )
            })
            .child(column)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\nl\n";

    #[test]
    fn the_window_adds_four_lines_each_way() {
        assert_eq!(window(12, 6, 6), 2..=10);
        assert_eq!(window(12, 6, 8), 2..=12);
    }

    #[test]
    fn the_window_stops_at_the_ends_of_the_file() {
        assert_eq!(window(12, 2, 3), 1..=7);
        assert_eq!(window(12, 11, 12), 7..=12);
        assert!(window(0, 1, 1).is_empty());
    }

    #[test]
    fn lines_that_match_the_anchor_text_are_not_moved() {
        let lines = read(FILE, Some(5), Some(6), Some("e\nf"));
        assert_eq!(lines.first, 1);
        assert_eq!(lines.band, Some(5..=6));
        assert!(!lines.moved);
        assert_eq!(lines.text.len(), 10);
    }

    #[test]
    fn changed_text_or_a_missing_anchor_marks_the_excerpt_moved() {
        assert!(read(FILE, Some(5), None, Some("x")).moved);
        assert!(!read(FILE, Some(5), None, None).moved);
    }

    #[test]
    fn lines_past_the_end_show_the_end_of_the_file_without_a_band() {
        let lines = read(FILE, Some(40), None, Some("z"));
        assert_eq!(lines.band, None);
        assert!(lines.moved);
        assert_eq!(lines.first, 8);
        assert_eq!(lines.text.len(), 5);
    }

    #[test]
    fn a_span_that_runs_past_the_end_keeps_the_band_to_the_last_line() {
        let lines = read(FILE, Some(11), Some(15), Some("k\nl\nm\nn\no"));
        assert_eq!(lines.band, Some(11..=12));
        assert!(lines.moved);
    }

    #[test]
    fn a_stop_without_a_line_shows_the_top_of_the_file() {
        let lines = read(FILE, None, None, None);
        assert_eq!((lines.first, lines.band.clone(), lines.moved), (1, None, false));
        assert_eq!(lines.text.len(), 9);
    }

    #[test]
    fn tabs_become_spaces_and_long_lines_are_cut() {
        assert_eq!(display("\tx").as_ref(), "    x");
        assert_eq!(display(&"é".repeat(MAX_LINE_BYTES)).len(), MAX_LINE_BYTES);
    }
}
