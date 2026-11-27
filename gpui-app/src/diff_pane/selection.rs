use crate::diff_pane::lines::Column;
use crate::diff_pane::rows::{Row, RowKind};
use diffpilot_core::diff::{FileDiff, LineKind};
use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Spot {
    pub hunk: u32,
    pub line: u32,
    pub byte: usize,
}

impl Spot {
    pub fn at(hunk: u32, line: u32, byte: usize) -> Self {
        Self { hunk, line, byte }
    }

    fn line(self) -> (u32, u32) {
        (self.hunk, self.line)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Extent {
    Lines,
    Chars,
    Hunk,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub file: usize,
    pub column: Column,
    pub extent: Extent,
    pub anchor: Spot,
    pub head: Spot,
    pub dragging: bool,
}

pub fn column_line(kind: RowKind, column: Column) -> Option<(u32, u32)> {
    match (kind, column) {
        (RowKind::Line { hunk, line }, Column::Unified) => Some((hunk, line)),
        (RowKind::Pair { hunk, left, .. }, Column::Left) => Some((hunk, left?)),
        (RowKind::Pair { hunk, right, .. }, Column::Right) => Some((hunk, right?)),
        _ => None,
    }
}

impl Selection {
    pub fn hunk(file: usize, hunk: u32) -> Self {
        let spot = Spot::at(hunk, 0, 0);
        Self {
            file,
            column: Column::Unified,
            extent: Extent::Hunk,
            anchor: spot,
            head: spot,
            dragging: false,
        }
    }

    pub fn line(file: usize, column: Column, at: (u32, u32)) -> Self {
        let spot = Spot::at(at.0, at.1, 0);
        Self {
            file,
            column,
            extent: Extent::Lines,
            anchor: spot,
            head: spot,
            dragging: false,
        }
    }

    /// An empty character selection at `spot`, which a drag grows.
    pub fn caret(file: usize, column: Column, spot: Spot) -> Self {
        Self {
            file,
            column,
            extent: Extent::Chars,
            anchor: spot,
            head: spot,
            dragging: true,
        }
    }

    fn ends(&self) -> (Spot, Spot) {
        (self.anchor.min(self.head), self.anchor.max(self.head))
    }

    pub fn is_caret(&self) -> bool {
        self.extent == Extent::Chars && self.anchor == self.head
    }

    pub fn covers(&self, row: Row) -> bool {
        if row.file as usize != self.file {
            return false;
        }
        if self.extent == Extent::Hunk {
            return match row.kind {
                RowKind::Hunk { hunk } | RowKind::Line { hunk, .. } | RowKind::Pair { hunk, .. } => {
                    hunk == self.anchor.hunk
                }
                _ => false,
            };
        }
        let (first, last) = self.ends();
        column_line(row.kind, self.column).is_some_and(|at| (first.line()..=last.line()).contains(&at))
    }

    /// The selected bytes of one line. Lines between the ends are selected whole, and may be empty.
    pub fn chars_in(&self, hunk: u32, line: u32, len: usize) -> Option<Range<usize>> {
        if self.extent != Extent::Chars {
            return None;
        }
        let (first, last) = self.ends();
        let at = (hunk, line);
        if at < first.line() || at > last.line() {
            return None;
        }
        let from = if at == first.line() { first.byte.min(len) } else { 0 };
        let to = if at == last.line() { last.byte.min(len) } else { len };
        Some(from..to.max(from))
    }

    pub fn lines(&self, file: &FileDiff) -> Vec<(u32, u32)> {
        let shown = |kind: LineKind| match self.column {
            Column::Unified => true,
            Column::Left => kind != LineKind::Added,
            Column::Right => kind != LineKind::Deleted,
        };
        let (first, last) = match self.extent {
            Extent::Hunk => ((self.anchor.hunk, 0), (self.anchor.hunk, u32::MAX)),
            _ => {
                let (first, last) = self.ends();
                (first.line(), last.line())
            }
        };
        let mut out = Vec::new();
        for (hunk, source) in file.hunks.iter().enumerate() {
            let hunk = hunk as u32;
            if hunk < first.0 || hunk > last.0 {
                continue;
            }
            for (line, diff_line) in source.lines.iter().enumerate() {
                let at = (hunk, line as u32);
                if at >= first && at <= last && shown(diff_line.kind) {
                    out.push(at);
                }
            }
        }
        out
    }

    /// The text without the `+` and `-` markers.
    pub fn text(&self, file: &FileDiff) -> String {
        self.lines(file)
            .into_iter()
            .filter_map(|(hunk, line)| {
                let text = file.hunks[hunk as usize].lines[line as usize].text.as_str();
                match self.extent {
                    Extent::Chars => text.get(self.chars_in(hunk, line, text.len())?),
                    _ => Some(text),
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn class(ch: char) -> u8 {
    if ch.is_whitespace() {
        0
    } else if ch.is_alphanumeric() || ch == '_' {
        1
    } else {
        2
    }
}

/// The run of word characters, spaces, or punctuation that holds `byte`.
pub fn word_at(text: &str, byte: usize) -> Range<usize> {
    let Some(last) = text.char_indices().next_back() else {
        return 0..0;
    };
    let at = text.floor_char_boundary(byte.min(last.0));
    let kind = class(text[at..].chars().next().unwrap_or(last.1));
    let start = text[..at]
        .char_indices()
        .rev()
        .take_while(|&(_, ch)| class(ch) == kind)
        .last()
        .map_or(at, |(index, _)| index);
    let end = text[at..]
        .char_indices()
        .find(|&(_, ch)| class(ch) != kind)
        .map_or(text.len(), |(index, _)| at + index);
    start..end
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff_pane::testing::{diff, line};

    fn file() -> FileDiff {
        diff(
            "a.rs",
            vec![
                line(LineKind::Context, Some(1), Some(1), "let alpha = 1;"),
                line(LineKind::Added, None, Some(2), "beta gamma"),
                line(LineKind::Context, Some(2), Some(3), ""),
                line(LineKind::Context, Some(3), Some(4), "tail"),
            ],
        )
    }

    fn chars(from: (u32, usize), to: (u32, usize)) -> Selection {
        Selection {
            head: Spot::at(0, to.0, to.1),
            dragging: false,
            ..Selection::caret(0, Column::Unified, Spot::at(0, from.0, from.1))
        }
    }

    #[test]
    fn a_character_selection_inside_one_line_copies_the_substring() {
        assert_eq!(chars((0, 4), (0, 9)).text(&file()), "alpha");
    }

    #[test]
    fn a_character_selection_over_lines_copies_the_ends_and_the_lines_between() {
        assert_eq!(chars((0, 10), (3, 2)).text(&file()), "= 1;\nbeta gamma\n\nta");
    }

    #[test]
    fn dragging_backwards_selects_the_same_text() {
        assert_eq!(chars((1, 4), (0, 4)).text(&file()), "alpha = 1;\nbeta");
    }

    #[test]
    fn lines_copy_whole_text_without_markers() {
        let selection = Selection {
            head: Spot::at(0, 1, 0),
            ..Selection::line(0, Column::Unified, (0, 0))
        };
        assert_eq!(selection.text(&file()), "let alpha = 1;\nbeta gamma");
    }

    #[test]
    fn a_split_column_only_holds_the_lines_it_shows() {
        let selection = Selection {
            head: Spot::at(0, 3, 0),
            ..Selection::line(0, Column::Left, (0, 0))
        };
        assert_eq!(selection.lines(&file()), [(0, 0), (0, 2), (0, 3)]);
    }

    #[test]
    fn a_hunk_selection_holds_every_line_of_the_hunk() {
        assert_eq!(Selection::hunk(0, 0).lines(&file()).len(), 4);
    }

    #[test]
    fn a_word_is_a_run_of_one_kind_of_character() {
        let text = "let alpha_1 = foo(bar);";
        assert_eq!(&text[word_at(text, 5)], "alpha_1");
        assert_eq!(&text[word_at(text, 3)], " ");
        assert_eq!(&text[word_at(text, 17)], "(");
        assert_eq!(&text[word_at(text, 100)], ");");
        assert_eq!(word_at("", 0), 0..0);
        assert_eq!(&"héllo wörld"[word_at("héllo wörld", 2)], "héllo");
    }
}
