use crate::diff_pane::markdown::{self, Block, Rich, Style};
use diffpilot_core::changes::FileStatus;
use diffpilot_core::comparison::Comparison;
use diffpilot_core::diff::FileDiff;
use diffpilot_core::preview::{PreviewKind, preview_kind, read_side};
use diffpilot_core::threads::Side;
use diffpilot_ui_kit::Theme;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, AvailableSpace, Context, Div, FontStyle, FontWeight, Hsla, Image, ImageFormat, IntoElement,
    ObjectFit, ParentElement as _, Render, Size, StrikethroughStyle, Styled as _, StyledImage as _, StyledText,
    TextRun, UnderlineStyle, Window, div, font, img, px,
};
use gpui_component::ActiveTheme;
use std::io::Cursor;
use std::path::Path;
use std::sync::Arc;

pub const PAD_X: f32 = 16.0;
pub const PAD_Y: f32 = 14.0;
pub const GAP: f32 = 16.0;
pub const MAX_IMAGE_HEIGHT: f32 = 320.0;
const IMAGE_PAD: f32 = 8.0;
const FALLBACK_BOX: f32 = 120.0;
const LABEL: f32 = 22.0;
const META: f32 = 24.0;
const TEXT: f32 = 13.0;
const LINE: f32 = 20.0;
const CODE: f32 = 12.0;
const CODE_LINE: f32 = 18.0;
const BLOCK_GAP: f32 = 10.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Neutral,
    Added,
    Deleted,
}

pub struct ImageBody {
    pub image: Arc<Image>,
    pub size: Option<(u32, u32)>,
    pub bytes: usize,
    pub broken: bool,
}

pub enum Body {
    Missing,
    Image(ImageBody),
    Markdown(Vec<Block>),
}

pub struct LoadedSide {
    pub label: &'static str,
    pub tone: Tone,
    pub body: Body,
}

pub struct Loaded {
    pub sides: Vec<LoadedSide>,
}

pub fn load(repo: &Path, comparison: &Comparison, file: &FileDiff) -> Option<Loaded> {
    let kind = preview_kind(&file.path)?;
    let old_path = file.old_path.as_deref().unwrap_or(&file.path);
    let side = |label, tone, side, path: &str| LoadedSide {
        label,
        tone,
        body: match read_side(repo, comparison, side, path) {
            Some(bytes) => body(kind, bytes, path),
            None => Body::Missing,
        },
    };
    let sides = match file.status {
        FileStatus::Added => vec![side("Added", Tone::Added, Side::New, &file.path)],
        FileStatus::Deleted => vec![side("Deleted", Tone::Deleted, Side::Old, old_path)],
        _ => vec![
            side("Before", Tone::Neutral, Side::Old, old_path),
            side("After", Tone::Neutral, Side::New, &file.path),
        ],
    };
    Some(Loaded { sides })
}

fn body(kind: PreviewKind, bytes: Vec<u8>, path: &str) -> Body {
    match kind {
        PreviewKind::Markdown => Body::Markdown(markdown::parse(&String::from_utf8_lossy(&bytes))),
        PreviewKind::Image => Body::Image(image_body(bytes, path)),
    }
}

fn image_body(bytes: Vec<u8>, path: &str) -> ImageBody {
    let total = bytes.len();
    let extension = path
        .rsplit_once('.')
        .map_or(String::new(), |(_, ext)| ext.to_ascii_lowercase());
    let (format, bytes, size) = match extension.as_str() {
        "svg" => {
            let size = svg_size(&bytes);
            (ImageFormat::Svg, bytes, size)
        }
        "ico" => match ico_as_png(&bytes) {
            Some((png, size)) => (ImageFormat::Png, png, Some(size)),
            None => (ImageFormat::Png, bytes, None),
        },
        other => {
            let (gpui_format, decoder) = match other {
                "png" => (ImageFormat::Png, image::ImageFormat::Png),
                "jpg" | "jpeg" => (ImageFormat::Jpeg, image::ImageFormat::Jpeg),
                "gif" => (ImageFormat::Gif, image::ImageFormat::Gif),
                "webp" => (ImageFormat::Webp, image::ImageFormat::WebP),
                _ => (ImageFormat::Bmp, image::ImageFormat::Bmp),
            };
            let size = image::ImageReader::with_format(Cursor::new(&bytes), decoder)
                .into_dimensions()
                .ok();
            (gpui_format, bytes, size)
        }
    };
    ImageBody {
        broken: size.is_none() && format != ImageFormat::Svg,
        image: Arc::new(Image::from_bytes(format, bytes)),
        size,
        bytes: total,
    }
}

