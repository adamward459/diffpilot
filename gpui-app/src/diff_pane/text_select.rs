use gpui::{
    Bounds, Div, Hsla, ParentElement as _, Pixels, Point, SharedString, Styled as _, StyledText, TextLayout, TextRun,
    canvas,
};
use std::cell::{Cell, RefCell};
use std::ops::Range;
use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Spot {
    pub piece: usize,
    pub byte: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextSelection {
    pub anchor: Spot,
    pub head: Spot,
    pub dragging: bool,
}

impl TextSelection {
    pub fn caret(at: Spot) -> Self {
        Self {
            anchor: at,
            head: at,
            dragging: true,
        }
    }

    pub fn ordered(self) -> (Spot, Spot) {
        (self.anchor.min(self.head), self.anchor.max(self.head))
    }

    pub fn is_empty(self) -> bool {
        self.anchor == self.head
    }

    pub fn range_in(self, piece: usize, len: usize) -> Option<Range<usize>> {
        let (start, end) = self.ordered();
        if self.is_empty() || piece < start.piece || piece > end.piece {
            return None;
        }
        let from = if piece == start.piece { start.byte.min(len) } else { 0 };
        let to = if piece == end.piece { end.byte.min(len) } else { len };
        (from < to).then_some(from..to)
    }
}

struct Piece {
    layout: TextLayout,
    text: SharedString,
    painted: Rc<Cell<Option<Bounds<Pixels>>>>,
}

/// The selectable text runs of one rendered card, in reading order. The card rebuilds it on every render.
#[derive(Clone, Default)]
pub struct Pieces {
    pieces: Rc<RefCell<Vec<Piece>>>,
    selection: Option<TextSelection>,
    highlight: Option<Hsla>,
}

impl Pieces {
    pub fn new(selection: Option<TextSelection>, highlight: Hsla) -> Self {
        Self {
            pieces: Rc::default(),
            selection,
            highlight: Some(highlight),
        }
    }

    pub fn text(&self, frame: Div, text: SharedString, runs: Vec<TextRun>) -> Div {
        let index = self.pieces.borrow().len();
        let runs = match (
            self.selection.and_then(|s| s.range_in(index, text.len())),
            self.highlight,
        ) {
            (Some(range), Some(color)) => highlight(runs, range, color),
            _ => runs,
        };
        let styled = StyledText::new(text.clone()).with_runs(runs);
        let painted: Rc<Cell<Option<Bounds<Pixels>>>> = Rc::default();
        self.pieces.borrow_mut().push(Piece {
            layout: styled.layout().clone(),
            text,
            painted: painted.clone(),
        });
        frame.relative().child(styled).child(
            canvas(move |bounds, _, _| painted.set(Some(bounds)), |_, _, _, _| {})
                .absolute()
                .inset_0(),
        )
    }

    /// The piece under `at`, or the nearest one above or below it, and the byte there.
    pub fn spot_at(&self, at: Point<Pixels>) -> Option<Spot> {
        let pieces = self.pieces.borrow();
        let distance = |bounds: Bounds<Pixels>| {
            let y = f32::from(at.y);
            let (top, bottom) = (f32::from(bounds.top()), f32::from(bounds.bottom()));
            let x = f32::from(at.x);
            let (left, right) = (f32::from(bounds.left()), f32::from(bounds.right()));
            let dy = (top - y).max(y - bottom).max(0.0);
            let dx = (left - x).max(x - right).max(0.0);
            (dy, dx)
        };
        let (piece, _) = pieces
            .iter()
            .enumerate()
            .filter_map(|(index, piece)| piece.painted.get().map(|bounds| (index, distance(bounds))))
            .min_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))?;
        let bounds = pieces[piece].painted.get()?;
        let byte = if at.y < bounds.top() {
            0
        } else if at.y > bounds.bottom() {
            pieces[piece].text.len()
        } else {
            let clamped = Point::new(at.x.clamp(bounds.left(), bounds.right()), at.y);
            match pieces[piece].layout.index_for_position(clamped) {
                Ok(byte) | Err(byte) => byte,
            }
        };
        Some(Spot { piece, byte })
    }

    /// The word under `at` for a double click, or its whole block for a triple click.
    pub fn expand(&self, at: Spot, clicks: usize) -> TextSelection {
        let pieces = self.pieces.borrow();
        let Some(text) = pieces.get(at.piece).map(|piece| piece.text.clone()) else {
            return TextSelection::caret(at);
        };
        let range = match clicks {
            2 => crate::diff_pane::selection::word_at(&text, at.byte),
            _ => 0..text.len(),
        };
        TextSelection {
            anchor: Spot {
                piece: at.piece,
                byte: range.start,
            },
            head: Spot {
                piece: at.piece,
                byte: range.end,
            },
            dragging: false,
        }
    }

    pub fn selected_text(&self, selection: TextSelection) -> String {
        let pieces = self.pieces.borrow();
        let (start, end) = selection.ordered();
        (start.piece..=end.piece.min(pieces.len().saturating_sub(1)))
            .filter_map(|index| {
                let text = &pieces.get(index)?.text;
                let range = selection.range_in(index, text.len())?;
                text.get(range)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn highlight(runs: Vec<TextRun>, range: Range<usize>, color: Hsla) -> Vec<TextRun> {
    let mut out = Vec::with_capacity(runs.len() + 2);
    let mut at = 0;
    for run in runs {
        let (start, end) = (at, at + run.len);
        at = end;
        let cuts = [start, range.start.clamp(start, end), range.end.clamp(start, end), end];
        for pair in cuts.windows(2) {
            let len = pair[1] - pair[0];
            if len == 0 {
                continue;
            }
            let inside = pair[0] >= range.start && pair[1] <= range.end;
            out.push(TextRun {
                len,
                background_color: if inside { Some(color) } else { run.background_color },
                ..run.clone()
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::font;

    fn spot(piece: usize, byte: usize) -> Spot {
        Spot { piece, byte }
    }

    fn selection(anchor: Spot, head: Spot) -> TextSelection {
        TextSelection {
            anchor,
            head,
            dragging: false,
        }
    }

    #[test]
    fn a_backward_drag_covers_the_same_text_as_a_forward_one() {
        let forward = selection(spot(0, 2), spot(2, 3));
        let backward = selection(spot(2, 3), spot(0, 2));
        for piece in 0..3 {
            assert_eq!(forward.range_in(piece, 10), backward.range_in(piece, 10));
        }
        assert_eq!(forward.range_in(0, 10), Some(2..10));
        assert_eq!(forward.range_in(1, 10), Some(0..10));
        assert_eq!(forward.range_in(2, 10), Some(0..3));
        assert_eq!(forward.range_in(3, 10), None);
    }

    #[test]
    fn a_caret_selects_nothing() {
        assert_eq!(selection(spot(1, 4), spot(1, 4)).range_in(1, 10), None);
    }

    #[test]
    fn the_highlight_splits_runs_at_the_selection_edges() {
        let run = |len| TextRun {
            len,
            font: font("Menlo"),
            color: Hsla::default(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let color = gpui::red();
        let out = highlight(vec![run(4), run(6)], 2..7, color);
        let lens: Vec<_> = out.iter().map(|run| run.len).collect();
        let lit: Vec<_> = out.iter().map(|run| run.background_color == Some(color)).collect();
        assert_eq!(lens, vec![2, 2, 3, 3]);
        assert_eq!(lit, vec![false, true, true, false]);
    }
}
