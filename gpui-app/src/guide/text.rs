//! Guide strings hold `inline code` and `[[path#L12]]` refs. Nothing else is markup.
//! The ref syntax follows diffity's `guide-refs.ts` (MIT, see THIRD_PARTY_NOTICES).

use diffpilot_core::threads::Side;
use gpui::SharedString;
use std::ops::Range;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeRef {
    pub path: String,
    pub line: Option<u32>,
}

impl CodeRef {
    pub fn side(&self) -> Side {
        Side::New
    }

    fn label(&self) -> String {
        let name = self.path.rsplit('/').next().unwrap_or(&self.path);
        match self.line {
            Some(line) => format!("{name}:{line}"),
            None => name.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InlineKind {
    Code,
    Ref(CodeRef),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Inline {
    pub text: SharedString,
    pub spans: Vec<(Range<usize>, InlineKind)>,
}

impl Inline {
    pub fn parse(source: &str) -> Self {
        let mut text = String::with_capacity(source.len());
        let mut spans = Vec::new();
        let mut rest = source;
        while let Some(at) = rest.find(['`', '[']) {
            let (before, tail) = rest.split_at(at);
            text.push_str(before);
            if let Some(after) = tail.strip_prefix('`')
                && let Some(close) = after
                    .find(['`', '\n'])
                    .filter(|&close| close > 0 && after.as_bytes()[close] == b'`')
            {
                let start = text.len();
                text.push_str(&after[..close]);
                spans.push((start..text.len(), InlineKind::Code));
                rest = &after[close + 1..];
                continue;
            }
            if let Some((code_ref, used)) = parse_ref(tail) {
                let start = text.len();
                text.push_str(&code_ref.label());
                spans.push((start..text.len(), InlineKind::Ref(code_ref)));
                rest = &tail[used..];
                continue;
            }
            text.push_str(&tail[..1]);
            rest = &tail[1..];
        }
        text.push_str(rest);
        Self {
            text: text.into(),
            spans,
        }
    }

    pub fn refs(&self) -> impl Iterator<Item = (&Range<usize>, &CodeRef)> {
        self.spans.iter().filter_map(|(range, kind)| match kind {
            InlineKind::Ref(code_ref) => Some((range, code_ref)),
            InlineKind::Code => None,
        })
    }
}

/// `[[path]]` or `[[path#L12]]` or `[[path#L12-L20]]` at the start of `text`, and how many bytes it takes.
fn parse_ref(text: &str) -> Option<(CodeRef, usize)> {
    let inner_start = text.strip_prefix("[[")?;
    let close = inner_start.find("]]")?;
    let inner = &inner_start[..close];
    if inner.contains(['[', ']', '\n']) {
        return None;
    }
    let (path, line) = match inner.split_once('#') {
        Some((path, anchor)) => {
            let digits = anchor.strip_prefix('L')?;
            let first: String = digits.chars().take_while(char::is_ascii_digit).collect();
            let rest = &digits[first.len()..];
            let range_ok = rest.is_empty()
                || rest
                    .strip_prefix("-L")
                    .or(rest.strip_prefix('-'))
                    .is_some_and(|end| !end.is_empty() && end.chars().all(|c| c.is_ascii_digit()));
            if first.is_empty() || !range_ok {
                return None;
            }
            (path, first.parse().ok())
        }
        None => (inner, None),
    };
    let path = path.trim().trim_start_matches("./");
    (!path.is_empty()).then(|| {
        (
            CodeRef {
                path: path.to_string(),
                line,
            },
            2 + close + 2,
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(inline: &Inline) -> Vec<(&str, &InlineKind)> {
        inline
            .spans
            .iter()
            .map(|(range, kind)| (&inline.text[range.clone()], kind))
            .collect()
    }

    #[test]
    fn plain_text_stays_as_it_is() {
        let inline = Inline::parse("Nothing special [here] or `unclosed.");
        assert_eq!(inline.text, "Nothing special [here] or `unclosed.");
        assert!(inline.spans.is_empty());
    }

    #[test]
    fn code_spans_lose_their_backticks_and_keep_their_place() {
        let inline = Inline::parse("Call `start()` then `stop()`.");
        assert_eq!(inline.text, "Call start() then stop().");
        assert_eq!(
            kinds(&inline),
            [("start()", &InlineKind::Code), ("stop()", &InlineKind::Code)]
        );
    }

    #[test]
    fn a_ref_becomes_a_short_label_that_knows_its_target() {
        let inline = Inline::parse("See [[src/hooks/undo.ts#L16]] and [[README.md]].");
        assert_eq!(inline.text, "See undo.ts:16 and README.md.");
        let refs: Vec<&CodeRef> = inline.refs().map(|(_, r)| r).collect();
        assert_eq!(
            refs,
            [
                &CodeRef {
                    path: "src/hooks/undo.ts".into(),
                    line: Some(16)
                },
                &CodeRef {
                    path: "README.md".into(),
                    line: None
                },
            ]
        );
    }

    #[test]
    fn a_range_keeps_its_first_line() {
        let inline = Inline::parse("[[a.ts#L12-L20]]");
        assert_eq!(inline.refs().next().unwrap().1.line, Some(12));
    }

    #[test]
    fn a_ref_inside_code_stays_literal() {
        let inline = Inline::parse("Write `[[a.ts#L1]]` to link.");
        assert_eq!(inline.text, "Write [[a.ts#L1]] to link.");
        assert_eq!(inline.refs().count(), 0);
    }

    #[test]
    fn a_broken_ref_is_plain_text() {
        for bad in ["[[]]", "[[a.ts#12]]", "[[a.ts#L]]", "[[a.ts", "[[a\nb]]"] {
            assert_eq!(Inline::parse(bad).text, bad, "{bad}");
        }
    }
}
