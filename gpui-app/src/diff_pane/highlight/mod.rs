mod runs;
mod theme_data;

pub use runs::{Piece, chunk_pieces, display_len, pieces, source_offset};

use diffpilot_core::diff::{FileDiff, LineKind};
use diffpilot_core::threads::Side;
use std::collections::HashMap;
use std::sync::OnceLock;
use syntect::highlighting::{
    Color, HighlightState, Highlighter, RangedHighlightIterator, ScopeSelectors, StyleModifier, Theme, ThemeItem,
    ThemeSettings,
};
use syntect::parsing::{ParseState, ScopeStack, SyntaxReference, SyntaxSet};
use theme_data::Rule;

const MAX_LINE_BYTES: usize = 2000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub len: u32,
    pub light: u32,
    pub dark: u32,
}

impl Span {
    pub fn color(self, dark: bool) -> u32 {
        if dark { self.dark } else { self.light }
    }
}

/// Highlight spans for every diff line of one file, indexed by hunk and line. A context line keeps the spans
/// of the new side, and the old side's own where they differ.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct FileTokens {
    hunk_start: Vec<u32>,
    line_end: Vec<u32>,
    spans: Vec<Span>,
    old_context: HashMap<u32, Vec<Span>>,
}

impl FileTokens {
    pub fn line(&self, hunk: usize, line: usize) -> &[Span] {
        let Some(&base) = self.hunk_start.get(hunk) else {
            return &[];
        };
        let index = base as usize + line;
        let Some(&end) = self.line_end.get(index) else {
            return &[];
        };
        let start = if index == 0 { 0 } else { self.line_end[index - 1] };
        &self.spans[start as usize..end as usize]
    }

    pub fn old_context_differs(&self, hunk: usize, line: usize) -> bool {
        self.hunk_start
            .get(hunk)
            .is_some_and(|&base| self.old_context.contains_key(&(base + line as u32)))
    }

    pub fn line_on(&self, side: Side, hunk: usize, line: usize) -> &[Span] {
        let old = match (side, self.hunk_start.get(hunk)) {
            (Side::Old, Some(&base)) => self.old_context.get(&(base + line as u32)),
            _ => None,
        };
        old.map_or_else(|| self.line(hunk, line), Vec::as_slice)
    }

    pub fn span_count(&self) -> usize {
        self.spans.len()
    }
}

pub struct Engine {
    syntaxes: SyntaxSet,
    dark: Theme,
    light: Theme,
}

pub fn engine() -> &'static Engine {
    static ENGINE: OnceLock<Engine> = OnceLock::new();
    ENGINE.get_or_init(|| Engine {
        syntaxes: two_face::syntax::extra_newlines(),
        dark: theme(theme_data::DARK_PLUS, theme_data::DARK_PLUS_FOREGROUND),
        light: theme(theme_data::LIGHT_PLUS, theme_data::LIGHT_PLUS_FOREGROUND),
    })
}

fn color(rgb: u32) -> Color {
    Color {
        r: (rgb >> 16) as u8,
        g: (rgb >> 8) as u8,
        b: rgb as u8,
        a: 255,
    }
}

fn rgb(color: Color) -> u32 {
    (u32::from(color.r) << 16) | (u32::from(color.g) << 8) | u32::from(color.b)
}

fn theme(rules: &[Rule], foreground: u32) -> Theme {
    Theme {
        name: None,
        author: None,
        settings: ThemeSettings {
            foreground: Some(color(foreground)),
            ..ThemeSettings::default()
        },
        scopes: rules
            .iter()
            .filter_map(|rule| {
                Some(ThemeItem {
                    scope: rule.scopes.parse::<ScopeSelectors>().ok()?,
                    style: StyleModifier {
                        foreground: Some(color(rule.color)),
                        ..StyleModifier::default()
                    },
                })
            })
            .collect(),
    }
}

impl Engine {
    pub fn syntax_for(&self, path: &str) -> Option<&SyntaxReference> {
        let name = path.rsplit('/').next().unwrap_or(path);
        let extension = name.rsplit_once('.').map(|(_, ext)| ext.to_ascii_lowercase())?;
        let alias = match extension.as_str() {
            "mts" | "cts" => "ts",
            "mjs" | "cjs" => "js",
            "mdx" => "md",
            "kts" => "kt",
            other => other,
        };
        self.syntaxes
            .find_syntax_by_extension(alias)
            .filter(|syntax| syntax.name != "Plain Text")
    }