fn ico_as_png(bytes: &[u8]) -> Option<(Vec<u8>, (u32, u32))> {
    let decoded = image::load_from_memory_with_format(bytes, image::ImageFormat::Ico).ok()?;
    let size = (decoded.width(), decoded.height());
    let mut png = Vec::new();
    decoded
        .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
        .ok()?;
    Some((png, size))
}

fn svg_size(bytes: &[u8]) -> Option<(u32, u32)> {
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(4096)]).into_owned();
    let attribute = |name: &str| {
        let key = format!("{name}=\"");
        let start = head.find(&key)? + key.len();
        let end = head[start..].find('"')?;
        Some(head[start..start + end].to_string())
    };
    let number = |text: &str| text.trim().trim_end_matches("px").parse::<f32>().ok();
    if let Some(view_box) = attribute("viewBox") {
        let parts: Vec<f32> = view_box
            .split(|c: char| c.is_whitespace() || c == ',')
            .filter_map(number)
            .collect();
        if let [_, _, width, height] = parts[..]
            && width > 0.0
            && height > 0.0
        {
            return Some((width.round() as u32, height.round() as u32));
        }
    }
    let width = attribute("width").and_then(|text| number(&text))?;
    let height = attribute("height").and_then(|text| number(&text))?;
    (width > 0.0 && height > 0.0).then(|| (width.round() as u32, height.round() as u32))
}

pub fn fit(size: (u32, u32), max_width: f32, max_height: f32) -> (f32, f32) {
    let (width, height) = (size.0.max(1) as f32, size.1.max(1) as f32);
    let scale = (max_width / width).min(max_height / height).min(1.0);
    ((width * scale).round().max(1.0), (height * scale).round().max(1.0))
}

fn file_size(bytes: usize) -> String {
    match bytes {
        0..1024 => format!("{bytes} B"),
        1024..1_048_576 => format!("{:.1} KB", bytes as f32 / 1024.0),
        _ => format!("{:.1} MB", bytes as f32 / 1_048_576.0),
    }
}

pub fn column_width(width: f32) -> f32 {
    ((width - 2.0 * PAD_X - GAP) / 2.0).max(40.0)
}

pub struct PreviewCard {
    loaded: Arc<Loaded>,
    width: f32,
}

impl PreviewCard {
    pub fn new(loaded: Arc<Loaded>, width: f32) -> Self {
        Self { loaded, width }
    }

    pub fn loaded(&self) -> Arc<Loaded> {
        Arc::clone(&self.loaded)
    }

    pub fn set_width(&mut self, width: f32) {
        self.width = width;
    }
}

impl Render for PreviewCard {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        card(&self.loaded, &Theme::of(cx), self.width, cx)
    }
}

pub fn height(loaded: &Loaded, width: f32, window: &mut Window, cx: &mut App) -> f32 {
    let theme = Theme::of(cx);
    let space = Size {
        width: AvailableSpace::Definite(px(width)),
        height: AvailableSpace::MinContent,
    };
    let size = card(loaded, &theme, width, cx)
        .into_any_element()
        .layout_as_root(space, window, cx);
    f32::from(size.height)
}

fn card(loaded: &Loaded, theme: &Theme, width: f32, cx: &App) -> Div {
    let column = column_width(width);
    div()
        .flex()
        .flex_row()
        .items_start()
        .gap(px(GAP))
        .w(px(width))
        .px(px(PAD_X))
        .py(px(PAD_Y))
        .children(loaded.sides.iter().map(|side| side_view(side, theme, column, cx)))
}

fn side_view(side: &LoadedSide, theme: &Theme, column: f32, cx: &App) -> Div {
    let tint = match side.tone {
        Tone::Neutral => theme.diff.muted,
        Tone::Added => theme.success,
        Tone::Deleted => theme.danger,
    };
    let label = div()
        .h(px(LABEL))
        .text_size(px(12.5))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(tint)
        .child(side.label);
    let shell = div().flex().flex_col().flex_none().w(px(column)).child(label);
    match &side.body {
        Body::Missing => shell.child(note(theme, "Not available.")),
        Body::Image(image) => shell
            .child(image_box(image, theme, column))
            .child(image_meta(image, theme)),
        Body::Markdown(blocks) => shell.child(document(blocks, theme, cx)),
    }
}

