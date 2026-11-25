//! Reads the Mermaid text a guide stores into a typed graph, so the UI can lay it out itself.
//!
//! Flowcharts and sequence diagrams are read in full and rejected with a line number when they do not parse.
//! Every other kind (class, state, gantt, ...) is accepted as source only. The `:::added` classes and the
//! `%% ref node:<id> [[path#L12]]` links follow diffity's guide prompt (MIT, see THIRD_PARTY_NOTICES).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiagramKind {
    Flowchart,
    Sequence,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("Mermaid line {line}: {message}")]
pub struct MermaidError {
    pub line: usize,
    pub message: String,
}

impl MermaidError {
    fn new(line: usize, message: impl Into<String>) -> Self {
        Self {
            line,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Graph {
    Flowchart(Flowchart),
    Sequence(Sequence),
    /// A kind this parser does not read. The source is all there is.
    Source,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Direction {
    #[default]
    TopDown,
    BottomUp,
    LeftRight,
    RightLeft,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    Rect,
    Round,
    Stadium,
    Subroutine,
    Cylinder,
    Circle,
    Diamond,
    Hexagon,
    Asymmetric,
    Parallelogram,
    ParallelogramAlt,
    Trapezoid,
    TrapezoidAlt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub id: String,
    pub label: String,
    pub shape: Shape,
    /// From `A:::added` or `class A added`. The guide prompt uses added, changed and removed.
    pub class: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineStyle {
    Solid,
    Dotted,
    Thick,
    Invisible,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tip {
    None,
    Arrow,
    Circle,
    Cross,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub label: Option<String>,
    pub style: LineStyle,
    pub tail: Tip,
    pub head: Tip,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subgraph {
    pub id: String,
    pub title: String,
    pub nodes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefTarget {
    Node(String),
    Edge { from: String, to: String },
}

/// `%% ref node:A [[src/a.rs#L12]]`: the code a node or an arrow stands for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeRef {
    pub target: RefTarget,
    pub path: String,
    pub line: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Flowchart {
    pub direction: Direction,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub subgraphs: Vec<Subgraph>,
    pub refs: Vec<CodeRef>,
}

impl Flowchart {
    pub fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.iter().find(|node| node.id == id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParticipantKind {
    Participant,
    Actor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Participant {
    pub id: String,
    pub label: String,
    pub kind: ParticipantKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arrow {
    Solid,
    SolidOpen,
    Dotted,
    DottedOpen,
    Cross,
    DottedCross,
    Async,
    DottedAsync,
}

impl Arrow {
    pub fn is_dotted(self) -> bool {
        matches!(
            self,
            Self::Dotted | Self::DottedOpen | Self::DottedCross | Self::DottedAsync
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub from: String,
    pub to: String,
    pub text: String,
    pub arrow: Arrow,
    pub activates: bool,
    pub deactivates: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotePlace {
    LeftOf,
    RightOf,
    Over,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    Loop,
    Alt,
    Opt,
    Par,
    Critical,
    Break,
    Rect,
    Box,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    Message(Message),
    Note {
        place: NotePlace,
        participants: Vec<String>,
        text: String,
    },
    Activate(String),
    Deactivate(String),
    BlockStart {
        kind: BlockKind,
        label: String,
    },
    /// `else`, `and` or `option` inside the block above.
    BlockBranch {
        label: String,
    },
    BlockEnd,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Sequence {
    pub title: Option<String>,
    pub autonumber: bool,
    pub participants: Vec<Participant>,
    pub steps: Vec<Step>,
}

impl Sequence {
    pub fn messages(&self) -> impl Iterator<Item = &Message> {
        self.steps.iter().filter_map(|step| match step {
            Step::Message(message) => Some(message),
            _ => None,
        })
    }
}

struct Lines<'a> {
    numbered: Vec<(usize, &'a str)>,
}

/// The lines of the diagram after its front matter and `%%{init}%%` directives, with their line numbers.
fn body(source: &str) -> Lines<'_> {
    let mut numbered = Vec::new();
    let mut in_front = false;
    let mut seen_text = false;
    for (index, raw) in source.lines().enumerate() {
        let line = raw.trim();
        if !seen_text && index == 0 && line == "---" {
            in_front = true;
            continue;
        }
        if in_front {
            in_front = line != "---";
            continue;
        }
        if line.is_empty() || (line.starts_with("%%{") && line.ends_with("}%%")) {
            continue;
        }
        if !line.starts_with("%%") {
            seen_text = true;
        }
        numbered.push((index + 1, line));
    }
    Lines { numbered }
}

fn header_word(lines: &Lines) -> Option<(usize, String)> {
    lines
        .numbered
        .iter()
        .find(|(_, line)| !line.starts_with("%%"))
        .map(|(number, line)| {
            let word = line
                .split(|c: char| c.is_whitespace() || c == ';')
                .next()
                .unwrap_or_default();
            (*number, word.to_string())
        })
}

pub fn kind_of(source: &str) -> Option<DiagramKind> {
    let (_, word) = header_word(&body(source))?;
    Some(match word.as_str() {
        "flowchart" | "flowchart-elk" | "graph" => DiagramKind::Flowchart,
        "sequenceDiagram" => DiagramKind::Sequence,
        other => DiagramKind::Other(other.to_string()),
    })
}

pub fn parse(source: &str) -> Result<Graph, MermaidError> {
    let lines = body(source);
    let Some((header_line, word)) = header_word(&lines) else {
        return Err(MermaidError::new(1, "The diagram is empty."));
    };
    match word.as_str() {
        "flowchart" | "flowchart-elk" | "graph" => parse_flowchart(&lines, header_line).map(Graph::Flowchart),
        "sequenceDiagram" => parse_sequence(&lines, header_line).map(Graph::Sequence),
        _ => Ok(Graph::Source),
    }
}

struct Scan<'a> {
    rest: &'a str,
}

impl<'a> Scan<'a> {
    fn skip_space(&mut self) {
        self.rest = self.rest.trim_start();
    }

    fn peek(&self) -> Option<char> {
        self.rest.chars().next()
    }

    fn eat(&mut self, token: &str) -> bool {
        match self.rest.strip_prefix(token) {
            Some(rest) => {
                self.rest = rest;
                true
            }
            None => false,
        }
    }

    fn take_while(&mut self, keep: impl Fn(char) -> bool) -> &'a str {
        let end = self.rest.find(|c| !keep(c)).unwrap_or(self.rest.len());
        let (taken, rest) = self.rest.split_at(end);
        self.rest = rest;
        taken
    }

    fn is_empty(&self) -> bool {
        self.rest.is_empty()
    }
}

fn unquoted(text: &str) -> String {
    let text = text.trim();
    text.strip_prefix('"')
        .and_then(|inner| inner.strip_suffix('"'))
        .unwrap_or(text)
        .trim()
        .to_string()
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

struct FlowBuilder {
    chart: Flowchart,
    index: HashMap<String, usize>,
    open: Vec<usize>,
}

impl FlowBuilder {
    fn node_at(&mut self, id: &str) -> usize {
        if let Some(&at) = self.index.get(id) {
            return at;
        }
        self.chart.nodes.push(Node {
            id: id.to_string(),
            label: id.to_string(),
            shape: Shape::Rect,
            class: None,
        });
        self.index.insert(id.to_string(), self.chart.nodes.len() - 1);
        self.chart.nodes.len() - 1
    }

    fn join_subgraph(&mut self, id: &str) {
        if let Some(&at) = self.open.last() {
            let members = &mut self.chart.subgraphs[at].nodes;
            if !members.iter().any(|member| member == id) {
                members.push(id.to_string());
            }
        }
    }

    fn id<'a>(scan: &mut Scan<'a>) -> &'a str {
        let start = scan.rest;
        let mut end = 0;
        let mut chars = start.char_indices().peekable();
        while let Some((at, c)) = chars.next() {
            let hyphen_inside = c == '-' && at > 0 && chars.peek().is_some_and(|(_, next)| is_word(*next));
            if !is_word(c) && !hyphen_inside {
                break;
            }
            end = at + c.len_utf8();
        }
        scan.rest = &start[end..];
        &start[..end]
    }

    fn node(&mut self, scan: &mut Scan, line: usize) -> Result<usize, MermaidError> {
        scan.skip_space();
        let id = Self::id(scan);
        if id.is_empty() {
            let shown: String = scan.rest.chars().take(20).collect();
            return Err(MermaidError::new(line, format!("Expected a node name at `{shown}`.")));
        }
        let at = self.node_at(id);
        if let Some((shape, label)) = read_shape(scan, line)? {
            self.chart.nodes[at].shape = shape;
            self.chart.nodes[at].label = label;
        }
        if scan.eat(":::") {
            let class = scan.take_while(|c| is_word(c) || c == '-');
            self.chart.nodes[at].class = Some(class.to_string()).filter(|class| !class.is_empty());
        }
        self.join_subgraph(id);
        Ok(at)
    }

    fn group(&mut self, scan: &mut Scan, line: usize) -> Result<Vec<usize>, MermaidError> {
        let mut nodes = vec![self.node(scan, line)?];
        loop {
            scan.skip_space();
            if !scan.eat("&") {
                return Ok(nodes);
            }
            nodes.push(self.node(scan, line)?);
        }
    }

    fn statement(&mut self, text: &str, line: usize) -> Result<(), MermaidError> {
        let text = text.trim();
        let (word, rest) = text.split_once(char::is_whitespace).unwrap_or((text, ""));
        match word {
            "" => return Ok(()),
            "end" if rest.is_empty() => {
                return match self.open.pop() {
                    Some(_) => Ok(()),
                    None => Err(MermaidError::new(line, "`end` closes nothing.")),
                };
            }
            "subgraph" => return self.subgraph(rest, line),
            "direction" | "classDef" | "style" | "linkStyle" | "click" | "accTitle" | "accDescr" | "interpolate" => {
                return Ok(());
            }
            "class" if !rest.is_empty() => {
                let (names, class) = rest.rsplit_once(char::is_whitespace).unwrap_or((rest, ""));
                for id in names.split(',').map(str::trim).filter(|id| !id.is_empty()) {
                    if let Some(&at) = self.index.get(id) {
                        self.chart.nodes[at].class = Some(class.to_string());
                    }
                }
                return Ok(());
            }
            _ => {}
        }
        let mut scan = Scan { rest: text };
        let mut previous = self.group(&mut scan, line)?;
        loop {
            scan.skip_space();
            if scan.is_empty() {
                return Ok(());
            }
            let link = read_edge(&mut scan, line)?;
            scan.skip_space();
            if scan.is_empty() {
                return Err(MermaidError::new(line, "An arrow needs a node after it."));
            }
            let next = self.group(&mut scan, line)?;
            for &from in &previous {
                for &to in &next {
                    self.chart.edges.push(Edge {
                        from: self.chart.nodes[from].id.clone(),
                        to: self.chart.nodes[to].id.clone(),
                        label: link.label.clone(),
                        style: link.style,
                        tail: link.tail,
                        head: link.head,
                    });
                }
            }
            previous = next;
        }
    }

    fn subgraph(&mut self, rest: &str, line: usize) -> Result<(), MermaidError> {
        let rest = rest.trim();
        if rest.is_empty() {
            return Err(MermaidError::new(line, "A subgraph needs a name."));
        }
        let (id, title) = match rest.split_once('[') {
            Some((id, title)) => {
                let title = title
                    .trim_end()
                    .strip_suffix(']')
                    .ok_or_else(|| MermaidError::new(line, "The subgraph title is missing its `]`."))?;
                (id.trim().to_string(), unquoted(title))
            }
            None => (unquoted(rest), unquoted(rest)),
        };
        let id = if id.is_empty() { title.clone() } else { id };
        self.chart.subgraphs.push(Subgraph {
            id,
            title,
            nodes: Vec::new(),
        });
        self.open.push(self.chart.subgraphs.len() - 1);
        Ok(())
    }
}

/// The text between an opening bracket and its closer, which may be quoted.
fn label_until<'a>(scan: &mut Scan<'a>, closers: &[&str]) -> Option<(usize, String)> {
    let rest = scan.rest.trim_start();
    let (text, after) = if let Some(quoted) = rest.strip_prefix('"') {
        let end = quoted.find('"')?;
        (quoted[..end].to_string(), &quoted[end + 1..])
    } else {
        let at = closers.iter().filter_map(|closer| rest.find(closer)).min()?;
        (rest[..at].trim().to_string(), &rest[at..])
    };
    let after = after.trim_start();
    let (which, closer) = closers
        .iter()
        .enumerate()
        .find(|(_, closer)| after.starts_with(**closer))?;
    scan.rest = &after[closer.len()..];
    Some((which, text))
}

fn read_shape(scan: &mut Scan, line: usize) -> Result<Option<(Shape, String)>, MermaidError> {
    const TABLE: [(&str, &[(&str, Shape)]); 11] = [
        ("[[", &[("]]", Shape::Subroutine)]),
        ("[(", &[(")]", Shape::Cylinder)]),
        ("[/", &[("/]", Shape::Parallelogram), ("\\]", Shape::Trapezoid)]),
        ("[\\", &[("\\]", Shape::ParallelogramAlt), ("/]", Shape::TrapezoidAlt)]),
        ("[", &[("]", Shape::Rect)]),
        ("(((", &[(")))", Shape::Circle)]),
        ("((", &[("))", Shape::Circle)]),
        ("([", &[("])", Shape::Stadium)]),
        ("(", &[(")", Shape::Round)]),
        ("{{", &[("}}", Shape::Hexagon)]),
        ("{", &[("}", Shape::Diamond)]),
    ];
    let open = |scan: &mut Scan, opener: &str, closers: &[(&str, Shape)]| {
        let names: Vec<&str> = closers.iter().map(|(closer, _)| *closer).collect();
        let mut trial = Scan {
            rest: &scan.rest[opener.len()..],
        };
        let (which, label) = label_until(&mut trial, &names)?;
        scan.rest = trial.rest;
        Some((closers[which].1, label))
    };
    let mut candidates: Vec<(&str, &[(&str, Shape)])> = TABLE.to_vec();
    candidates.push((">", &[("]", Shape::Asymmetric)]));
    for (opener, closers) in candidates {
        if !scan.rest.starts_with(opener) {
            continue;
        }
        if let Some(found) = open(scan, opener, closers) {
            return Ok(Some(found));
        }
        if opener != "[/" && opener != "[\\" && opener != ">" {
            return Err(MermaidError::new(
                line,
                format!("`{opener}` is not closed with `{}`.", closers[0].0),
            ));
        }
    }
    Ok(None)
}

struct Link {
    label: Option<String>,
    style: LineStyle,
    tail: Tip,
    head: Tip,
}

fn read_tip(scan: &mut Scan) -> Tip {
    let mut chars = scan.rest.chars();
    let tip = match chars.next() {
        Some('>') => Tip::Arrow,
        Some('o') if chars.next().is_none_or(|next| !is_word(next)) => Tip::Circle,
        Some('x') if chars.next().is_none_or(|next| !is_word(next)) => Tip::Cross,
        _ => return Tip::None,
    };
    scan.rest = &scan.rest[1..];
    tip
}

fn read_edge(scan: &mut Scan, line: usize) -> Result<Link, MermaidError> {
    let shown: String = scan.rest.chars().take(20).collect();
    let bad = || MermaidError::new(line, format!("Expected an arrow such as `-->` at `{shown}`."));
    let mut tail = Tip::None;
    if scan.eat("<") {
        tail = Tip::Arrow;
    } else if let Some(marker @ ('o' | 'x')) = scan.peek()
        && ["--", "==", "-."]
            .iter()
            .any(|dashes| scan.rest[1..].starts_with(dashes))
    {
        tail = if marker == 'o' { Tip::Circle } else { Tip::Cross };
        scan.rest = &scan.rest[1..];
    }
    let style;
    let mut text = None;
    if scan.rest.starts_with("~~~") {
        scan.take_while(|c| c == '~');
        return Ok(Link {
            label: None,
            style: LineStyle::Invisible,
            tail,
            head: Tip::None,
        });
    } else if scan.rest.starts_with("==") {
        style = LineStyle::Thick;
        let run = scan.take_while(|c| c == '=').len();
        if run == 2 && scan.peek().is_some_and(char::is_whitespace) {
            let (before, after) = scan.rest.split_once("==").ok_or_else(bad)?;
            text = Some(unquoted(before));
            scan.rest = after;
            scan.take_while(|c| c == '=');
        }
    } else if scan.rest.starts_with("-.") {
        style = LineStyle::Dotted;
        scan.eat("-.");
        if scan.peek().is_some_and(char::is_whitespace) {
            let (before, after) = scan.rest.split_once(".-").ok_or_else(bad)?;
            text = Some(unquoted(before));
            scan.rest = after;
        } else {
            scan.take_while(|c| c == '.');
            if !scan.eat("-") {
                return Err(bad());
            }
        }
    } else if scan.rest.starts_with("--") {
        style = LineStyle::Solid;
        let run = scan.take_while(|c| c == '-').len();
        if run == 2 && scan.peek().is_some_and(char::is_whitespace) {
            let (before, after) = scan.rest.split_once("--").ok_or_else(bad)?;
            text = Some(unquoted(before));
            scan.rest = after;
            scan.take_while(|c| c == '-');
        }
    } else {
        return Err(bad());
    }
    let head = read_tip(scan);
    scan.skip_space();
    if text.is_none() && scan.rest.starts_with('|') {
        let inner = &scan.rest[1..];
        let end = inner
            .find('|')
            .ok_or_else(|| MermaidError::new(line, "The arrow label is missing its closing `|`."))?;
        text = Some(unquoted(&inner[..end]));
        scan.rest = &inner[end + 1..];
    }
    Ok(Link {
        label: text.filter(|text| !text.is_empty()),
        style,
        tail,
        head,
    })
}

fn split_statements(line: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut start, mut quoted) = (0, false);
    for (at, c) in line.char_indices() {
        match c {
            '"' => quoted = !quoted,
            ';' if !quoted => {
                parts.push(&line[start..at]);
                start = at + 1;
            }
            _ => {}
        }
    }
    parts.push(&line[start..]);
    parts
}

fn code_ref(comment: &str) -> Option<CodeRef> {
    let rest = comment.trim_start_matches('%').trim().strip_prefix("ref ")?.trim();
    let (target, link) = rest.split_once(char::is_whitespace)?;
    let target = match target.strip_prefix("node:") {
        Some(id) => RefTarget::Node(id.to_string()),
        None => {
            let (from, to) = target.strip_prefix("edge:")?.split_once("->")?;
            RefTarget::Edge {
                from: from.to_string(),
                to: to.to_string(),
            }
        }
    };
    let link = link.trim().strip_prefix("[[")?.strip_suffix("]]")?;
    let (path, line) = match link.rsplit_once("#L") {
        Some((path, line)) => (path, line.split('-').next().and_then(|line| line.parse().ok())),
        None => (link, None),
    };
    Some(CodeRef {
        target,
        path: path.to_string(),
        line,
    })
}

fn parse_flowchart(lines: &Lines, header_line: usize) -> Result<Flowchart, MermaidError> {
    let mut builder = FlowBuilder {
        chart: Flowchart::default(),
        index: HashMap::new(),
        open: Vec::new(),
    };
    for &(number, text) in &lines.numbered {
        if text.starts_with("%%") {
            if let Some(found) = code_ref(text) {
                builder.chart.refs.push(found);
            }
            continue;
        }
        if number == header_line {
            let mut words = text
                .split(|c: char| c.is_whitespace() || c == ';')
                .filter(|word| !word.is_empty());
            words.next();
            builder.chart.direction = match words.next() {
                Some("LR") => Direction::LeftRight,
                Some("RL") => Direction::RightLeft,
                Some("BT") => Direction::BottomUp,
                Some("TB" | "TD") | None => Direction::TopDown,
                Some(other) => {
                    return Err(MermaidError::new(
                        number,
                        format!("`{other}` is not a direction. Use TD, TB, BT, LR or RL."),
                    ));
                }
            };
            continue;
        }
        let text = text.split(" %%").next().unwrap_or(text);
        for part in split_statements(text) {
            builder.statement(part, number)?;
        }
    }
    if !builder.open.is_empty() {
        return Err(MermaidError::new(
            lines.numbered.last().map_or(1, |(number, _)| *number),
            "A `subgraph` is missing its `end`.",
        ));
    }
    if builder.chart.nodes.is_empty() {
        return Err(MermaidError::new(header_line, "The flowchart has no nodes."));
    }
    Ok(builder.chart)
}

const ARROWS: [(&str, Arrow); 8] = [
    ("-->>", Arrow::Dotted),
    ("->>", Arrow::Solid),
    ("--x", Arrow::DottedCross),
    ("-x", Arrow::Cross),
    ("--)", Arrow::DottedAsync),
    ("-)", Arrow::Async),
    ("-->", Arrow::DottedOpen),
    ("->", Arrow::SolidOpen),
];

fn arrow_in(line: &str) -> Option<(usize, &'static str, Arrow)> {
    let mut best: Option<(usize, &'static str, Arrow)> = None;
    for (token, arrow) in ARROWS {
        if let Some(at) = line.find(token) {
            let better =
                best.is_none_or(|(found, current, _)| at < found || (at == found && token.len() > current.len()));
            if better {
                best = Some((at, token, arrow));
            }
        }
    }
    best
}

fn declare(sequence: &mut Sequence, id: &str, label: Option<&str>, kind: ParticipantKind) {
    match sequence.participants.iter_mut().find(|known| known.id == id) {
        Some(known) => {
            if let Some(label) = label {
                known.label = label.to_string();
                known.kind = kind;
            }
        }
        None => sequence.participants.push(Participant {
            id: id.to_string(),
            label: label.unwrap_or(id).to_string(),
            kind,
        }),
    }
}

fn parse_sequence(lines: &Lines, header_line: usize) -> Result<Sequence, MermaidError> {
    let mut sequence = Sequence::default();
    let mut open_blocks = 0usize;
    for &(number, text) in &lines.numbered {
        if text.starts_with("%%") || number == header_line {
            continue;
        }
        let text = text.trim_end_matches(';').trim();
        let (word, rest) = text.split_once(char::is_whitespace).unwrap_or((text, ""));
        let rest = rest.trim();
        match word {
            "participant" | "actor" => {
                let kind = if word == "actor" {
                    ParticipantKind::Actor
                } else {
                    ParticipantKind::Participant
                };
                let (id, label) = match rest.split_once(" as ") {
                    Some((id, label)) => (id.trim(), Some(label.trim())),
                    None => (rest, None),
                };
                if id.is_empty() {
                    return Err(MermaidError::new(number, format!("`{word}` needs a name.")));
                }
                declare(&mut sequence, id, label, kind);
            }
            "autonumber" => sequence.autonumber = true,
            "title" => sequence.title = Some(rest.to_string()),
            "activate" | "deactivate" => {
                declare(&mut sequence, rest, None, ParticipantKind::Participant);
                let id = rest.to_string();
                sequence.steps.push(if word == "activate" {
                    Step::Activate(id)
                } else {
                    Step::Deactivate(id)
                });
            }
            "loop" | "alt" | "opt" | "par" | "critical" | "break" | "rect" | "box" => {
                open_blocks += 1;
                let kind = match word {
                    "loop" => BlockKind::Loop,
                    "alt" => BlockKind::Alt,
                    "opt" => BlockKind::Opt,
                    "par" => BlockKind::Par,
                    "critical" => BlockKind::Critical,
                    "break" => BlockKind::Break,
                    "rect" => BlockKind::Rect,
                    _ => BlockKind::Box,
                };
                sequence.steps.push(Step::BlockStart {
                    kind,
                    label: rest.to_string(),
                });
            }
            "else" | "and" | "option" => {
                if open_blocks == 0 {
                    return Err(MermaidError::new(number, format!("`{word}` is outside a block.")));
                }
                sequence.steps.push(Step::BlockBranch {
                    label: rest.to_string(),
                });
            }
            "end" => {
                if open_blocks == 0 {
                    return Err(MermaidError::new(number, "`end` closes nothing."));
                }
                open_blocks -= 1;
                sequence.steps.push(Step::BlockEnd);
            }
            "Note" | "note" => {
                let (place, rest) = if let Some(rest) = rest.strip_prefix("left of ") {
                    (NotePlace::LeftOf, rest)
                } else if let Some(rest) = rest.strip_prefix("right of ") {
                    (NotePlace::RightOf, rest)
                } else if let Some(rest) = rest.strip_prefix("over ") {
                    (NotePlace::Over, rest)
                } else {
                    return Err(MermaidError::new(
                        number,
                        "A note is `Note left of A: text`, `Note right of A: text` or `Note over A,B: text`.",
                    ));
                };
                let (who, note) = rest
                    .split_once(':')
                    .ok_or_else(|| MermaidError::new(number, "A note needs `:` and its text."))?;
                let participants: Vec<String> = who.split(',').map(|id| id.trim().to_string()).collect();
                for id in &participants {
                    declare(&mut sequence, id, None, ParticipantKind::Participant);
                }
                sequence.steps.push(Step::Note {
                    place,
                    participants,
                    text: note.trim().to_string(),
                });
            }
            _ => {
                let Some((at, token, arrow)) = arrow_in(text) else {
                    return Err(MermaidError::new(
                        number,
                        format!(
                            "Cannot read `{}`. A message looks like `A->>B: text`.",
                            text.chars().take(40).collect::<String>()
                        ),
                    ));
                };
                let from = text[..at].trim();
                let (target, message) = match text[at + token.len()..].split_once(':') {
                    Some((target, message)) => (target.trim(), message.trim()),
                    None => (text[at + token.len()..].trim(), ""),
                };
                let (activates, target) = match target.strip_prefix('+') {
                    Some(target) => (true, target.trim()),
                    None => (false, target),
                };
                let (deactivates, target) = match target.strip_prefix('-') {
                    Some(target) => (true, target.trim()),
                    None => (false, target),
                };
                if from.is_empty() || target.is_empty() {
                    return Err(MermaidError::new(number, "A message needs a sender and a receiver."));
                }
                declare(&mut sequence, from, None, ParticipantKind::Participant);
                declare(&mut sequence, target, None, ParticipantKind::Participant);
                sequence.steps.push(Step::Message(Message {
                    from: from.to_string(),
                    to: target.to_string(),
                    text: message.to_string(),
                    arrow,
                    activates,
                    deactivates,
                }));
            }
        }
    }
    if open_blocks > 0 {
        return Err(MermaidError::new(
            lines.numbered.last().map_or(1, |(number, _)| *number),
            "A block (`loop`, `alt`, `opt`, ...) is missing its `end`.",
        ));
    }
    if sequence.participants.is_empty() {
        return Err(MermaidError::new(
            header_line,
            "The sequence diagram has no participants.",
        ));
    }
    Ok(sequence)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flow(source: &str) -> Flowchart {
        match parse(source) {
            Ok(Graph::Flowchart(chart)) => chart,
            other => panic!("not a flowchart: {other:?}\n{source}"),
        }
    }

    fn sequence(source: &str) -> Sequence {
        match parse(source) {
            Ok(Graph::Sequence(sequence)) => sequence,
            other => panic!("not a sequence diagram: {other:?}\n{source}"),
        }
    }

    fn edges(chart: &Flowchart) -> Vec<(&str, &str, Option<&str>)> {
        chart
            .edges
            .iter()
            .map(|edge| (edge.from.as_str(), edge.to.as_str(), edge.label.as_deref()))
            .collect()
    }

    #[test]
    fn every_node_shape_of_the_mermaid_docs_is_read() {
        let chart = flow(
            "flowchart LR
    id1[This is the text in the box]
    id2(This is the text in the box)
    id3([Stadium shape])
    id4[[Subroutine shape]]
    id5[(Database)]
    id6((Circle))
    id7>Asymmetric shape]
    id8{Rhombus}
    id9{{Hexagon}}
    id10[/Parallelogram/]
    id11[\\Parallelogram alt\\]
    id12[/Trapezoid\\]
    id13[\\Trapezoid alt/]
    id14(((Double circle)))",
        );
        let shapes: Vec<Shape> = chart.nodes.iter().map(|node| node.shape).collect();
        assert_eq!(
            shapes,
            [
                Shape::Rect,
                Shape::Round,
                Shape::Stadium,
                Shape::Subroutine,
                Shape::Cylinder,
                Shape::Circle,
                Shape::Asymmetric,
                Shape::Diamond,
                Shape::Hexagon,
                Shape::Parallelogram,
                Shape::ParallelogramAlt,
                Shape::Trapezoid,
                Shape::TrapezoidAlt,
                Shape::Circle,
            ]
        );
        assert_eq!(chart.direction, Direction::LeftRight);
        assert_eq!(chart.node("id1").unwrap().label, "This is the text in the box");
        assert_eq!(chart.node("id11").unwrap().label, "Parallelogram alt");
        assert_eq!(chart.node("id14").unwrap().label, "Double circle");
    }

    #[test]
    fn a_guide_style_flowchart_keeps_classes_labels_and_code_links() {
        let chart = flow(
            "flowchart LR
  Client[Browser]:::changed -->|POST /undo| API(Undo handler):::added
  API --> Timer[[Timer]]:::added
  Timer -. fires .-> Outbox[(Outbox)]
  Old[Direct send]:::removed
  %% ref node:API [[src/api.ts#L12]]
  %% ref edge:Client->API [[src/client.ts#L40-L48]]
  %% ref node:Outbox [[src/outbox.ts]]",
        );
        assert_eq!(
            edges(&chart),
            [
                ("Client", "API", Some("POST /undo")),
                ("API", "Timer", None),
                ("Timer", "Outbox", Some("fires")),
            ]
        );
        assert_eq!(chart.edges[2].style, LineStyle::Dotted);
        let classes: Vec<_> = chart.nodes.iter().map(|node| node.class.as_deref()).collect();
        assert_eq!(
            classes,
            [Some("changed"), Some("added"), Some("added"), None, Some("removed")]
        );
        assert_eq!(chart.node("Client").unwrap().label, "Browser");
        assert_eq!(
            chart.refs,
            [
                CodeRef {
                    target: RefTarget::Node("API".into()),
                    path: "src/api.ts".into(),
                    line: Some(12)
                },
                CodeRef {
                    target: RefTarget::Edge {
                        from: "Client".into(),
                        to: "API".into()
                    },
                    path: "src/client.ts".into(),
                    line: Some(40)
                },
                CodeRef {
                    target: RefTarget::Node("Outbox".into()),
                    path: "src/outbox.ts".into(),
                    line: None
                },
            ]
        );
    }

    #[test]
    fn arrows_of_every_style_chains_and_groups_are_read() {
        let chart = flow(
            "graph TD
    A --> B --> C
    A & B --> D & E
    C -- text --> F
    C -- \"quoted text\" --- G
    D == thick ==> H
    D ===> I
    E -.-> J
    E -. dotted text .-> K
    F --o G
    F --x H
    G <--> H
    I ~~~ J
    K -->|pipe label| L; L --> M",
        );
        assert_eq!(chart.direction, Direction::TopDown);
        let pairs: Vec<_> = edges(&chart).iter().map(|(a, b, _)| (*a, *b)).collect();
        assert_eq!(&pairs[..2], [("A", "B"), ("B", "C")]);
        assert_eq!(&pairs[2..6], [("A", "D"), ("A", "E"), ("B", "D"), ("B", "E")]);
        let edge = |from: &str, to: &str| chart.edges.iter().find(|e| e.from == from && e.to == to).unwrap();
        assert_eq!(edge("C", "F").label.as_deref(), Some("text"));
        assert_eq!(
            (edge("C", "F").head, edge("C", "F").style),
            (Tip::Arrow, LineStyle::Solid)
        );
        assert_eq!(
            (edge("C", "G").label.as_deref(), edge("C", "G").head),
            (Some("quoted text"), Tip::None)
        );
        assert_eq!(
            (edge("D", "H").style, edge("D", "H").label.as_deref()),
            (LineStyle::Thick, Some("thick"))
        );
        assert_eq!(
            (edge("D", "I").style, edge("D", "I").head),
            (LineStyle::Thick, Tip::Arrow)
        );
        assert_eq!(
            (edge("E", "J").style, edge("E", "J").head),
            (LineStyle::Dotted, Tip::Arrow)
        );
        assert_eq!(edge("E", "K").label.as_deref(), Some("dotted text"));
        assert_eq!(edge("F", "G").head, Tip::Circle);
        assert_eq!(edge("F", "H").head, Tip::Cross);
        assert_eq!((edge("G", "H").tail, edge("G", "H").head), (Tip::Arrow, Tip::Arrow));
        assert_eq!(edge("I", "J").style, LineStyle::Invisible);
        assert_eq!(edge("K", "L").label.as_deref(), Some("pipe label"));
        assert_eq!((edge("L", "M").from.as_str(), chart.nodes.len()), ("L", 13));
    }

    #[test]
    fn subgraphs_collect_their_nodes_and_styling_lines_are_skipped() {
        let chart = flow(
            "---
title: Release flow
---
%%{init: {'theme': 'neutral'}}%%
flowchart TB
    subgraph client [Client side]
      direction LR
      A & B --> C
    end
    subgraph server
      D[(Store)]
      subgraph inner [\"Inner box\"]
        E
      end
    end
    C ==> D
    classDef changed fill:#f96,stroke:#333;
    class A,B changed
    style C fill:#bbf
    linkStyle 0 stroke:#f00
    click A callback \"Tooltip\"",
        );
        let titles: Vec<_> = chart
            .subgraphs
            .iter()
            .map(|sub| (sub.id.as_str(), sub.title.as_str()))
            .collect();
        assert_eq!(
            titles,
            [("client", "Client side"), ("server", "server"), ("inner", "Inner box")]
        );
        assert_eq!(chart.subgraphs[0].nodes, ["A", "B", "C"]);
        assert_eq!(chart.subgraphs[1].nodes, ["D"]);
        assert_eq!(chart.subgraphs[2].nodes, ["E"]);
        assert_eq!(chart.node("A").unwrap().class.as_deref(), Some("changed"));
        assert_eq!(chart.node("D").unwrap().label, "Store");
        assert_eq!(chart.edges.len(), 3);
    }

    #[test]
    fn a_plain_graph_header_with_a_semicolon_and_hyphenated_ids_is_read() {
        let chart = flow("graph LR;\n  build-step --> test-step;\n  test-step --> deploy_2");
        assert_eq!(
            edges(&chart),
            [("build-step", "test-step", None), ("test-step", "deploy_2", None)]
        );
        assert_eq!(flow("flowchart\n  A --> B").direction, Direction::TopDown);
    }

    #[test]
    fn a_quoted_label_may_hold_brackets_and_a_slash_label_falls_back_to_a_box() {
        let chart = flow("flowchart TD\n  A[\"list[0] (first)\"] --> B[/api/users]\n  B --> C(\"Done; really\")");
        assert_eq!(chart.node("A").unwrap().label, "list[0] (first)");
        assert_eq!(
            (chart.node("B").unwrap().shape, chart.node("B").unwrap().label.as_str()),
            (Shape::Rect, "/api/users")
        );
        assert_eq!(chart.node("C").unwrap().label, "Done; really");
    }

    #[test]
    fn a_flowchart_that_does_not_parse_says_which_line() {
        let cases = [
            ("flowchart LR\n  A --> B\n  B -->", 3, "needs a node"),
            ("flowchart LR\n  A[never closed --> B", 2, "not closed"),
            ("flowchart LR\n  A --> B\n  subgraph x\n  C", 4, "`end`"),
            ("flowchart LR\n  A --> B\n  end", 3, "closes nothing"),
            ("flowchart XY\n  A --> B", 1, "not a direction"),
            ("flowchart LR\n  A --> B\n  --> C", 3, "Expected a node"),
            ("flowchart LR\n  A B", 2, "arrow"),
            ("flowchart LR", 1, "no nodes"),
            ("", 1, "empty"),
        ];
        for (source, line, words) in cases {
            let error = parse(source).unwrap_err();
            assert_eq!(error.line, line, "{source:?}: {error}");
            assert!(error.message.contains(words), "{source:?}: {error}");
        }
    }

    const UNDO_SEND: &str = "sequenceDiagram
    title Undo send
    autonumber
    actor U as User
    participant UI as Compose view
    participant S as Outbox service
    U->>UI: Press Send
    UI->>+S: queue(message)
    Note right of S: Starts a 5 s timer
    alt Undo within 5 s
        U->>UI: Press Undo
        UI->>S: cancel(message)
        S-->>UI: cancelled
    else Timer fires
        S--)UI: sent
    end
    loop every minute
        S-xS: retry failed sends
    end
    Note over U,UI: Done
    S-->>-UI: ok
    activate UI
    deactivate UI";

    #[test]
    fn a_sequence_diagram_gives_participants_messages_notes_and_blocks() {
        let diagram = sequence(UNDO_SEND);
        assert_eq!(
            (diagram.title.as_deref(), diagram.autonumber),
            (Some("Undo send"), true)
        );
        let people: Vec<_> = diagram
            .participants
            .iter()
            .map(|p| (p.id.as_str(), p.label.as_str(), p.kind))
            .collect();
        assert_eq!(
            people,
            [
                ("U", "User", ParticipantKind::Actor),
                ("UI", "Compose view", ParticipantKind::Participant),
                ("S", "Outbox service", ParticipantKind::Participant)
            ]
        );
        let messages: Vec<_> = diagram
            .messages()
            .map(|m| (m.from.as_str(), m.to.as_str(), m.text.as_str(), m.arrow))
            .collect();
        assert_eq!(
            messages,
            [
                ("U", "UI", "Press Send", Arrow::Solid),
                ("UI", "S", "queue(message)", Arrow::Solid),
                ("U", "UI", "Press Undo", Arrow::Solid),
                ("UI", "S", "cancel(message)", Arrow::Solid),
                ("S", "UI", "cancelled", Arrow::Dotted),
                ("S", "UI", "sent", Arrow::DottedAsync),
                ("S", "S", "retry failed sends", Arrow::Cross),
                ("S", "UI", "ok", Arrow::Dotted),
            ]
        );
        let queue = diagram.messages().nth(1).unwrap();
        assert!(queue.activates && !queue.deactivates);
        assert!(diagram.messages().last().unwrap().deactivates);
        let shape: Vec<&str> = diagram
            .steps
            .iter()
            .map(|step| match step {
                Step::Message(_) => "message",
                Step::Note { .. } => "note",
                Step::BlockStart { .. } => "start",
                Step::BlockBranch { .. } => "branch",
                Step::BlockEnd => "end",
                Step::Activate(_) => "activate",
                Step::Deactivate(_) => "deactivate",
            })
            .collect();
        assert_eq!(
            shape,
            [
                "message",
                "message",
                "note",
                "start",
                "message",
                "message",
                "message",
                "branch",
                "message",
                "end",
                "start",
                "message",
                "end",
                "note",
                "message",
                "activate",
                "deactivate"
            ]
        );
        assert!(matches!(
            &diagram.steps[2],
            Step::Note { place: NotePlace::RightOf, participants, text } if participants == &["S"] && text == "Starts a 5 s timer"
        ));
        assert!(
            matches!(&diagram.steps[3], Step::BlockStart { kind: BlockKind::Alt, label } if label == "Undo within 5 s")
        );
    }

    #[test]
    fn participants_are_made_on_first_use() {
        let diagram = sequence("sequenceDiagram\n  Alice->>John: Hello John, how are you?\n  John-->>Alice: Great!");
        let ids: Vec<_> = diagram.participants.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["Alice", "John"]);
        assert_eq!(diagram.messages().count(), 2);
    }

    #[test]
    fn a_sequence_diagram_that_does_not_parse_says_which_line() {
        let cases = [
            ("sequenceDiagram\n  A->>B: x\n  loop forever\n  B->>A: y", 4, "`end`"),
            ("sequenceDiagram\n  A->>B: x\n  end", 3, "closes nothing"),
            ("sequenceDiagram\n  A->>B: x\n  else y", 3, "outside a block"),
            ("sequenceDiagram\n  A->>B: x\n  this is not a message", 3, "Cannot read"),
            ("sequenceDiagram\n  Note under A: x", 2, "A note is"),
            ("sequenceDiagram\n  A->>: x", 2, "sender and a receiver"),
            ("sequenceDiagram", 1, "no participants"),
        ];
        for (source, line, words) in cases {
            let error = parse(source).unwrap_err();
            assert_eq!(error.line, line, "{source:?}: {error}");
            assert!(error.message.contains(words), "{source:?}: {error}");
        }
    }

    #[test]
    fn other_kinds_keep_their_source_only() {
        for (source, word) in [
            ("classDiagram\n  class Animal", "classDiagram"),
            ("stateDiagram-v2\n  [*] --> Still", "stateDiagram-v2"),
            ("erDiagram\n  CUSTOMER ||--o{ ORDER : places", "erDiagram"),
            ("gantt\n  title A Gantt", "gantt"),
            ("pie title Pets\n  \"Dogs\" : 386", "pie"),
        ] {
            assert_eq!(parse(source), Ok(Graph::Source), "{source}");
            assert_eq!(kind_of(source), Some(DiagramKind::Other(word.into())));
        }
    }

    #[test]
    fn the_kind_is_read_past_front_matter_comments_and_directives() {
        assert_eq!(
            kind_of("---\ntitle: x\n---\n%% a note\n%%{init: {}}%%\nflowchart LR\nA-->B"),
            Some(DiagramKind::Flowchart)
        );
        assert_eq!(
            kind_of("---\ntitle: x\n---\nsequenceDiagram\nA->>B: x"),
            Some(DiagramKind::Sequence)
        );
        assert_eq!(kind_of("graph TD\nA-->B"), Some(DiagramKind::Flowchart));
        assert_eq!(kind_of("  \n\n"), None);
    }
}