    pub fn highlight(&self, file: &FileDiff) -> Option<FileTokens> {
        let syntax = self.syntax_for(&file.path)?;
        let dark = Highlighter::new(&self.dark);
        let light = Highlighter::new(&self.light);
        let mut old = Stream::new(syntax, &dark, &light);
        let mut new = Stream::new(syntax, &dark, &light);
        let mut tokens = FileTokens::default();
        let mut buffer = String::new();
        let mut scratch = Scratch::default();
        let mut old_spans = Vec::new();
        for hunk in &file.hunks {
            tokens.hunk_start.push(tokens.line_end.len() as u32);
            for line in &hunk.lines {
                buffer.clear();
                buffer.push_str(&line.text);
                buffer.push('\n');
                let plain = line.text.len() > MAX_LINE_BYTES;
                let keep = match line.kind {
                    LineKind::Deleted => old.feed(&buffer, plain, self, &mut scratch),
                    LineKind::Added => new.feed(&buffer, plain, self, &mut scratch),
                    LineKind::Context => {
                        old_spans.clear();
                        old_spans.extend_from_slice(old.feed(&buffer, plain, self, &mut scratch));
                        let keep = new.feed(&buffer, plain, self, &mut scratch);
                        if old_spans != keep {
                            tokens
                                .old_context
                                .insert(tokens.line_end.len() as u32, old_spans.clone());
                        }
                        keep
                    }
                };
                tokens.spans.extend_from_slice(keep);
                tokens.line_end.push(tokens.spans.len() as u32);
            }
        }
        Some(tokens)
    }
}

#[derive(Default)]
struct Scratch {
    dark: Vec<(usize, u32)>,
    light: Vec<(usize, u32)>,
    spans: Vec<Span>,
}

struct Stream<'a> {
    parse: ParseState,
    dark: HighlightState,
    light: HighlightState,
    highlighters: (&'a Highlighter<'a>, &'a Highlighter<'a>),
}

impl<'a> Stream<'a> {
    fn new(syntax: &SyntaxReference, dark: &'a Highlighter<'a>, light: &'a Highlighter<'a>) -> Self {
        Self {
            parse: ParseState::new(syntax),
            dark: HighlightState::new(dark, ScopeStack::new()),
            light: HighlightState::new(light, ScopeStack::new()),
            highlighters: (dark, light),
        }
    }

    /// Advances the parser over `line` (which ends in a newline) and returns the spans of its text.
    fn feed<'s>(&mut self, line: &str, skip: bool, engine: &Engine, scratch: &'s mut Scratch) -> &'s [Span] {
        let content = line.len() - 1;
        scratch.spans.clear();
        let ops = if skip {
            None
        } else {
            self.parse.parse_line(line, &engine.syntaxes).ok()
        };
        let Some(ops) = ops else {
            if content > 0 {
                scratch.spans.push(Span {
                    len: content as u32,
                    light: theme_fg(&engine.light),
                    dark: theme_fg(&engine.dark),
                });
            }
            return &scratch.spans;
        };
        scratch.dark.clear();
        scratch.light.clear();
        collect(&mut self.dark, &ops, line, self.highlighters.0, &mut scratch.dark);
        collect(&mut self.light, &ops, line, self.highlighters.1, &mut scratch.light);
        merge(&scratch.dark, &scratch.light, content, &mut scratch.spans);
        &scratch.spans
    }
}

fn theme_fg(theme: &Theme) -> u32 {
    theme.settings.foreground.map_or(0, rgb)
}

fn collect(
    state: &mut HighlightState,
    ops: &[(usize, syntect::parsing::ScopeStackOp)],
    line: &str,
    highlighter: &Highlighter,
    out: &mut Vec<(usize, u32)>,
) {
    out.clear();
    for (style, _, range) in RangedHighlightIterator::new(state, ops, line, highlighter) {
        out.push((range.end, rgb(style.foreground)));
    }
}

