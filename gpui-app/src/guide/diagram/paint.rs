use super::focus::{Focus, NodeState};
use diffpilot_core::guide::layout::{
    BlockGeom, BoxGeom, EdgeGeom, FlowLayout, GroupGeom, Layout, Measure, MessageGeom, ParticipantGeom, Point, Rect,
    SequenceLayout,
};
use diffpilot_core::guide::mermaid::{Arrow, BlockKind, LineStyle, ParticipantKind, Shape, Tip};
use diffpilot_ui_kit::Theme;
use gpui::{
    App, BorderStyle, Bounds, Font, FontWeight, Hsla, PathBuilder, Pixels, SharedString, TextRun, Window,
    WindowTextSystem, fill, font, point, px, size,
};
use std::sync::Arc;

const LINE_HEIGHT: f32 = 1.4;

pub struct TextMeasure {
    system: Arc<WindowTextSystem>,
    font: Font,
    size: f32,
}

impl TextMeasure {
    pub fn new(window: &Window, family: SharedString, size: f32) -> Self {
        Self {
            system: window.text_system().clone(),
            font: font(family),
            size,
        }
    }
}

impl Measure for TextMeasure {
    fn text_width(&self, text: &str) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        let run = TextRun {
            len: text.len(),
            font: self.font.clone(),
            color: gpui::black(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let line = self
            .system
            .shape_line(text.to_string().into(), px(self.size), &[run], None);
        f32::from(line.width)
    }

    fn line_height(&self) -> f32 {
        self.size * LINE_HEIGHT
    }
}

#[derive(Clone, Copy)]
pub struct Palette {
    background: Hsla,
    text: Hsla,
    text2: Hsla,
    muted: Hsla,
    line: Hsla,
    line2: Hsla,
    raised: Hsla,
    claude: Hsla,
    claude_bg: Hsla,
    added: Hsla,
    added_bg: Hsla,
    changed: Hsla,
    removed: Hsla,
    removed_bg: Hsla,
}

impl Palette {
    pub fn of(theme: &Theme) -> Self {
        Self {
            background: theme.background,
            text: theme.ui.text,
            text2: theme.ui.text2,
            muted: theme.ui.muted,
            line: theme.ui.line,
            line2: theme.ui.line2,
            raised: theme.ui.raised,
            claude: theme.ui.claude,
            claude_bg: theme.ui.claude_bg,
            added: theme.diff.add_number,
            added_bg: theme.diff.add_bg,
            changed: theme.amber,
            removed: theme.danger,
            removed_bg: theme.diff.del_bg,
        }
    }

    pub fn class_color(&self, class: &str) -> Option<Hsla> {
        match class {
            "added" => Some(self.added),
            "changed" => Some(self.changed),
            "removed" => Some(self.removed),
            _ => None,
        }
    }
}

struct Tone {
    stroke: Hsla,
    fill: Hsla,
    text: Hsla,
    width: f32,
    dashed: bool,
}

fn tone(palette: &Palette, class: Option<&str>, state: NodeState) -> Tone {
    let mut tone = match class {
        Some("added") => Tone {
            stroke: palette.added,
            fill: palette.added_bg,
            text: palette.text,
            width: 1.3,
            dashed: false,
        },
        Some("changed") => Tone {
            stroke: palette.changed,
            fill: palette.changed.opacity(0.14),
            text: palette.text,
            width: 1.3,
            dashed: false,
        },
        Some("removed") => Tone {
            stroke: palette.removed,
            fill: palette.removed_bg,
            text: palette.text2,
            width: 1.3,
            dashed: true,
        },
        _ => Tone {
            stroke: palette.line2,
            fill: palette.raised,
            text: palette.text,
            width: 1.2,
            dashed: false,
        },
    };
    match state {
        NodeState::Normal => {}
        NodeState::Active => {
            tone.stroke = palette.claude;
            tone.fill = palette.claude_bg;
            tone.width = 2.2;
        }
        NodeState::Dim => {
            tone.stroke = tone.stroke.opacity(0.35);
            tone.fill = tone.fill.opacity(0.35);
            tone.text = tone.text.opacity(0.4);
        }
    }
    tone
}

struct Pen<'a> {
    window: &'a mut Window,
    cx: &'a mut App,
    origin: gpui::Point<Pixels>,
    scale: f32,
    font: Font,
    font_size: f32,
}

