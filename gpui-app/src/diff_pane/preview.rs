use crate::diff_pane::markdown::{self, Block, Rich, Style};
use diffpilot_core::changes::FileStatus;
use diffpilot_core::comparison::Comparison;
use diffpilot_core::diff::FileDiff;
use diffpilot_core::preview::{PreviewKind, preview_kind, read_side};
use diffpilot_core::threads::Side;
use diffpilot_ui_kit::Theme;
use diffpilot_ui_kit::components::{label_switch, tag};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, AvailableSpace, Context, Div, FontStyle, FontWeight, Hsla, Image, ImageFormat, IntoElement,
    ObjectFit, ParentElement as _, Render, Size, StrikethroughStyle, Styled as _, StyledImage as _, StyledText,
    TextRun, UnderlineStyle, Window, div, font, img, px,
};
use gpui::{
    AppContext as _, Bounds, CursorStyle, DragMoveEvent, EventEmitter, InteractiveElement as _, MouseButton, Pixels,
    Point, StatefulInteractiveElement as _, canvas,
};
use gpui_component::ActiveTheme;
use std::cell::Cell;
use std::hash::{DefaultHasher, Hash as _, Hasher as _};
use std::io::Cursor;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

pub const PAD_X: f32 = 16.0;
pub const PAD_Y: f32 = 14.0;
pub const GAP: f32 = 16.0;
pub const MAX_IMAGE_HEIGHT: f32 = 320.0;
const IMAGE_PAD: f32 = 8.0;
const FALLBACK_BOX: f32 = 120.0;
const LABEL: f32 = 22.0;
const META: f32 = 24.0;
const TEXT: f32 = diffpilot_ui_kit::text_size::BODY;
const LINE: f32 = 20.0;
const CODE: f32 = 12.0;
const CODE_LINE: f32 = 18.0;
const BLOCK_GAP: f32 = 10.0;
const MARK_INSET: f32 = 12.0;
const MODE_GAP: f32 = 12.0;
const KNOB: f32 = 14.0;
const ONION_TRACK: f32 = 200.0;
const SLIDER_ROW: f32 = 34.0;
const CAPTION_MIN: f32 = 380.0;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    Same,
    Removed,
    Added,
}

pub enum Body {
    Missing,
    Image(ImageBody),
    Markdown(Vec<(Change, Block)>),
}

pub struct LoadedSide {
    pub label: Option<&'static str>,
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
        label: Some(label),
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
    Some(Loaded {
        sides: merge_markdown(sides),
    })
}

fn merge_markdown(sides: Vec<LoadedSide>) -> Vec<LoadedSide> {
    match <[LoadedSide; 2]>::try_from(sides) {
        Ok(
            [
                LoadedSide {
                    body: Body::Markdown(old),
                    ..
                },
                LoadedSide {
                    body: Body::Markdown(new),
                    ..
                },
            ],
        ) => vec![LoadedSide {
            label: None,
            tone: Tone::Neutral,
            body: Body::Markdown(block_changes(old, new)),
        }],
        Ok(pair) => pair.into(),
        Err(sides) => sides,
    }
}

fn block_changes(old: Vec<(Change, Block)>, new: Vec<(Change, Block)>) -> Vec<(Change, Block)> {
    let old: Vec<Block> = old.into_iter().map(|(_, block)| block).collect();
    let new: Vec<Block> = new.into_iter().map(|(_, block)| block).collect();
    let hashes = |blocks: &[Block]| -> Vec<u64> {
        blocks
            .iter()
            .map(|block| {
                let mut hasher = DefaultHasher::new();
                block.hash(&mut hasher);
                hasher.finish()
            })
            .collect()
    };
    let mut out = Vec::with_capacity(new.len());
    for op in similar::capture_diff_slices(similar::Algorithm::Myers, &hashes(&old), &hashes(&new)) {
        let (tag, removed, added) = op.as_tag_tuple();
        if tag == similar::DiffTag::Equal {
            out.extend(new[added].iter().map(|block| (Change::Same, block.clone())));
            continue;
        }
        out.extend(old[removed].iter().map(|block| (Change::Removed, block.clone())));
        out.extend(new[added].iter().map(|block| (Change::Added, block.clone())));
    }
    out
}

