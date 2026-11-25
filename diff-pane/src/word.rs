use diffpilot_core::diff::{DiffLine, LineKind};
use similar::{ChangeTag, TextDiff};
use std::collections::VecDeque;
use std::ops::Range;

const WORD_DIFF_MAX_BYTES: usize = 500;
const WORD_DIFF_MIN_SAME: f32 = 0.4;

/// Pairs each deleted line with the added line that replaces it, like the web page: the first
/// pending deletion pairs with the next added line, and a context line drops the pending ones.
pub fn pair_lines(lines: &[DiffLine]) -> Vec<Option<u32>> {
    let mut pairs = vec![None; lines.len()];
    let mut pending: VecDeque<usize> = VecDeque::new();
    for (index, line) in lines.iter().enumerate() {
        match line.kind {
            LineKind::Deleted => pending.push_back(index),
            LineKind::Added => {
                if let Some(deleted) = pending.pop_front() {
                    pairs[deleted] = Some(index as u32);
                    pairs[index] = Some(deleted as u32);
                }
            }
            LineKind::Context => pending.clear(),
        }
    }
    pairs
}

fn push_merged(ranges: &mut Vec<Range<usize>>, range: Range<usize>) {
    match ranges.last_mut() {
        Some(last) if last.end == range.start => last.end = range.end,
        _ => ranges.push(range),
    }
}

pub struct WordMarks {
    pub deleted: Vec<Range<usize>>,
    pub added: Vec<Range<usize>>,
}

/// Byte ranges that differ in the deleted and the added text. `None` when the two lines
/// are too long or too different for word highlights to help.
pub fn word_ranges(deleted: &str, added: &str) -> Option<WordMarks> {
    if deleted.len() > WORD_DIFF_MAX_BYTES || added.len() > WORD_DIFF_MAX_BYTES {
        return None;
    }
    let diff = TextDiff::configure().diff_unicode_words(deleted, added);
    let (mut deleted_at, mut added_at, mut same) = (0, 0, 0);
    let (mut deleted_ranges, mut added_ranges) = (Vec::new(), Vec::new());
    for change in diff.iter_all_changes() {
        let len = change.value().len();
        match change.tag() {
            ChangeTag::Delete => {
                push_merged(&mut deleted_ranges, deleted_at..deleted_at + len);
                deleted_at += len;
            }
            ChangeTag::Insert => {
                push_merged(&mut added_ranges, added_at..added_at + len);
                added_at += len;
            }
            ChangeTag::Equal => {
                deleted_at += len;
                added_at += len;
                same += len;
            }
        }
    }
    let longest = deleted.len().max(added.len());
    if (same as f32) < longest as f32 * WORD_DIFF_MIN_SAME {
        return None;
    }
    Some(WordMarks {
        deleted: deleted_ranges,
        added: added_ranges,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::line;

    fn kinds(text: &str) -> Vec<DiffLine> {
        text.chars()
            .map(|c| match c {
                'd' => line(LineKind::Deleted, Some(1), None, "x"),
                'a' => line(LineKind::Added, None, Some(1), "x"),
                _ => line(LineKind::Context, Some(1), Some(1), "x"),
            })
            .collect()
    }

    #[test]
    fn pairs_deletions_with_additions_in_order() {
        assert_eq!(pair_lines(&kinds("ddaaa")), [Some(2), Some(3), Some(0), Some(1), None]);
    }

    #[test]
    fn interleaved_changes_pair_as_they_come() {
        assert_eq!(pair_lines(&kinds("dada")), [Some(1), Some(0), Some(3), Some(2)]);
    }

    #[test]
    fn context_lines_drop_pending_deletions() {
        assert_eq!(pair_lines(&kinds("dcad")), [None, None, None, None]);
    }

    #[test]
    fn lone_additions_and_deletions_have_no_partner() {
        assert_eq!(pair_lines(&kinds("aadd")), [None; 4]);
    }

    #[test]
    fn marks_the_words_that_changed() {
        let marks = word_ranges("let a = 1;", "let b = 1;").unwrap();
        assert_eq!(marks.deleted.len(), 1);
        assert_eq!(marks.deleted[0], 4..5);
        assert_eq!(marks.added.len(), 1);
        assert_eq!(marks.added[0], 4..5);
    }

    #[test]
    fn marks_inserted_text_on_one_side_only() {
        let marks = word_ranges("call(a)", "call(a, b)").unwrap();
        assert!(marks.deleted.is_empty());
        let covered: usize = marks.added.iter().map(|range| range.len()).sum();
        assert_eq!(covered, 3);
    }

    #[test]
    fn unrelated_lines_get_no_word_marks() {
        assert!(word_ranges("completely different", "nothing alike here!!").is_none());
    }

    #[test]
    fn long_lines_are_skipped() {
        let long = "word ".repeat(150);
        assert!(word_ranges(&long, &long).is_none());
    }

    #[test]
    fn ranges_use_byte_offsets_for_non_ascii_text() {
        let marks = word_ranges("caf\u{e9} one two", "caf\u{e9} six two").unwrap();
        assert_eq!(&"caf\u{e9} one two"[marks.deleted[0].clone()], "one");
        assert_eq!(&"caf\u{e9} six two"[marks.added[0].clone()], "six");
    }
}