impl Pen<'_> {
    fn at(&self, p: Point) -> gpui::Point<Pixels> {
        point(
            self.origin.x + px(p.x * self.scale),
            self.origin.y + px(p.y * self.scale),
        )
    }

    fn bounds(&self, r: Rect) -> Bounds<Pixels> {
        Bounds::new(
            self.at(Point::new(r.x, r.y)),
            size(px(r.w * self.scale), px(r.h * self.scale)),
        )
    }

    fn px(&self, value: f32) -> Pixels {
        px(value * self.scale)
    }

    fn quad(&mut self, r: Rect, radius: f32, tone: &Tone) {
        let mut quad = fill(self.bounds(r), tone.fill)
            .corner_radii(self.px(radius))
            .border_widths(self.px(tone.width))
            .border_color(tone.stroke);
        if tone.dashed {
            quad.border_style = BorderStyle::Dashed;
        }
        self.window.paint_quad(quad);
    }

    fn stroke(&mut self, points: &[Point], closed: bool, width: f32, color: Hsla, dash: Option<(f32, f32)>) {
        if points.len() < 2 {
            return;
        }
        let points: Vec<_> = points.iter().map(|p| self.at(*p)).collect();
        let mut builder = PathBuilder::stroke(self.px(width));
        if let Some((on, off)) = dash {
            builder = builder.dash_array(&[self.px(on), self.px(off)]);
        }
        builder.add_polygon(&points, closed);
        if let Ok(path) = builder.build() {
            self.window.paint_path(path, color);
        }
    }

    fn polygon(&mut self, points: &[Point], tone: &Tone) {
        let screen: Vec<_> = points.iter().map(|p| self.at(*p)).collect();
        let mut builder = PathBuilder::fill();
        builder.add_polygon(&screen, true);
        if let Ok(path) = builder.build() {
            self.window.paint_path(path, tone.fill);
        }
        self.stroke(points, true, tone.width, tone.stroke, tone.dashed.then_some((4.0, 3.0)));
    }

    fn solid(&mut self, points: &[Point], color: Hsla) {
        let screen: Vec<_> = points.iter().map(|p| self.at(*p)).collect();
        let mut builder = PathBuilder::fill();
        builder.add_polygon(&screen, true);
        if let Ok(path) = builder.build() {
            self.window.paint_path(path, color);
        }
    }

    fn line_height(&self) -> f32 {
        self.font_size * LINE_HEIGHT
    }

    fn shape(&self, text: &str, color: Hsla, bold: bool) -> gpui::ShapedLine {
        let mut font = self.font.clone();
        if bold {
            font.weight = FontWeight::SEMIBOLD;
        }
        let run = TextRun {
            len: text.len(),
            font,
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        self.window
            .text_system()
            .shape_line(text.to_string().into(), px(self.font_size * self.scale), &[run], None)
    }

    fn text_width(&self, text: &str) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        f32::from(self.shape(text, gpui::black(), false).width) / self.scale
    }

    fn text(&mut self, text: &str, left: f32, top: f32, color: Hsla, bold: bool) {
        for (row, line) in text.lines().enumerate() {
            if line.is_empty() {
                continue;
            }
            let shaped = self.shape(line, color, bold);
            let at = self.at(Point::new(left, top + row as f32 * self.line_height()));
            shaped.paint(at, self.px(self.line_height()), self.window, self.cx).ok();
        }
    }

    fn centered(&mut self, text: &str, center: Point, color: Hsla, bold: bool) {
        let lines: Vec<&str> = text.lines().collect();
        let top = center.y - lines.len() as f32 * self.line_height() / 2.0;
        for (row, line) in lines.iter().enumerate() {
            let width = self.text_width(line);
            self.text(
                line,
                center.x - width / 2.0,
                top + row as f32 * self.line_height(),
                color,
                bold,
            );
        }
    }

    fn label(&mut self, text: &str, center: Point, color: Hsla, background: Hsla) {
        let width = self.text_width(text) + 8.0;
        let height = self.line_height() + 2.0;
        let rect = Rect::new(center.x - width / 2.0, center.y - height / 2.0, width, height);
        let quad = fill(self.bounds(rect), background).corner_radii(self.px(4.0));
        self.window.paint_quad(quad);
        self.centered(text, center, color, false);
    }
}