fn body(kind: PreviewKind, bytes: Vec<u8>, path: &str) -> Body {
    match kind {
        PreviewKind::Markdown => Body::Markdown(
            markdown::parse(&String::from_utf8_lossy(&bytes))
                .into_iter()
                .map(|block| (Change::Same, block))
                .collect(),
        ),
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

fn column_width(width: f32, sides: usize) -> f32 {
    let sides = sides.max(1) as f32;
    ((width - 2.0 * PAD_X - GAP * (sides - 1.0)) / sides).max(40.0)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImageMode {
    #[default]
    TwoUp,
    Swipe,
    Onion,
}

impl ImageMode {
    const ALL: [ImageMode; 3] = [ImageMode::TwoUp, ImageMode::Swipe, ImageMode::Onion];
    const LABELS: [&'static str; 3] = ["2-up", "Swipe", "Onion Skin"];

    fn index(self) -> usize {
        Self::ALL.iter().position(|mode| *mode == self).unwrap_or(0)
    }
}

pub enum PreviewEvent {
    Resized,
}

type OnMode = Rc<dyn Fn(usize, &mut Window, &mut App)>;
type OnPick = Rc<dyn Fn(Point<Pixels>, &mut Window, &mut App)>;

struct SliderDrag;

impl Render for SliderDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }
}

struct Controls {
    path: String,
    mode: ImageMode,
    position: f32,
    track: Rc<Cell<Option<Bounds<Pixels>>>>,
    on_mode: OnMode,
    on_pick: OnPick,
}

impl Controls {
    fn inert(mode: ImageMode) -> Self {
        Self {
            path: String::new(),
            mode,
            position: 0.5,
            track: Rc::default(),
            on_mode: Rc::new(|_, _, _| {}),
            on_pick: Rc::new(|_, _, _| {}),
        }
    }
}

pub struct PreviewCard {
    path: String,
    loaded: Arc<Loaded>,
    width: f32,
    mode: ImageMode,
    position: f32,
    track: Rc<Cell<Option<Bounds<Pixels>>>>,
}

impl EventEmitter<PreviewEvent> for PreviewCard {}

impl PreviewCard {
    pub fn new(path: String, loaded: Arc<Loaded>, width: f32) -> Self {
        Self {
            path,
            loaded,
            width,
            mode: ImageMode::default(),
            position: 0.5,
            track: Rc::default(),
        }
    }

    pub fn loaded(&self) -> Arc<Loaded> {
        Arc::clone(&self.loaded)
    }

    pub fn mode(&self) -> ImageMode {
        self.mode
    }

    pub fn set_width(&mut self, width: f32) {
        self.width = width;
    }

    fn set_mode(&mut self, mode: ImageMode, cx: &mut Context<Self>) {
        if mode != self.mode {
            self.mode = mode;
            self.position = 0.5;
            cx.emit(PreviewEvent::Resized);
            cx.notify();
        }
    }

    fn pick(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(track) = self.track.get() else { return };
        let width = f32::from(track.size.width).max(1.0);
        let position = (f32::from(at.x - track.origin.x) / width).clamp(0.0, 1.0);
        if position != self.position {
            self.position = position;
            cx.notify();
        }
    }
}

impl Render for PreviewCard {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity();
        let picker = this.clone();
        let controls = Controls {
            path: self.path.clone(),
            mode: self.mode,
            position: self.position,
            track: self.track.clone(),
            on_mode: Rc::new(move |index, _, cx| this.update(cx, |card, cx| card.set_mode(ImageMode::ALL[index], cx))),
            on_pick: Rc::new(move |at, _, cx| picker.update(cx, |card, cx| card.pick(at, cx))),
        };
        card(&self.loaded, &Theme::of(cx), self.width, &controls, cx)
    }
}

pub fn height(loaded: &Loaded, width: f32, mode: ImageMode, window: &mut Window, cx: &mut App) -> f32 {
    let theme = Theme::of(cx);
    let space = Size {
        width: AvailableSpace::Definite(px(width)),
        height: AvailableSpace::MinContent,
    };
    let size = card(loaded, &theme, width, &Controls::inert(mode), cx)
        .into_any_element()
        .layout_as_root(space, window, cx);
    f32::from(size.height)
}

fn usable(side: &LoadedSide) -> Option<&ImageBody> {
    match &side.body {
        Body::Image(image) if !image.broken && image.size.is_some() => Some(image),
        _ => None,
    }
}

fn pair(loaded: &Loaded) -> Option<(&ImageBody, &ImageBody)> {
    match loaded.sides.as_slice() {
        [old, new] => Some((usable(old)?, usable(new)?)),
        _ => None,
    }
}

fn card(loaded: &Loaded, theme: &Theme, width: f32, controls: &Controls, cx: &App) -> Div {
    let frame = div().w(px(width)).px(px(PAD_X)).py(px(PAD_Y));
    let Some((old, new)) = pair(loaded) else {
        return two_up(frame, loaded, theme, width, cx);
    };
    let path = controls.path.clone();
    let switch = div().flex().justify_center().pb(px(MODE_GAP)).child(label_switch(
        theme,
        "image-mode",
        &ImageMode::LABELS,
        controls.mode.index(),
        move |index| format!("pane:image-mode:{path}:{}", ImageMode::LABELS[index]),
        controls.on_mode.clone(),
    ));
    let frame = frame.flex().flex_col().child(switch);
    match controls.mode {
        ImageMode::TwoUp => two_up(frame, loaded, theme, width, cx),
        ImageMode::Swipe => frame.child(stacked(old, new, theme, width, controls, true)),
        ImageMode::Onion => frame.child(stacked(old, new, theme, width, controls, false)),
    }
}

fn two_up(frame: Div, loaded: &Loaded, theme: &Theme, width: f32, cx: &App) -> Div {
    let column = column_width(width, loaded.sides.len());
    frame.child(
        div()
            .flex()
            .flex_row()
            .items_start()
            .gap(px(GAP))
            .children(loaded.sides.iter().map(|side| side_view(side, theme, column, cx))),
    )
}

fn stacked(old: &ImageBody, new: &ImageBody, theme: &Theme, width: f32, controls: &Controls, swipe: bool) -> Div {
    let inset = 2.0 * (IMAGE_PAD + 1.0);
    let room = width - 2.0 * PAD_X - inset;
    let fitted = |image: &ImageBody| fit(image.size.unwrap_or((1, 1)), room, MAX_IMAGE_HEIGHT);
    let (old_size, new_size) = (fitted(old), fitted(new));
    let stage_w = old_size.0.max(new_size.0) + inset;
    let stage_h = old_size.1.max(new_size.1) + inset;
    let layer = |image: &ImageBody, (w, h): (f32, f32)| {
        div()
            .absolute()
            .top_0()
            .left_0()
            .w(px(stage_w))
            .h(px(stage_h))
            .flex()
            .items_center()
            .justify_center()
            .child(
                img(image.image.clone())
                    .w(px(w))
                    .h(px(h))
                    .object_fit(ObjectFit::Contain),
            )
    };
    let position = controls.position;
    let track = controls.track.clone();
    let measure = canvas(move |bounds, _, _| track.set(Some(bounds)), |_, _, _, _| {})
        .absolute()
        .size_full();
    let on_down = controls.on_pick.clone();
    let on_move = controls.on_pick.clone();
    let draggable = |element: Div, id: &'static str| {
        element
            .id(id)
            .cursor(CursorStyle::ResizeLeftRight)
            .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                on_down(event.position, window, cx)
            })
            .on_drag(SliderDrag, |_, _, _, cx| cx.new(|_| SliderDrag))
            .on_drag_move(move |event: &DragMoveEvent<SliderDrag>, window, cx| {
                on_move(event.event.position, window, cx)
            })
            .into_any_element()
    };
    let stage = tag(div(), || format!("pane:image-stage:{}", controls.path))
        .relative()
        .flex_none()
        .w(px(stage_w))
        .h(px(stage_h))
        .overflow_hidden()
        .border_1()
        .border_color(theme.diff.line)
        .rounded(px(6.0))
        .bg(theme.diff.cap)
        .child(layer(old, old_size));
    let (stage, slider) = if swipe {
        let split = position * stage_w;
        let stage = draggable(
            stage
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left(px(split))
                        .w(px(stage_w - split))
                        .h(px(stage_h))
                        .overflow_hidden()
                        .child(layer(new, new_size).left(px(-split))),
                )
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left(px(split - 1.0))
                        .w(px(2.0))
                        .h_full()
                        .bg(theme.accent),
                )
                .child(
                    tag(div(), || format!("pane:swipe-knob:{}", controls.path))
                        .absolute()
                        .top(px(stage_h / 2.0 - KNOB / 2.0))
                        .left(px(split - KNOB / 2.0))
                        .size(px(KNOB))
                        .rounded_full()
                        .bg(theme.accent),
                )
                .child(measure),
            "image-swipe",
        );
        (stage, None)
    } else {
        let stage = stage.child(layer(new, new_size).opacity(position)).into_any_element();
        let track = draggable(
            tag(div(), || format!("pane:onion-track:{}", controls.path))
                .relative()
                .w(px(ONION_TRACK))
                .h(px(KNOB))
                .flex()
                .items_center()
                .child(div().w_full().h(px(4.0)).rounded_full().bg(theme.diff.line))
                .child(
                    tag(div(), || format!("pane:onion-knob:{}", controls.path))
                        .absolute()
                        .top_0()
                        .left(px(position * ONION_TRACK - KNOB / 2.0))
                        .size(px(KNOB))
                        .rounded_full()
                        .bg(theme.accent),
                )
                .child(measure),
            "onion-track",
        );
        let slider = Some(
            div()
                .flex()
                .items_center()
                .justify_center()
                .gap(px(10.0))
                .h(px(SLIDER_ROW))
                .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SECONDARY))
                .text_color(theme.diff.muted)
                .child("Before")
                .child(track)
                .child("After"),
        );
        (stage, slider)
    };
    let caption = |label: &'static str, image: &ImageBody| {
        div()
            .flex()
            .gap(px(6.0))
            .child(div().font_weight(FontWeight::SEMIBOLD).child(label))
            .child(image_meta_text(image))
    };
    div()
        .flex()
        .flex_col()
        .items_center()
        .child(stage)
        .children(slider)
        .child(
            div()
                .flex()
                .justify_between()
                .gap(px(GAP))
                .w(px(stage_w.max(CAPTION_MIN)))
                .h(px(META))
                .pt(px(6.0))
                .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SECONDARY))
                .text_color(theme.diff.muted)
                .child(caption("Before", old))
                .child(caption("After", new)),
        )
}