fn merge(dark: &[(usize, u32)], light: &[(usize, u32)], content: usize, out: &mut Vec<Span>) {
    let (mut d, mut l, mut at) = (0, 0, 0);
    while at < content && d < dark.len() && l < light.len() {
        let end = dark[d].0.min(light[l].0).min(content);
        if end > at {
            let (dark_color, light_color) = (dark[d].1, light[l].1);
            match out.last_mut() {
                Some(last) if last.dark == dark_color && last.light == light_color => last.len += (end - at) as u32,
                _ => out.push(Span {
                    len: (end - at) as u32,
                    light: light_color,
                    dark: dark_color,
                }),
            }
            at = end;
        }
        if dark[d].0 <= at {
            d += 1;
        }
        if light[l].0 <= at {
            l += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use diffpilot_core::changes::FileStatus;
    use diffpilot_core::diff::{DiffLine, Hunk};

    fn diff(path: &str, lines: Vec<(LineKind, &str)>) -> FileDiff {
        let mut number = 0;
        let lines = lines
            .into_iter()
            .map(|(kind, text)| {
                number += 1;
                DiffLine {
                    kind,
                    old: (kind != LineKind::Added).then_some(number),
                    new: (kind != LineKind::Deleted).then_some(number),
                    text: text.into(),
                }
            })
            .collect();
        let mut file = FileDiff::new(path, FileStatus::Modified);
        file.hunks.push(Hunk {
            header: "@@".into(),
            lines,
        });
        file
    }

    fn lengths(spans: &[Span]) -> u32 {
        spans.iter().map(|span| span.len).sum()
    }

    #[test]
    fn spans_cover_every_byte_of_each_line() {
        let file = diff(
            "a.ts",
            vec![
                (LineKind::Context, "import { a } from './a';"),
                (LineKind::Deleted, "const old = 1; // gone"),
                (LineKind::Added, "const next = `x${a}`;"),
                (LineKind::Added, ""),
            ],
        );
        let tokens = engine().highlight(&file).expect("typescript is known");
        for (index, line) in file.hunks[0].lines.iter().enumerate() {
            assert_eq!(lengths(tokens.line(0, index)) as usize, line.text.len(), "line {index}");
        }
    }

    #[test]
    fn keywords_and_comments_get_the_vs_code_colors() {
        let file = diff("a.ts", vec![(LineKind::Added, "const a = 1; // note")]);
        let tokens = engine().highlight(&file).unwrap();
        let spans = tokens.line(0, 0);
        let first = spans[0];
        assert_eq!((first.dark, first.light), (0x569cd6, 0x0000ff));
        let last = spans.last().unwrap();
        assert_eq!((last.dark, last.light), (0x6a9955, 0x008000));
        assert!(spans.len() > 3);
    }

    #[test]
    fn old_and_new_sides_are_parsed_apart() {
        let file = diff(
            "a.ts",
            vec![
                (LineKind::Deleted, "/* old note"),
                (LineKind::Added, "const b = 2;"),
                (LineKind::Deleted, "still the old note */"),
                (LineKind::Context, "const c = 3;"),
            ],
        );
        let tokens = engine().highlight(&file).unwrap();
        let comment = 0x6a9955;
        assert!(tokens.line(0, 0).iter().all(|span| span.dark == comment));
        assert!(
            tokens.line(0, 1).iter().all(|span| span.dark != comment),
            "the new side has no open comment"
        );
        assert_eq!(tokens.line(0, 1)[0].dark, 0x569cd6);
        assert_eq!(tokens.line(0, 3)[0].dark, 0x569cd6);
    }

    #[test]
    fn a_context_line_keeps_the_old_sides_spans_where_they_differ() {
        let file = diff(
            "a.ts",
            vec![
                (LineKind::Deleted, "/* old note"),
                (LineKind::Context, "const c = 3;"),
                (LineKind::Context, "const d = 4;"),
            ],
        );
        let tokens = engine().highlight(&file).unwrap();
        let comment = 0x6a9955;
        assert!(tokens.line_on(Side::Old, 0, 1).iter().all(|span| span.dark == comment));
        assert_eq!(tokens.line_on(Side::New, 0, 1)[0].dark, 0x569cd6);
        assert_eq!(tokens.line(0, 1), tokens.line_on(Side::New, 0, 1));
        assert_eq!(tokens.line_on(Side::Old, 0, 0), tokens.line(0, 0));
        assert_eq!(tokens.line_on(Side::Old, 3, 0), &[]);
        assert!(tokens.old_context_differs(0, 1));
        assert!(!tokens.old_context_differs(0, 0));
        assert!(!tokens.old_context_differs(3, 0));
    }

    #[test]
    fn a_context_line_that_parses_the_same_on_both_sides_stores_one_set() {
        let file = diff("a.ts", vec![(LineKind::Context, "const c = 3;")]);
        let tokens = engine().highlight(&file).unwrap();
        assert!(tokens.old_context.is_empty());
        assert_eq!(tokens.line_on(Side::Old, 0, 0), tokens.line(0, 0));
    }

    #[test]
    fn unknown_extensions_are_left_plain() {
        let file = diff("notes.unknownext", vec![(LineKind::Added, "hello")]);
        assert!(engine().highlight(&file).is_none());
        let file = diff("Makefile", vec![(LineKind::Added, "hello")]);
        assert!(engine().highlight(&file).is_none());
    }

    #[test]
    fn typescript_variants_share_one_grammar() {
        for path in [
            "a.ts", "a.tsx", "a.mts", "a.cts", "a.js", "a.mjs", "a.json", "a.md", "a.yml", "a.swift", "a.kt", "a.java",
        ] {
            assert!(engine().syntax_for(path).is_some(), "{path}");
        }
    }

    #[test]
    fn very_long_lines_stay_plain_but_keep_their_length() {
        let long = "a".repeat(MAX_LINE_BYTES + 1);
        let file = diff(
            "a.ts",
            vec![(LineKind::Added, &long), (LineKind::Added, "const a = 1;")],
        );
        let tokens = engine().highlight(&file).unwrap();
        assert_eq!(lengths(tokens.line(0, 0)) as usize, long.len());
        assert_eq!(tokens.line(0, 1)[0].dark, 0x569cd6);
    }

    #[test]
    fn lines_outside_the_file_have_no_spans() {
        let tokens = FileTokens::default();
        assert!(tokens.line(3, 4).is_empty());
    }

    #[test]
    fn merge_splits_where_either_theme_changes_and_joins_equal_runs() {
        let dark = [(3, 1), (6, 2), (9, 2)];
        let light = [(4, 7), (9, 8)];
        let mut out = Vec::new();
        merge(&dark, &light, 9, &mut out);
        assert_eq!(
            out,
            [
                Span {
                    len: 3,
                    dark: 1,
                    light: 7
                },
                Span {
                    len: 1,
                    dark: 2,
                    light: 7
                },
                Span {
                    len: 5,
                    dark: 2,
                    light: 8
                },
            ]
        );
    }
}