fn direction(from: Point, to: Point) -> (f32, f32) {
    let (dx, dy) = (to.x - from.x, to.y - from.y);
    let length = (dx * dx + dy * dy).sqrt();
    if length < 0.001 {
        (0.0, 1.0)
    } else {
        (dx / length, dy / length)
    }
}

const HEAD_LENGTH: f32 = 9.0;
const HEAD_HALF: f32 = 4.0;

fn tip(pen: &mut Pen, at: Point, toward: Point, kind: Tip, color: Hsla, background: Hsla) {
    let (ux, uy) = direction(toward, at);
    let base = Point::new(at.x - ux * HEAD_LENGTH, at.y - uy * HEAD_LENGTH);
    let (nx, ny) = (-uy, ux);
    match kind {
        Tip::None => {}
        Tip::Arrow => pen.solid(
            &[
                at,
                Point::new(base.x + nx * HEAD_HALF, base.y + ny * HEAD_HALF),
                Point::new(base.x - nx * HEAD_HALF, base.y - ny * HEAD_HALF),
            ],
            color,
        ),
        Tip::Circle => {
            let r = 3.5;
            let center = Point::new(at.x - ux * r, at.y - uy * r);
            let quad = fill(pen.bounds(Rect::centered(center, 2.0 * r, 2.0 * r)), background)
                .corner_radii(pen.px(r))
                .border_widths(pen.px(1.2))
                .border_color(color);
            pen.window.paint_quad(quad);
        }
        Tip::Cross => {
            let c = Point::new(at.x - ux * 4.0, at.y - uy * 4.0);
            let (a, b) = (3.5, 3.5);
            pen.stroke(
                &[Point::new(c.x - a, c.y - b), Point::new(c.x + a, c.y + b)],
                false,
                1.4,
                color,
                None,
            );
            pen.stroke(
                &[Point::new(c.x - a, c.y + b), Point::new(c.x + a, c.y - b)],
                false,
                1.4,
                color,
                None,
            );
        }
    }
}

fn open_tip(pen: &mut Pen, at: Point, toward: Point, color: Hsla) {
    let (ux, uy) = direction(toward, at);
    let base = Point::new(at.x - ux * HEAD_LENGTH, at.y - uy * HEAD_LENGTH);
    let (nx, ny) = (-uy, ux);
    pen.stroke(
        &[
            Point::new(base.x + nx * HEAD_HALF, base.y + ny * HEAD_HALF),
            at,
            Point::new(base.x - nx * HEAD_HALF, base.y - ny * HEAD_HALF),
        ],
        false,
        1.3,
        color,
        None,
    );
}

fn shape_points(shape: Shape, r: Rect) -> Option<Vec<Point>> {
    let (x, y, w, h) = (r.x, r.y, r.w, r.h);
    let slant = (h * 0.3).min(w * 0.25);
    let points = match shape {
        Shape::Diamond => vec![
            Point::new(x + w / 2.0, y),
            Point::new(x + w, y + h / 2.0),
            Point::new(x + w / 2.0, y + h),
            Point::new(x, y + h / 2.0),
        ],
        Shape::Hexagon => vec![
            Point::new(x + slant, y),
            Point::new(x + w - slant, y),
            Point::new(x + w, y + h / 2.0),
            Point::new(x + w - slant, y + h),
            Point::new(x + slant, y + h),
            Point::new(x, y + h / 2.0),
        ],
        Shape::Parallelogram => vec![
            Point::new(x + slant, y),
            Point::new(x + w, y),
            Point::new(x + w - slant, y + h),
            Point::new(x, y + h),
        ],
        Shape::ParallelogramAlt => vec![
            Point::new(x, y),
            Point::new(x + w - slant, y),
            Point::new(x + w, y + h),
            Point::new(x + slant, y + h),
        ],
        Shape::Trapezoid => vec![
            Point::new(x + slant, y),
            Point::new(x + w - slant, y),
            Point::new(x + w, y + h),
            Point::new(x, y + h),
        ],
        Shape::TrapezoidAlt => vec![
            Point::new(x, y),
            Point::new(x + w, y),
            Point::new(x + w - slant, y + h),
            Point::new(x + slant, y + h),
        ],
        Shape::Asymmetric => vec![
            Point::new(x + slant, y),
            Point::new(x + w, y),
            Point::new(x + w, y + h),
            Point::new(x + slant, y + h),
            Point::new(x, y + h / 2.0),
        ],
        _ => return None,
    };
    Some(points)
}