fn note(theme: &Theme, text: &'static str) -> Div {
    div()
        .h(px(LINE))
        .text_size(px(12.5))
        .text_color(theme.diff.muted)
        .child(text)
}

fn image_box(image: &ImageBody, theme: &Theme, column: f32) -> Div {
    let frame = div()
        .flex()
        .items_center()
        .justify_center()
        .w(px(column))
        .border_1()
        .border_color(theme.diff.line)
        .rounded(px(6.0))
        .bg(theme.diff.cap);
    let inset = 2.0 * (IMAGE_PAD + 1.0);
    if image.broken {
        return frame
            .h(px(FALLBACK_BOX))
            .text_size(px(12.5))
            .text_color(theme.diff.muted)
            .child("Preview not available.");
    }
    match image.size {
        Some(size) => {
            let (width, height) = fit(size, column - inset, MAX_IMAGE_HEIGHT);
            frame.h(px(height + inset)).child(
                img(image.image.clone())
                    .w(px(width))
                    .h(px(height))
                    .object_fit(ObjectFit::Contain),
            )
        }
        None => frame.h(px(FALLBACK_BOX)).child(
            img(image.image.clone())
                .w(px(column - inset))
                .h(px(FALLBACK_BOX - inset))
                .object_fit(ObjectFit::Contain),
        ),
    }
}

fn image_meta(image: &ImageBody, theme: &Theme) -> Div {
    let text = match (image.broken, image.size) {
        (false, Some((width, height))) => format!("{width} \u{d7} {height} \u{b7} {}", file_size(image.bytes)),
        _ => file_size(image.bytes),
    };
    div()
        .h(px(META))
        .pt(px(6.0))
        .text_size(px(12.0))
        .text_color(theme.diff.muted)
        .child(text)
}

struct Fonts {
    base: gpui::Font,
    mono: gpui::Font,
}

fn document(blocks: &[Block], theme: &Theme, cx: &App) -> Div {
    let fonts = Fonts {
        base: font(cx.theme().font_family.clone()),
        mono: font(cx.theme().mono_font_family.clone()),
    };
    stack(blocks, theme, &fonts)
}

fn stack(blocks: &[Block], theme: &Theme, fonts: &Fonts) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(BLOCK_GAP))
        .min_w_0()
        .children(blocks.iter().map(|block| block_view(block, theme, fonts)))
}

fn block_view(block: &Block, theme: &Theme, fonts: &Fonts) -> AnyElement {
    match block {
        Block::Heading { level, text } => {
            let (size, line) = match level {
                1 => (22.0, 28.0),
                2 => (18.0, 24.0),
                3 => (16.0, 22.0),
                _ => (14.0, 20.0),
            };
            let color = if *level == 6 { theme.diff.muted } else { theme.diff.text };
            rich(text, theme, fonts, size, line, color, FontWeight::SEMIBOLD)
        }
        Block::Paragraph(text) => rich(text, theme, fonts, TEXT, LINE, theme.diff.text, FontWeight::NORMAL),
        Block::Code(code) => div()
            .w_full()
            .px(px(10.0))
            .py(px(8.0))
            .rounded(px(6.0))
            .bg(theme.diff.text.opacity(0.06))
            .overflow_hidden()
            .font(fonts.mono.clone())
            .text_size(px(CODE))
            .text_color(theme.diff.text)
            .children(code.lines().map(|line| {
                div()
                    .h(px(CODE_LINE))
                    .whitespace_nowrap()
                    .child(gpui::SharedString::from(line.to_string()))
            }))
            .into_any_element(),
        Block::Quote(blocks) => stack(blocks, theme, fonts)
            .pl(px(12.0))
            .border_l_2()
            .border_color(theme.diff.line)
            .into_any_element(),
        Block::List { start, items } => div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .children(items.iter().enumerate().map(|(index, item)| {
                let marker = match start {
                    Some(start) => format!("{}.", start + index as u64),
                    None => "\u{2022}".to_string(),
                };
                div()
                    .flex()
                    .flex_row()
                    .items_start()
                    .gap(px(6.0))
                    .child(
                        div()
                            .flex_none()
                            .w(px(22.0))
                            .h(px(LINE))
                            .text_size(px(TEXT))
                            .text_color(theme.diff.muted)
                            .flex()
                            .justify_end()
                            .child(gpui::SharedString::from(marker)),
                    )
                    .child(stack(item, theme, fonts).flex_1().gap(px(4.0)))
            }))
            .into_any_element(),
        Block::Rule => div().h(px(1.0)).w_full().bg(theme.diff.line).into_any_element(),
        Block::Table { head, rows } => table(head, rows, theme, fonts),
    }
}

