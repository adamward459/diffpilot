use super::Span;
use std::ops::Range;

/// A stretch of a line with one text color. `color` is `None` where no highlight covers the text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Piece {
    pub len: usize,
    pub color: Option<u32>,
    pub word: bool,
}

/// Cuts a line into pieces at every highlight span edge and every word-diff range edge.
pub fn pieces(text_len: usize, spans: &[Span], dark: bool, words: &[Range<usize>]) -> Vec<Piece> {
    let mut out: Vec<Piece> = Vec::with_capacity(spans.len() + words.len() * 2 + 1);
    let mut at = 0;
    let mut span_index = 0;
    let mut span_left = spans.first().map_or(0, |span| span.len as usize);
    let mut word_index = 0;
    while at < text_len {
        while span_index < spans.len() && span_left == 0 {
            span_index += 1;
            span_left = spans.get(span_index).map_or(0, |span| span.len as usize);
        }
        let in_span = span_index < spans.len();
        let color = spans.get(span_index).map(|span| span.color(dark));
        let span_end = if in_span { at + span_left } else { text_len };
        while word_index < words.len() && words[word_index].end <= at {
            word_index += 1;
        }
        let (word, word_end) = match words.get(word_index) {
            Some(range) if range.start <= at => (true, range.end),
            Some(range) => (false, range.start),
            None => (false, text_len),
        };
        let end = span_end.min(word_end).min(text_len).max(at + 1);
        let len = end - at;
        match out.last_mut() {
            Some(last) if last.color == color && last.word == word => last.len += len,
            _ => out.push(Piece { len, color, word }),
        }
        if in_span {
            span_left = span_left.saturating_sub(len);
        }
        at = end;
    }
    out
}

pub const TAB: &str = "    ";

/// The text of `range` with tabs widened to four spaces, and the pieces that cover it.
pub fn chunk_pieces(text: &str, range: Range<usize>, pieces: &[Piece]) -> (String, Vec<Piece>) {
    let mut display = String::with_capacity(range.len());
    let mut out: Vec<Piece> = Vec::with_capacity(pieces.len().min(8));
    let mut offset = 0;
    for piece in pieces {
        let start = offset.max(range.start);
        let end = (offset + piece.len).min(range.end);
        offset += piece.len;
        if start >= end {
            if offset >= range.end {
                break;
            }
            continue;
        }
        let before = display.len();
        for ch in text[start..end].chars() {
            if ch == '\t' {
                display.push_str(TAB);
            } else {
                display.push(ch);
            }
        }
        let len = display.len() - before;
        match out.last_mut() {
            Some(last) if last.color == piece.color && last.word == piece.word => last.len += len,
            _ => out.push(Piece {
                len,
                color: piece.color,
                word: piece.word,
            }),
        }
    }
    (display, out)
}

/// How many bytes `text` takes once its tabs are widened.
pub fn display_len(text: &str) -> usize {
    text.len() + text.bytes().filter(|&byte| byte == b'\t').count() * (TAB.len() - 1)
}