fn paint_box(pen: &mut Pen, palette: &Palette, node: &BoxGeom, state: NodeState) {
    let tone = tone(palette, node.class.as_deref(), state);
    let r = node.rect;
    if let Some(points) = shape_points(node.shape, r) {
        pen.polygon(&points, &tone);
    } else {
        let radius = match node.shape {
            Shape::Stadium | Shape::Circle => r.h.min(r.w) / 2.0,
            Shape::Round | Shape::Cylinder => 10.0,
            _ => 5.0,
        };
        pen.quad(r, radius, &tone);
        if node.shape == Shape::Subroutine {
            for x in [r.x + 6.0, r.right() - 6.0] {
                pen.stroke(
                    &[Point::new(x, r.y), Point::new(x, r.bottom())],
                    false,
                    1.0,
                    tone.stroke,
                    None,
                );
            }
        }
    }
    pen.centered(&node.label, r.center(), tone.text, state == NodeState::Active);
}

fn paint_group(pen: &mut Pen, palette: &Palette, group: &GroupGeom) {
    let tone = Tone {
        stroke: palette.line,
        fill: palette.raised.opacity(0.5),
        text: palette.muted,
        width: 1.0,
        dashed: false,
    };
    pen.quad(group.rect, 8.0, &tone);
    pen.text(
        &group.title,
        group.rect.x + 10.0,
        group.rect.y + 5.0,
        palette.muted,
        true,
    );
}

fn paint_edge(pen: &mut Pen, palette: &Palette, edge: &EdgeGeom, lit: bool, dim: bool) {
    if edge.style == LineStyle::Invisible || edge.points.len() < 2 {
        return;
    }
    let mut color = if lit { palette.claude } else { palette.muted };
    if dim && !lit {
        color = color.opacity(0.35);
    }
    let width = match edge.style {
        LineStyle::Thick => 2.6,
        _ => 1.4,
    } + if lit { 0.8 } else { 0.0 };
    let dash = (edge.style == LineStyle::Dotted).then_some((2.0, 3.0));
    pen.stroke(&edge.points, false, width, color, dash);
    let last = edge.points.len() - 1;
    tip(
        pen,
        edge.points[last],
        edge.points[last - 1],
        edge.head,
        color,
        palette.background,
    );
    tip(
        pen,
        edge.points[0],
        edge.points[1],
        edge.tail,
        color,
        palette.background,
    );
    if let Some((text, center)) = &edge.label {
        let text_color = if dim && !lit {
            palette.muted.opacity(0.5)
        } else {
            palette.text2
        };
        pen.label(text, *center, text_color, palette.background);
    }
}

fn paint_flow(pen: &mut Pen, palette: &Palette, flow: &FlowLayout, focus: Option<&Focus>) {
    for group in &flow.groups {
        paint_group(pen, palette, group);
    }
    for edge in &flow.edges {
        let lit = focus
            .and_then(|focus| focus.edge.as_ref())
            .and_then(|(a, b)| flow.edge_between(a, b))
            .is_some_and(|lit| std::ptr::eq(lit, edge));
        paint_edge(pen, palette, edge, lit, focus.is_some());
    }
    for node in &flow.boxes {
        let state = focus.map_or(NodeState::Normal, |focus| {
            focus.node_state(&node.id, focus.has_visited(&node.id))
        });
        paint_box(pen, palette, node, state);
    }
}

fn block_name(kind: BlockKind) -> &'static str {
    match kind {
        BlockKind::Loop => "loop",
        BlockKind::Alt => "alt",
        BlockKind::Opt => "opt",
        BlockKind::Par => "par",
        BlockKind::Critical => "critical",
        BlockKind::Break => "break",
        BlockKind::Rect | BlockKind::Box => "",
    }
}