fn table(head: &[Rich], rows: &[Vec<Rich>], theme: &Theme, fonts: &Fonts) -> AnyElement {
    let cells = |row: &[Rich], bold: bool| {
        let weight = if bold { FontWeight::SEMIBOLD } else { FontWeight::NORMAL };
        div()
            .flex()
            .flex_row()
            .border_b_1()
            .border_color(theme.diff.line)
            .when(bold, |this| this.bg(theme.diff.cap))
            .children(row.iter().map(|cell| {
                div()
                    .flex_1()
                    .min_w_0()
                    .px(px(8.0))
                    .py(px(4.0))
                    .border_r_1()
                    .border_color(theme.diff.line)
                    .child(rich(cell, theme, fonts, 12.5, 18.0, theme.diff.text, weight))
            }))
    };
    div()
        .flex()
        .flex_col()
        .w_full()
        .border_t_1()
        .border_l_1()
        .border_color(theme.diff.line)
        .child(cells(head, true))
        .children(rows.iter().map(|row| cells(row, false)))
        .into_any_element()
}

fn rich(
    text: &Rich,
    theme: &Theme,
    fonts: &Fonts,
    size: f32,
    line: f32,
    color: Hsla,
    weight: FontWeight,
) -> AnyElement {
    let frame = div().w_full().min_w_0().text_size(px(size)).line_height(px(line));
    if text.text.is_empty() {
        return frame.h(px(line)).into_any_element();
    }
    let runs = text
        .spans
        .iter()
        .map(|(range, style)| run(range.len(), *style, theme, fonts, color, weight))
        .collect();
    frame
        .child(StyledText::new(text.text.clone()).with_runs(runs))
        .into_any_element()
}

fn run(len: usize, style: Style, theme: &Theme, fonts: &Fonts, color: Hsla, weight: FontWeight) -> TextRun {
    let mut font = if style.code {
        fonts.mono.clone()
    } else {
        fonts.base.clone()
    };
    font.weight = if style.bold { FontWeight::BOLD } else { weight };
    if style.italic {
        font.style = FontStyle::Italic;
    }
    let color = if style.link { theme.accent } else { color };
    TextRun {
        len,
        font,
        color,
        background_color: style.code.then(|| color.opacity(0.1)),
        underline: style.link.then(|| UnderlineStyle {
            thickness: px(1.0),
            color: Some(color),
            wavy: false,
        }),
        strikethrough: style.strike.then(|| StrikethroughStyle {
            thickness: px(1.0),
            color: Some(color),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_images_keep_their_size_and_large_ones_fit_the_box() {
        assert_eq!(fit((40, 30), 400.0, 320.0), (40.0, 30.0));
        assert_eq!(fit((800, 400), 400.0, 320.0), (400.0, 200.0));
        assert_eq!(fit((100, 1000), 400.0, 320.0), (32.0, 320.0));
    }

    #[test]
    fn formats_file_sizes() {
        assert_eq!(file_size(12), "12 B");
        assert_eq!(file_size(2048), "2.0 KB");
        assert_eq!(file_size(3 * 1_048_576), "3.0 MB");
    }

    #[test]
    fn reads_svg_sizes_from_the_view_box_or_the_attributes() {
        assert_eq!(
            svg_size(br#"<svg viewBox="0 0 24 16" stroke-width="2">"#),
            Some((24, 16))
        );
        assert_eq!(svg_size(br#"<svg width="32px" height="8">"#), Some((32, 8)));
        assert_eq!(svg_size(b"<svg>"), None);
    }
}
