use gpui::SharedString;
use std::ops::Range;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanKind {
    Code,
    Mention,
    Caret,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Markup {
    pub text: SharedString,
    pub spans: Vec<(Range<usize>, SpanKind)>,
}

impl Markup {
    pub fn parse(body: &str) -> Self {
        let (text, code) = strip_code(body);
        let mut spans: Vec<(Range<usize>, SpanKind)> = code.into_iter().map(|range| (range, SpanKind::Code)).collect();
        for range in mention_ranges(&text) {
            if !spans
                .iter()
                .any(|(code, _)| code.start < range.end && range.start < code.end)
            {
                spans.push((range, SpanKind::Mention));
            }
        }
        spans.sort_by_key(|(range, _)| range.start);
        Self {
            text: text.into(),
            spans,
        }
    }

    pub fn has_mention(&self) -> bool {
        self.spans.iter().any(|(_, kind)| *kind == SpanKind::Mention)
    }
}

/// Drops the backticks around `code` that sits on one line and returns where the code now is.
fn strip_code(body: &str) -> (String, Vec<Range<usize>>) {
    let mut text = String::with_capacity(body.len());
    let mut code = Vec::new();
    let mut rest = body;
    while let Some(open) = rest.find('`') {
        let after = &rest[open + 1..];
        let line_end = after.find('\n').unwrap_or(after.len());
        match after[..line_end].find('`') {
            Some(close) if close > 0 => {
                text.push_str(&rest[..open]);
                let start = text.len();
                text.push_str(&after[..close]);
                code.push(start..text.len());
                rest = &after[close + 1..];
            }
            _ => {
                text.push_str(&rest[..=open]);
                rest = after;
            }
        }
    }
    text.push_str(rest);
    (text, code)
}

const MENTION: &str = "@claude";

/// Same rule as `diffpilot_core::threads::mentions_claude`, with the places.
pub fn mention_ranges(text: &str) -> Vec<Range<usize>> {
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    text.char_indices()
        .filter(|(_, c)| *c == '@')
        .filter_map(|(start, _)| {
            let end = start + MENTION.len();
            let candidate = text.get(start..end)?;
            let named = candidate.eq_ignore_ascii_case(MENTION);
            let before_ok = !text[..start]
                .chars()
                .next_back()
                .is_some_and(|c| is_word(c) || c == '@');
            let after_ok = !text[end..].chars().next().is_some_and(is_word);
            (named && before_ok && after_ok).then_some(start..end)
        })
        .collect()
}

/// The `@` word being typed at the end of `text`, when it can still become `@claude`.
pub fn pending_mention(text: &str) -> Option<&str> {
    let at = text.rfind('@')?;
    let typed = &text[at + 1..];
    let starts_word = text[..at].chars().next_back().is_none_or(char::is_whitespace);
    let prefix = MENTION[1..].get(..typed.len())?;
    (starts_word && typed.len() < MENTION.len() - 1 && prefix.eq_ignore_ascii_case(typed)).then_some(typed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use diffpilot_core::threads::mentions_claude;

    #[test]
    fn inline_code_loses_its_backticks_and_keeps_its_place() {
        let markup = Markup::parse("Add the limit in `docs/a.md:2`. Then `x`.");
        assert_eq!(markup.text, "Add the limit in docs/a.md:2. Then x.");
        let found: Vec<&str> = markup
            .spans
            .iter()
            .map(|(range, _)| &markup.text[range.clone()])
            .collect();
        assert_eq!(found, ["docs/a.md:2", "x"]);
    }

    #[test]
    fn a_backtick_that_does_not_close_on_its_line_stays_as_text() {
        assert_eq!(Markup::parse("a ` b\nc ` d").text, "a ` b\nc ` d");
        assert_eq!(Markup::parse("empty `` pair").text, "empty `` pair");
    }

    #[test]
    fn mentions_match_the_core_rule() {
        let cases = [
            "@claude check this",
            "hey @Claude, look",
            "a\n@claude",
            "mail me@claude.com",
            "@claudette",
            "no mention",
            "@@claude",
            "first @claudette then @claude",
            "@claude@claude",
            "é@claude",
        ];
        for text in cases {
            assert_eq!(!mention_ranges(text).is_empty(), mentions_claude(text), "{text}");
        }
    }

    #[test]
    fn a_mention_inside_code_is_not_marked() {
        assert!(!Markup::parse("`@claude`").has_mention());
        assert!(Markup::parse("`x` @claude").has_mention());
    }

    #[test]
    fn spans_use_byte_ranges_of_the_stripped_text() {
        let markup = Markup::parse("é `a` @claude");
        for (range, _) in &markup.spans {
            assert!(markup.text.is_char_boundary(range.start) && markup.text.is_char_boundary(range.end));
        }
        assert_eq!(&markup.text[markup.spans[1].0.clone()], "@claude");
    }

    #[test]
    fn the_pending_mention_is_the_word_being_typed() {
        assert_eq!(pending_mention("hi @"), Some(""));
        assert_eq!(pending_mention("hi @Cl"), Some("Cl"));
        assert_eq!(pending_mention("@claud"), Some("claud"));
        assert_eq!(pending_mention("@claude"), None);
        assert_eq!(pending_mention("me@cl"), None);
        assert_eq!(pending_mention("hi @x"), None);
        assert_eq!(pending_mention("hi"), None);
    }
}