fn side_view(side: &LoadedSide, theme: &Theme, column: f32, cx: &App) -> Div {
    let tint = match side.tone {
        Tone::Neutral => theme.diff.muted,
        Tone::Added => theme.success,
        Tone::Deleted => theme.danger,
    };
    let label = side.label.map(|label| {
        div()
            .h(px(LABEL))
            .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SECONDARY))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(tint)
            .child(label)
    });
    let shell = div().flex().flex_col().flex_none().w(px(column)).children(label);
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
        .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SECONDARY))
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
            .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SECONDARY))
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

fn image_meta_text(image: &ImageBody) -> String {
    match (image.broken, image.size) {
        (false, Some((width, height))) => format!("{width} \u{d7} {height} \u{b7} {}", file_size(image.bytes)),
        _ => file_size(image.bytes),
    }
}

fn image_meta(image: &ImageBody, theme: &Theme) -> Div {
    let text = image_meta_text(image);
    div()
        .h(px(META))
        .pt(px(6.0))
        .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SECONDARY))
        .text_color(theme.diff.muted)
        .child(text)
}

struct Fonts {
    base: gpui::Font,
    mono: gpui::Font,
}

fn document(blocks: &[(Change, Block)], theme: &Theme, cx: &App) -> Div {
    let fonts = Fonts {
        base: font(cx.theme().font_family.clone()),
        mono: font(cx.theme().mono_font_family.clone()),
    };
    let marked = blocks.iter().any(|(change, _)| *change != Change::Same);
    div()
        .flex()
        .flex_col()
        .gap(px(BLOCK_GAP))
        .min_w_0()
        .children(blocks.iter().map(|(change, block)| {
            let view = block_view(block, theme, &fonts);
            let tint = match change {
                Change::Same if !marked => return view,
                Change::Same => return div().pl(px(MARK_INSET)).child(view).into_any_element(),
                Change::Removed => theme.danger,
                Change::Added => theme.success,
            };
            div()
                .pl(px(MARK_INSET - 2.0))
                .py(px(4.0))
                .border_l_2()
                .border_color(tint)
                .bg(tint.opacity(0.08))
                .child(view)
                .into_any_element()
        }))
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
                            .text_size(diffpilot_ui_kit::text_px(TEXT))
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
                    .child(rich(
                        cell,
                        theme,
                        fonts,
                        diffpilot_ui_kit::text_size::SECONDARY,
                        18.0,
                        theme.diff.text,
                        weight,
                    ))
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
    let frame = div()
        .w_full()
        .min_w_0()
        .text_size(diffpilot_ui_kit::text_px(size))
        .line_height(px(line));
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
    fn a_changed_markdown_file_becomes_one_document_with_marked_blocks() {
        let side = |text: &str| LoadedSide {
            label: Some("Side"),
            tone: Tone::Neutral,
            body: body(PreviewKind::Markdown, text.as_bytes().to_vec(), "notes.md"),
        };
        let merged = merge_markdown(vec![
            side("# Title\n\nold line\n\nkept"),
            side("# Title\n\nnew line\n\nkept"),
        ]);
        let [only] = merged.as_slice() else {
            panic!("expected one document, got {}", merged.len())
        };
        assert_eq!(only.label, None);
        let Body::Markdown(blocks) = &only.body else {
            panic!("expected markdown")
        };
        let text = |block: &Block| match block {
            Block::Heading { text, .. } | Block::Paragraph(text) => text.text.clone(),
            other => panic!("unexpected block {other:?}"),
        };
        let changes: Vec<(Change, String)> = blocks.iter().map(|(change, block)| (*change, text(block))).collect();
        assert_eq!(
            changes,
            vec![
                (Change::Same, "Title".into()),
                (Change::Removed, "old line".into()),
                (Change::Added, "new line".into()),
                (Change::Same, "kept".into()),
            ]
        );
    }

    #[test]
    fn one_side_fills_the_card_and_two_sides_share_it() {
        assert_eq!(column_width(1000.0, 1), 1000.0 - 2.0 * PAD_X);
        assert_eq!(column_width(1000.0, 2), (1000.0 - 2.0 * PAD_X - GAP) / 2.0);
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
