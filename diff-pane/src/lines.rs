use crate::highlight::{Piece, Span, chunk_pieces, pieces};
use crate::layout::{CODE_PADDING, LINE, NUMBER_COLUMN, RIBBON, wrap_ranges};
use crate::theme::{Palette, hex};
use crate::word::{pair_lines, word_ranges};
use diffpilot_core::diff::{DiffLine, Hunk, LineKind};
use gpui::{App, Bounds, ContentMask, Font, Pixels, ShapedLine, TextRun, Window, fill, point, px, size};
use std::collections::HashMap;
use std::ops::Range;

pub const CODE_SIZE: f32 = 12.0;
pub const NUMBER_SIZE: f32 = 11.0;
const NUMBER_PADDING: f32 = 6.0;
const MAX_UNWRAPPED_BYTES: usize = 4096;
const MAX_CACHED_LINES: usize = 3000;
const EVICTIONS_PER_FRAME: usize = 256;

#[derive(Clone)]
pub struct Fonts {
    pub ui: Font,
    pub mono: Font,
    pub cell: f32,
}

pub struct Style<'a> {
    pub fonts: &'a Fonts,
    pub palette: &'a Palette,
    pub wrap: Option<usize>,
}

#[derive(Clone, Copy, Hash, PartialEq, Eq)]
pub struct LineKey {
    pub file: u32,
    pub hunk: u32,
    pub line: u32,
}

struct Prepared {
    last_used: u64,
    ranges: Vec<Range<usize>>,
    pieces: Vec<Piece>,
    old: Option<ShapedLine>,
    new: Option<ShapedLine>,
    chunks: Vec<Option<ShapedLine>>,
}

#[derive(Default)]
pub struct LineCache {
    style: Option<(bool, Option<usize>)>,
    frame: u64,
    map: HashMap<LineKey, Prepared>,
    pairs: HashMap<(u32, u32), Vec<Option<u32>>>,
}

pub struct LineRequest<'a> {
    pub key: LineKey,
    pub hunk: &'a Hunk,
    pub spans: &'a [Span],
    pub y: f32,
    pub height: f32,
}

pub struct PaintLine {
    pub y: f32,
    pub height: f32,
    pub kind: LineKind,
    pub old: Option<ShapedLine>,
    pub new: Option<ShapedLine>,
    pub chunks: Vec<(usize, ShapedLine)>,
}

pub struct Thumb {
    pub top: f32,
    pub height: f32,
    pub opacity: f32,
}

pub struct Frame {
    pub scroll_x: f32,
    pub palette: Palette,
    pub lines: Vec<PaintLine>,
    pub thumb: Option<Thumb>,
}

impl LineCache {
    pub fn clear(&mut self) {
        self.map.clear();
        self.pairs.clear();
    }

    pub fn invalidate_file(&mut self, file: u32) {
        self.map.retain(|key, _| key.file != file);
    }

    pub fn begin_frame(&mut self, style: &Style) {
        let key = (style.palette.dark, style.wrap);
        self.frame += 1;
        if self.style != Some(key) {
            self.style = Some(key);
            self.clear();
        } else if self.map.len() > MAX_CACHED_LINES {
            let frame = self.frame;
            self.map
                .extract_if(|_, prepared| prepared.last_used + 1 < frame)
                .take(EVICTIONS_PER_FRAME)
                .for_each(drop);
        }
    }

    /// Shapes what is missing for one line and returns what to paint. `y` is the row top inside the viewport.
    pub fn line(&mut self, style: &Style, window: &mut Window, request: LineRequest, viewport: f32) -> PaintLine {
        let LineRequest {
            key,
            hunk,
            spans,
            y,
            height,
        } = request;
        let line = &hunk.lines[key.line as usize];
        let prepared = self.map.entry(key).or_insert_with(|| {
            let pairs = self
                .pairs
                .entry((key.file, key.hunk))
                .or_insert_with(|| pair_lines(&hunk.lines));
            let words = word_marks(pairs, hunk, key.line as usize);
            prepare(style, window, line, spans, &words)
        });
        prepared.last_used = self.frame;
        let mut chunks = Vec::new();
        for index in 0..prepared.ranges.len() {
            let top = y + index as f32 * LINE;
            if top + LINE <= 0.0 || top >= viewport {
                continue;
            }
            if prepared.chunks[index].is_none() {
                let shaped = shape_chunk(style, window, line, &prepared.ranges[index], &prepared.pieces);
                prepared.chunks[index] = Some(shaped);
            }
            if let Some(shaped) = &prepared.chunks[index] {
                chunks.push((index, shaped.clone()));
            }
        }
        PaintLine {
            y,
            height,
            kind: line.kind,
            old: prepared.old.clone(),
            new: prepared.new.clone(),
            chunks,
        }
    }
}

fn word_marks(pairs: &[Option<u32>], hunk: &Hunk, index: usize) -> Vec<Range<usize>> {
    let Some(partner) = pairs[index] else { return Vec::new() };
    let (own, other) = (&hunk.lines[index], &hunk.lines[partner as usize]);
    let (deleted, added) = if own.kind == LineKind::Deleted {
        (own, other)
    } else {
        (other, own)
    };
    match word_ranges(&deleted.text, &added.text) {
        Some(marks) if own.kind == LineKind::Deleted => marks.deleted,
        Some(marks) => marks.added,
        None => Vec::new(),
    }
}