fn paint_block(pen: &mut Pen, palette: &Palette, block: &BlockGeom) {
    let tinted = matches!(block.kind, BlockKind::Rect | BlockKind::Box);
    let tone = Tone {
        stroke: if tinted { palette.line } else { palette.line2 },
        fill: if tinted {
            palette.claude_bg.opacity(0.5)
        } else {
            gpui::transparent_black()
        },
        text: palette.muted,
        width: 1.0,
        dashed: false,
    };
    pen.quad(block.rect, 5.0, &tone);
    let name = block_name(block.kind);
    let r = block.rect;
    if !name.is_empty() {
        let tab = Rect::new(r.x, r.y, pen.text_width(name) + 14.0, pen.line_height() + 4.0);
        let pill = Tone {
            stroke: palette.line2,
            fill: palette.raised,
            text: palette.text2,
            width: 1.0,
            dashed: false,
        };
        pen.quad(tab, 5.0, &pill);
        pen.text(name, tab.x + 7.0, tab.y + 2.0, palette.text2, true);
        if !block.label.is_empty() {
            let text = format!("[{}]", block.label);
            pen.text(&text, tab.right() + 8.0, tab.y + 2.0, palette.muted, false);
        }
    }
    for (y, label) in &block.branches {
        pen.stroke(
            &[Point::new(r.x, *y), Point::new(r.right(), *y)],
            false,
            1.0,
            palette.line2,
            Some((4.0, 3.0)),
        );
        if !label.is_empty() {
            let text = format!("[{label}]");
            pen.text(&text, r.x + 10.0, y + 3.0, palette.muted, false);
        }
    }
}

fn paint_participant(pen: &mut Pen, palette: &Palette, who: &ParticipantGeom) {
    pen.stroke(
        &[who.lifeline.0, who.lifeline.1],
        false,
        1.0,
        palette.line2,
        Some((4.0, 4.0)),
    );
    let tone = Tone {
        stroke: palette.line2,
        fill: palette.raised,
        text: palette.text,
        width: 1.2,
        dashed: false,
    };
    let radius = if who.kind == ParticipantKind::Actor {
        who.rect.h / 2.0
    } else {
        6.0
    };
    pen.quad(who.rect, radius, &tone);
    pen.centered(&who.label, who.rect.center(), palette.text, true);
}

fn paint_message(pen: &mut Pen, palette: &Palette, message: &MessageGeom) {
    if message.points.len() < 2 {
        return;
    }
    let color = palette.text2;
    let dash = message.arrow.is_dotted().then_some((4.0, 3.0));
    pen.stroke(&message.points, false, 1.3, color, dash);
    let last = message.points.len() - 1;
    let (end, before) = (message.points[last], message.points[last - 1]);
    match message.arrow {
        Arrow::Solid | Arrow::Dotted => tip(pen, end, before, Tip::Arrow, color, palette.background),
        Arrow::Cross | Arrow::DottedCross => tip(pen, end, before, Tip::Cross, color, palette.background),
        Arrow::SolidOpen | Arrow::DottedOpen => {}
        Arrow::Async | Arrow::DottedAsync => open_tip(pen, end, before, color),
    }
    if !message.text.is_empty() {
        pen.centered(&message.text, message.label, palette.text, false);
    }
}

fn paint_sequence(pen: &mut Pen, palette: &Palette, seq: &SequenceLayout) {
    for block in &seq.blocks {
        paint_block(pen, palette, block);
    }
    for who in &seq.participants {
        paint_participant(pen, palette, who);
    }
    for message in &seq.messages {
        paint_message(pen, palette, message);
    }
    for note in &seq.notes {
        let tone = Tone {
            stroke: palette.changed.opacity(0.6),
            fill: palette.changed.opacity(0.14),
            text: palette.text,
            width: 1.0,
            dashed: false,
        };
        pen.quad(note.rect, 4.0, &tone);
        pen.centered(&note.text, note.rect.center(), palette.text, false);
    }
}

pub struct Drawing {
    pub layout: std::rc::Rc<Layout>,
    pub palette: Palette,
    pub focus: Option<Focus>,
    pub font: Font,
    pub font_size: f32,
}

impl Drawing {
    pub fn paint(&self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
        let (width, height) = self.layout.size();
        let scale = super::focus::fit((width, height), f32::from(bounds.size.width)).scale;
        let mut pen = Pen {
            window,
            cx,
            origin: bounds.origin,
            scale,
            font: self.font.clone(),
            font_size: self.font_size,
        };
        match &*self.layout {
            Layout::Flow(flow) => paint_flow(&mut pen, &self.palette, flow, self.focus.as_ref()),
            Layout::Sequence(seq) => paint_sequence(&mut pen, &self.palette, seq),
        }
    }
}