/// The byte of `text` nearest to byte `display` of the widened text. A tab counts as one character.
pub fn source_offset(text: &str, display: usize) -> usize {
    let mut shown = 0;
    for (index, ch) in text.char_indices() {
        let width = if ch == '\t' { TAB.len() } else { ch.len_utf8() };
        if display <= shown || (display - shown) * 2 < width {
            return index;
        }
        shown += width;
    }
    text.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(len: u32, dark: u32) -> Span {
        Span {
            len,
            light: dark + 1,
            dark,
        }
    }

    #[test]
    fn plain_text_is_one_uncolored_piece() {
        assert_eq!(
            pieces(5, &[], true, &[]),
            [Piece {
                len: 5,
                color: None,
                word: false
            }]
        );
        assert!(pieces(0, &[], true, &[]).is_empty());
    }

    #[test]
    fn spans_become_pieces_in_the_asked_appearance() {
        let spans = [span(3, 10), span(2, 20)];
        let dark = pieces(5, &spans, true, &[]);
        assert_eq!(
            dark,
            [
                Piece {
                    len: 3,
                    color: Some(10),
                    word: false
                },
                Piece {
                    len: 2,
                    color: Some(20),
                    word: false
                }
            ]
        );
        let light = pieces(5, &spans, false, &[]);
        assert_eq!(light[0].color, Some(11));
    }

    #[test]
    fn spans_that_end_early_leave_the_rest_uncolored() {
        let out = pieces(6, &[span(2, 10)], true, &[]);
        assert_eq!(
            out,
            [
                Piece {
                    len: 2,
                    color: Some(10),
                    word: false
                },
                Piece {
                    len: 4,
                    color: None,
                    word: false
                }
            ]
        );
    }

    #[test]
    fn word_ranges_split_spans_and_mark_pieces() {
        let out = pieces(8, &[span(8, 10)], true, &[2..4, 6..8]);
        assert_eq!(
            out,
            [
                Piece {
                    len: 2,
                    color: Some(10),
                    word: false
                },
                Piece {
                    len: 2,
                    color: Some(10),
                    word: true
                },
                Piece {
                    len: 2,
                    color: Some(10),
                    word: false
                },
                Piece {
                    len: 2,
                    color: Some(10),
                    word: true
                },
            ]
        );
    }

    #[test]
    fn pieces_always_add_up_to_the_text() {
        let spans = [span(1, 1), span(0, 2), span(3, 3)];
        let out = pieces(7, &spans, false, &[0..2, 5..6]);
        assert_eq!(out.iter().map(|piece| piece.len).sum::<usize>(), 7);
    }

    #[test]
    fn equal_neighbours_are_joined() {
        let out = pieces(4, &[span(2, 5), span(2, 5)], true, &[]);
        assert_eq!(
            out,
            [Piece {
                len: 4,
                color: Some(5),
                word: false
            }]
        );
    }

    #[test]
    fn chunks_slice_the_pieces_at_the_range() {
        let text = "abcdefgh";
        let all = [
            Piece {
                len: 3,
                color: Some(1),
                word: false,
            },
            Piece {
                len: 5,
                color: Some(2),
                word: true,
            },
        ];
        let (display, out) = chunk_pieces(text, 2..6, &all);
        assert_eq!(display, "cdef");
        assert_eq!(
            out,
            [
                Piece {
                    len: 1,
                    color: Some(1),
                    word: false
                },
                Piece {
                    len: 3,
                    color: Some(2),
                    word: true
                }
            ]
        );
        let (display, out) = chunk_pieces(text, 6..8, &all);
        assert_eq!(display, "gh");
        assert_eq!(
            out,
            [Piece {
                len: 2,
                color: Some(2),
                word: true
            }]
        );
    }

    #[test]
    fn chunks_widen_tabs_and_keep_piece_lengths_in_step() {
        let text = "\tab\tc";
        let all = [
            Piece {
                len: 3,
                color: Some(1),
                word: false,
            },
            Piece {
                len: 2,
                color: None,
                word: false,
            },
        ];
        let (display, out) = chunk_pieces(text, 0..text.len(), &all);
        assert_eq!(display, "    ab    c");
        assert_eq!(out.iter().map(|piece| piece.len).sum::<usize>(), display.len());
        assert_eq!(out[0].len, 6);
        assert_eq!(out[1].len, 5);
    }

    #[test]
    fn chunks_of_empty_text_are_empty() {
        let (display, out) = chunk_pieces("", 0..0, &[]);
        assert!(display.is_empty());
        assert!(out.is_empty());
    }

    #[test]
    fn display_and_source_offsets_agree_across_tabs() {
        let text = "\tab\tc";
        assert_eq!(display_len(text), 11);
        assert_eq!(display_len(&text[..1]), 4);
        for (display, source) in [(0, 0), (4, 1), (6, 3), (10, 4), (11, 5), (99, 5)] {
            assert_eq!(source_offset(text, display), source, "display {display}");
        }
        assert_eq!(source_offset(text, 1), 0);
        assert_eq!(source_offset(text, 3), 1);
    }
}