fn prepare(style: &Style, window: &mut Window, line: &DiffLine, spans: &[Span], words: &[Range<usize>]) -> Prepared {
    let text = line.text.as_str();
    let ranges = match style.wrap {
        Some(cols) => wrap_ranges(text, cols),
        None => std::iter::once(0..text.floor_char_boundary(MAX_UNWRAPPED_BYTES)).collect(),
    };
    let number_color = if line.kind == LineKind::Context {
        style.palette.faint
    } else {
        style.palette.muted
    };
    Prepared {
        last_used: 0,
        chunks: vec![None; ranges.len()],
        pieces: pieces(text.len(), spans, style.palette.dark, words),
        old: line.old.map(|number| shape_number(style, window, number, number_color)),
        new: line.new.map(|number| shape_number(style, window, number, number_color)),
        ranges,
    }
}

fn shape_number(style: &Style, window: &mut Window, number: u32, color: gpui::Hsla) -> ShapedLine {
    let text = number.to_string();
    let run = TextRun {
        len: text.len(),
        font: style.fonts.mono.clone(),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    window
        .text_system()
        .shape_line(text.into(), px(NUMBER_SIZE), &[run], None)
}

fn shape_chunk(style: &Style, window: &mut Window, line: &DiffLine, range: &Range<usize>, all: &[Piece]) -> ShapedLine {
    let (display, chunk) = chunk_pieces(&line.text, range.clone(), all);
    if display.is_empty() {
        return ShapedLine::default();
    }
    let word_background = match line.kind {
        LineKind::Added => Some(style.palette.add_word),
        LineKind::Deleted => Some(style.palette.del_word),
        LineKind::Context => None,
    };
    let runs: Vec<TextRun> = chunk
        .iter()
        .map(|piece| TextRun {
            len: piece.len,
            font: style.fonts.mono.clone(),
            color: piece.color.map_or(style.palette.text, hex),
            background_color: if piece.word { word_background } else { None },
            underline: None,
            strikethrough: None,
        })
        .collect();
    window
        .text_system()
        .shape_line(display.into(), px(CODE_SIZE), &runs, None)
}

const THUMB_WIDTH: f32 = 6.0;
const THUMB_INSET: f32 = 3.0;

pub fn paint(frame: &Frame, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
    let palette = &frame.palette;
    let line_height = px(LINE);
    window.with_content_mask(Some(ContentMask { bounds }), |window| {
        for line in &frame.lines {
            let top = bounds.origin.y + px(line.y);
            let height = px(line.height);
            let (background, number, ribbon) = match line.kind {
                LineKind::Added => (Some(palette.add_bg), Some(palette.add_number), Some(palette.add_ribbon)),
                LineKind::Deleted => (Some(palette.del_bg), Some(palette.del_number), Some(palette.del_ribbon)),
                LineKind::Context => (None, None, None),
            };
            if let Some(color) = background {
                window.paint_quad(fill(
                    Bounds::new(point(bounds.origin.x, top), size(bounds.size.width, height)),
                    color,
                ));
            }
            if let Some(color) = number {
                let width = px(NUMBER_COLUMN * 2.0);
                window.paint_quad(fill(
                    Bounds::new(point(bounds.origin.x, top), size(width, height)),
                    color,
                ));
            }
            if let Some(color) = ribbon {
                let origin = point(bounds.origin.x + px(NUMBER_COLUMN * 2.0), top);
                window.paint_quad(fill(Bounds::new(origin, size(px(RIBBON), height)), color));
            }
            for (column, shaped) in [(0.0, &line.old), (1.0, &line.new)] {
                if let Some(shaped) = shaped {
                    let right = bounds.origin.x + px(NUMBER_COLUMN * (column + 1.0) - NUMBER_PADDING);
                    shaped
                        .paint(point(right - shaped.width, top), line_height, window, cx)
                        .ok();
                }
            }
            let code = bounds.origin.x + px(NUMBER_COLUMN * 2.0 + RIBBON);
            let clip = Bounds::new(
                point(code, bounds.origin.y),
                size(bounds.size.width - px(NUMBER_COLUMN * 2.0 + RIBBON), bounds.size.height),
            );
            window.with_content_mask(Some(ContentMask { bounds: clip }), |window| {
                let left = code + px(CODE_PADDING - frame.scroll_x);
                for (index, shaped) in &line.chunks {
                    shaped
                        .paint(point(left, top + px(*index as f32 * LINE)), line_height, window, cx)
                        .ok();
                }
            });
        }
        if let Some(thumb) = &frame.thumb {
            let origin = point(
                bounds.right() - px(THUMB_WIDTH + THUMB_INSET),
                bounds.origin.y + px(thumb.top),
            );
            let color = palette.muted.opacity(thumb.opacity);
            window.paint_quad(
                fill(Bounds::new(origin, size(px(THUMB_WIDTH), px(thumb.height))), color)
                    .corner_radii(px(THUMB_WIDTH / 2.0)),
            );
        }
    });
}
