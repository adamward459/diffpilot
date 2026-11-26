//! Turns a parsed Mermaid graph into positions and lines, so the app can paint it on a canvas.
//! It has no drawing code and no font. The caller supplies a `Measure` to size the text.

mod flow;
mod sequence;

use super::mermaid::{Arrow, BlockKind, Graph, LineStyle, ParticipantKind, Shape, Tip};

pub use flow::flow_layout;
pub use sequence::sequence_layout;

pub trait Measure {
    fn text_width(&self, text: &str) -> f32;
    fn line_height(&self) -> f32;
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    fn shifted(self, dx: f32, dy: f32) -> Self {
        Self::new(self.x + dx, self.y + dy)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

const EPS: f32 = 0.01;

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn centered(center: Point, w: f32, h: f32) -> Self {
        Self::new(center.x - w / 2.0, center.y - h / 2.0, w, h)
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn center(&self) -> Point {
        Point::new(self.x + self.w / 2.0, self.y + self.h / 2.0)
    }

    pub fn contains(&self, other: &Rect) -> bool {
        other.x >= self.x - EPS
            && other.y >= self.y - EPS
            && other.right() <= self.right() + EPS
            && other.bottom() <= self.bottom() + EPS
    }

    pub fn intersects(&self, other: &Rect) -> bool {
        self.x < other.right() - EPS
            && other.x < self.right() - EPS
            && self.y < other.bottom() - EPS
            && other.y < self.bottom() - EPS
    }

    pub fn on_border(&self, p: Point) -> bool {
        let inside_x = p.x >= self.x - EPS && p.x <= self.right() + EPS;
        let inside_y = p.y >= self.y - EPS && p.y <= self.bottom() + EPS;
        let on_x = (p.x - self.x).abs() <= EPS || (p.x - self.right()).abs() <= EPS;
        let on_y = (p.y - self.y).abs() <= EPS || (p.y - self.bottom()).abs() <= EPS;
        inside_x && inside_y && (on_x || on_y)
    }

    fn union(self, other: Rect) -> Rect {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        Rect::new(
            x,
            y,
            self.right().max(other.right()) - x,
            self.bottom().max(other.bottom()) - y,
        )
    }

    fn shifted(self, dx: f32, dy: f32) -> Self {
        Self::new(self.x + dx, self.y + dy, self.w, self.h)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BoxGeom {
    pub id: String,
    pub label: String,
    pub shape: Shape,
    pub rect: Rect,
    /// `added`, `changed` or `removed` when the diagram says so.
    pub class: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EdgeGeom {
    pub from: String,
    pub to: String,
    /// The first point is on the source border and the last on the target border.
    pub points: Vec<Point>,
    pub label: Option<(String, Point)>,
    pub style: LineStyle,
    pub tail: Tip,
    pub head: Tip,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GroupGeom {
    pub id: String,
    pub title: String,
    pub rect: Rect,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct FlowLayout {
    pub width: f32,
    pub height: f32,
    pub boxes: Vec<BoxGeom>,
    pub edges: Vec<EdgeGeom>,
    pub groups: Vec<GroupGeom>,
}

impl FlowLayout {
    pub fn node(&self, id: &str) -> Option<&BoxGeom> {
        self.boxes.iter().find(|b| b.id == id)
    }

    pub fn edge(&self, from: &str, to: &str) -> Option<&EdgeGeom> {
        self.edges.iter().find(|e| e.from == from && e.to == to)
    }

    /// The edge from `a` to `b`, or else the one from `b` to `a`.
    pub fn edge_between(&self, a: &str, b: &str) -> Option<&EdgeGeom> {
        self.edge(a, b).or_else(|| self.edge(b, a))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParticipantGeom {
    pub id: String,
    pub label: String,
    pub kind: ParticipantKind,
    pub rect: Rect,
    pub lifeline: (Point, Point),
}

#[derive(Debug, Clone, PartialEq)]
pub struct MessageGeom {
    pub from: String,
    pub to: String,
    pub text: String,
    pub arrow: Arrow,
    /// Two points for a plain arrow. A message to oneself loops out to the right and back.
    pub points: Vec<Point>,
    pub label: Point,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NoteGeom {
    pub text: String,
    pub rect: Rect,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BlockGeom {
    pub kind: BlockKind,
    pub label: String,
    pub rect: Rect,
    /// `else` and `and` lines: the y of the divider and its label.
    pub branches: Vec<(f32, String)>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SequenceLayout {
    pub width: f32,
    pub height: f32,
    pub participants: Vec<ParticipantGeom>,
    pub messages: Vec<MessageGeom>,
    pub notes: Vec<NoteGeom>,
    /// Outer blocks come before the blocks inside them.
    pub blocks: Vec<BlockGeom>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Layout {
    Flow(FlowLayout),
    Sequence(SequenceLayout),
}

impl Layout {
    pub fn size(&self) -> (f32, f32) {
        match self {
            Layout::Flow(l) => (l.width, l.height),
            Layout::Sequence(l) => (l.width, l.height),
        }
    }
}

/// `None` for a diagram kind the parser keeps as source only.
pub fn layout(graph: &Graph, measure: &dyn Measure) -> Option<Layout> {
    match graph {
        Graph::Flowchart(chart) => Some(Layout::Flow(flow_layout(chart, measure))),
        Graph::Sequence(seq) => Some(Layout::Sequence(sequence_layout(seq, measure))),
        Graph::Source => None,
    }
}

fn text_size(text: &str, measure: &dyn Measure) -> (f32, f32) {
    let lines = text.lines().count().max(1) as f32;
    let width = text.lines().map(|line| measure.text_width(line)).fold(0.0, f32::max);
    (width, lines * measure.line_height())
}

#[cfg(test)]
pub(crate) struct FakeMeasure;

#[cfg(test)]
impl Measure for FakeMeasure {
    fn text_width(&self, text: &str) -> f32 {
        text.chars().count() as f32 * 8.0
    }

    fn line_height(&self) -> f32 {
        16.0
    }
}
