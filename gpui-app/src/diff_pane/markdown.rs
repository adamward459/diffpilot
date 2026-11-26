use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use std::ops::Range;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub strike: bool,
    pub link: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rich {
    pub text: String,
    pub spans: Vec<(Range<usize>, Style)>,
}

impl Rich {
    fn push(&mut self, text: &str, style: Style) {
        let start = self.text.len();
        self.text.push_str(text);
        if !text.is_empty() {
            self.spans.push((start..self.text.len(), style));
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Block {
    Heading { level: u8, text: Rich },
    Paragraph(Rich),
    Code(String),
    Quote(Vec<Block>),
    List { start: Option<u64>, items: Vec<Vec<Block>> },
    Rule,
    Table { head: Vec<Rich>, rows: Vec<Vec<Rich>> },
}

#[derive(Default)]
struct TableBuilder {
    head: Vec<Rich>,
    rows: Vec<Vec<Rich>>,
    row: Vec<Rich>,
}

enum Frame {
    Root(Vec<Block>),
    Quote(Vec<Block>),
    Item(Vec<Block>),
    List { start: Option<u64>, items: Vec<Vec<Block>> },
    Table(TableBuilder),
}

#[derive(Default)]
struct Marks {
    emphasis: u32,
    strong: u32,
    strike: u32,
    link: u32,
}

struct Builder {
    frames: Vec<Frame>,
    inline: Option<Rich>,
    code: Option<String>,
    marks: Marks,
}

impl Builder {
    fn push_block(&mut self, block: Block) {
        if let Some(Frame::Root(blocks) | Frame::Quote(blocks) | Frame::Item(blocks)) = self.frames.last_mut() {
            blocks.push(block);
        }
    }

    fn flush(&mut self, make: impl FnOnce(Rich) -> Block) {
        if let Some(rich) = self.inline.take()
            && !rich.text.is_empty()
        {
            self.push_block(make(rich));
        }
    }

    fn text(&mut self, text: &str, code: bool) {
        if let Some(buffer) = &mut self.code {
            buffer.push_str(text);
            return;
        }
        let style = Style {
            bold: self.marks.strong > 0,
            italic: self.marks.emphasis > 0,
            strike: self.marks.strike > 0,
            link: self.marks.link > 0,
            code,
        };
        self.inline.get_or_insert_with(Rich::default).push(text, style);
    }

    fn start(&mut self, tag: Tag) {
        match tag {
            Tag::Paragraph | Tag::Heading { .. } | Tag::TableCell => {
                self.flush(Block::Paragraph);
                self.inline = Some(Rich::default());
            }
            Tag::BlockQuote(_) => {
                self.flush(Block::Paragraph);
                self.frames.push(Frame::Quote(Vec::new()));
            }
            Tag::CodeBlock(_) => {
                self.flush(Block::Paragraph);
                self.code = Some(String::new());
            }
            Tag::List(start) => {
                self.flush(Block::Paragraph);
                self.frames.push(Frame::List {
                    start,
                    items: Vec::new(),
                });
            }
            Tag::Item => self.frames.push(Frame::Item(Vec::new())),
            Tag::Table(_) => self.frames.push(Frame::Table(TableBuilder::default())),
            Tag::Emphasis => self.marks.emphasis += 1,
            Tag::Strong => self.marks.strong += 1,
            Tag::Strikethrough => self.marks.strike += 1,
            Tag::Link { .. } => self.marks.link += 1,
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => self.flush(Block::Paragraph),
            TagEnd::Heading(level) => self.flush(|text| Block::Heading {
                level: level_number(level),
                text,
            }),
            TagEnd::TableCell => {
                let cell = self.inline.take().unwrap_or_default();
                if let Some(Frame::Table(table)) = self.frames.last_mut() {
                    table.row.push(cell);
                }
            }
            TagEnd::TableHead => {
                if let Some(Frame::Table(table)) = self.frames.last_mut() {
                    table.head = std::mem::take(&mut table.row);
                }
            }
            TagEnd::TableRow => {
                if let Some(Frame::Table(table)) = self.frames.last_mut() {
                    let row = std::mem::take(&mut table.row);
                    table.rows.push(row);
                }
            }
            TagEnd::Table => {
                if let Some(Frame::Table(table)) = self.frames.pop() {
                    self.push_block(Block::Table {
                        head: table.head,
                        rows: table.rows,
                    });
                }
            }
            TagEnd::CodeBlock => {
                if let Some(code) = self.code.take() {
                    self.push_block(Block::Code(code.trim_end_matches('\n').to_string()));
                }
            }
            TagEnd::BlockQuote(_) => {
                self.flush(Block::Paragraph);
                if let Some(Frame::Quote(blocks)) = self.frames.pop() {
                    self.push_block(Block::Quote(blocks));
                }
            }
            TagEnd::Item => {
                self.flush(Block::Paragraph);
                if let Some(Frame::Item(blocks)) = self.frames.pop()
                    && let Some(Frame::List { items, .. }) = self.frames.last_mut()
                {
                    items.push(blocks);
                }
            }
            TagEnd::List(_) => {
                if let Some(Frame::List { start, items }) = self.frames.pop() {
                    self.push_block(Block::List { start, items });
                }
            }
            TagEnd::Emphasis => self.marks.emphasis = self.marks.emphasis.saturating_sub(1),
            TagEnd::Strong => self.marks.strong = self.marks.strong.saturating_sub(1),
            TagEnd::Strikethrough => self.marks.strike = self.marks.strike.saturating_sub(1),
            TagEnd::Link => self.marks.link = self.marks.link.saturating_sub(1),
            _ => {}
        }
    }
}

fn level_number(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

pub fn parse(source: &str) -> Vec<Block> {
    let options = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut builder = Builder {
        frames: vec![Frame::Root(Vec::new())],
        inline: None,
        code: None,
        marks: Marks::default(),
    };
    for event in Parser::new_ext(source, options) {
        match event {
            Event::Start(tag) => builder.start(tag),
            Event::End(tag) => builder.end(tag),
            Event::Text(text) => builder.text(&text, false),
            Event::Code(text) => builder.text(&text, true),
            Event::SoftBreak => builder.text(" ", false),
            Event::HardBreak => builder.text("\n", false),
            Event::TaskListMarker(done) => builder.text(if done { "[x] " } else { "[ ] " }, false),
            Event::Rule => {
                builder.flush(Block::Paragraph);
                builder.push_block(Block::Rule);
            }
            _ => {}
        }
    }
    builder.flush(Block::Paragraph);
    match builder.frames.pop() {
        Some(Frame::Root(blocks)) => blocks,
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paragraph(text: &str) -> Block {
        Block::Paragraph(Rich {
            text: text.into(),
            spans: vec![(0..text.len(), Style::default())],
        })
    }

    #[test]
    fn splits_headings_paragraphs_and_rules() {
        let blocks = parse("# Title\n\nbody\n\n---\n");
        assert_eq!(blocks.len(), 3);
        assert!(matches!(&blocks[0], Block::Heading { level: 1, text } if text.text == "Title"));
        assert_eq!(blocks[1], paragraph("body"));
        assert_eq!(blocks[2], Block::Rule);
    }

    #[test]
    fn marks_inline_styles_over_their_text() {
        let blocks = parse("a **b** `c` [d](x)");
        let Block::Paragraph(rich) = &blocks[0] else {
            panic!("{blocks:?}")
        };
        assert_eq!(rich.text, "a b c d");
        let style = |at: usize| rich.spans.iter().find(|(range, _)| range.contains(&at)).unwrap().1;
        assert!(style(2).bold);
        assert!(style(4).code);
        assert!(style(6).link);
        assert!(!style(0).bold);
    }

    #[test]
    fn nests_lists_and_quotes() {
        let blocks = parse("- one\n- two\n  - inner\n\n> quoted\n\n1. first\n");
        let Block::List { start: None, items } = &blocks[0] else {
            panic!("{blocks:?}")
        };
        assert_eq!(items.len(), 2);
        assert!(matches!(&items[1][1], Block::List { items, .. } if items.len() == 1));
        assert!(matches!(&blocks[1], Block::Quote(inner) if inner.len() == 1));
        assert!(matches!(&blocks[2], Block::List { start: Some(1), .. }));
    }

    #[test]
    fn keeps_code_blocks_whole_and_reads_tables() {
        let blocks = parse("```rs\nlet a = 1;\n\nlet b = 2;\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |\n");
        assert_eq!(blocks[0], Block::Code("let a = 1;\n\nlet b = 2;".into()));
        let Block::Table { head, rows } = &blocks[1] else {
            panic!("{blocks:?}")
        };
        assert_eq!((head.len(), rows.len(), rows[0].len()), (2, 1, 2));
        assert_eq!(rows[0][1].text, "2");
    }
}
