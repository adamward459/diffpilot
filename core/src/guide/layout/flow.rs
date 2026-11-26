use super::{BoxGeom, EdgeGeom, FlowLayout, GroupGeom, Measure, Point, Rect, text_size};
use crate::guide::mermaid::{Direction, Flowchart, Shape};
use std::collections::{HashMap, VecDeque};

const PAD_X: f32 = 14.0;
const PAD_Y: f32 = 8.0;
const MIN_WIDTH: f32 = 48.0;
const LAYER_GAP: f32 = 56.0;
const NODE_GAP: f32 = 32.0;
const GROUP_PAD: f32 = 16.0;
const MARGIN: f32 = 16.0;
const LOOP_OUT: f32 = 24.0;
const SWEEPS: usize = 4;

pub fn flow_layout(chart: &Flowchart, measure: &dyn Measure) -> FlowLayout {
    let count = chart.nodes.len();
    let index: HashMap<&str, usize> = chart
        .nodes
        .iter()
        .enumerate()
        .map(|(i, node)| (node.id.as_str(), i))
        .collect();
    let links: Vec<(usize, usize)> = chart
        .edges
        .iter()
        .filter_map(|e| Some((*index.get(e.from.as_str())?, *index.get(e.to.as_str())?)))
        .collect();
    let dag = break_cycles(count, &links);
    let rank = longest_path_ranks(count, &dag);
    let layers = order_layers(count, &rank, &dag, &group_of(chart, &index));

    let sizes: Vec<(f32, f32)> = chart
        .nodes
        .iter()
        .map(|node| node_size(&node.label, node.shape, measure))
        .collect();
    let horizontal = matches!(chart.direction, Direction::LeftRight | Direction::RightLeft);
    let reversed = matches!(chart.direction, Direction::BottomUp | Direction::RightLeft);
    let has_labels = chart.edges.iter().any(|e| e.label.is_some());
    let layer_gap = LAYER_GAP + if has_labels { measure.line_height() } else { 0.0 };

    let mut centers = vec![Point::default(); count];
    let mut flow_at = 0.0;
    let mut placed: Vec<(Vec<(usize, f32)>, f32)> = Vec::new();
    for layer in &layers {
        let cross = |n: usize| if horizontal { sizes[n].1 } else { sizes[n].0 };
        let flow = |n: usize| if horizontal { sizes[n].0 } else { sizes[n].1 };
        let thickness = layer.iter().map(|&n| flow(n)).fold(0.0, f32::max);
        let total: f32 = layer.iter().map(|&n| cross(n)).sum::<f32>() + NODE_GAP * layer.len().saturating_sub(1) as f32;
        let mut at = -total / 2.0;
        let mut row = Vec::new();
        for &n in layer {
            row.push((n, at + cross(n) / 2.0));
            at += cross(n) + NODE_GAP;
        }
        placed.push((row, flow_at + thickness / 2.0));
        flow_at += thickness + layer_gap;
    }
    let flow_total = (flow_at - layer_gap).max(0.0);
    for (row, flow) in &placed {
        let flow = if reversed { flow_total - flow } else { *flow };
        for &(n, cross) in row {
            centers[n] = if horizontal {
                Point::new(flow, cross)
            } else {
                Point::new(cross, flow)
            };
        }
    }

    let rects: Vec<Rect> = (0..count)
        .map(|n| Rect::centered(centers[n], sizes[n].0, sizes[n].1))
        .collect();
    let title_h = measure.line_height() + 8.0;
    let groups: Vec<GroupGeom> = chart
        .subgraphs
        .iter()
        .filter_map(|sub| {
            let members: Vec<Rect> = sub
                .nodes
                .iter()
                .filter_map(|id| index.get(id.as_str()).map(|&n| rects[n]))
                .collect();
            let first = *members.first()?;
            let bounds = members.into_iter().fold(first, Rect::union);
            let (title_w, _) = text_size(&sub.title, measure);
            let width = bounds.w + 2.0 * GROUP_PAD;
            let rect = Rect::new(
                bounds.x - GROUP_PAD - (title_w + 2.0 * PAD_X - width).max(0.0) / 2.0,
                bounds.y - GROUP_PAD - title_h,
                width.max(title_w + 2.0 * PAD_X),
                bounds.h + 2.0 * GROUP_PAD + title_h,
            );
            Some(GroupGeom {
                id: sub.id.clone(),
                title: sub.title.clone(),
                rect,
            })
        })
        .collect();

    let edges: Vec<EdgeGeom> = chart
        .edges
        .iter()
        .filter_map(|edge| {
            let from = *index.get(edge.from.as_str())?;
            let to = *index.get(edge.to.as_str())?;
            let points = if from == to {
                self_loop(rects[from])
            } else {
                vec![clip(rects[from], centers[to]), clip(rects[to], centers[from])]
            };
            let label = edge.label.as_ref().map(|text| {
                let a = points[0];
                let b = points[points.len() - 1];
                let mid = Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
                (text.clone(), mid)
            });
            Some(EdgeGeom {
                from: edge.from.clone(),
                to: edge.to.clone(),
                points,
                label,
                style: edge.style,
                tail: edge.tail,
                head: edge.head,
            })
        })
        .collect();

    let mut boxes: Vec<BoxGeom> = chart
        .nodes
        .iter()
        .zip(&rects)
        .map(|(node, rect)| BoxGeom {
            id: node.id.clone(),
            label: node.label.clone(),
            shape: node.shape,
            rect: *rect,
            class: node.class.clone(),
        })
        .collect();
    let mut layout = FlowLayout {
        boxes: std::mem::take(&mut boxes),
        edges,
        groups,
        ..FlowLayout::default()
    };
    normalize(&mut layout);
    layout
}

fn node_size(label: &str, shape: Shape, measure: &dyn Measure) -> (f32, f32) {
    let (text_w, text_h) = text_size(label, measure);
    let w = (text_w + 2.0 * PAD_X).max(MIN_WIDTH);
    let h = text_h + 2.0 * PAD_Y;
    match shape {
        Shape::Circle => {
            let side = w.max(h) * 1.2;
            (side, side)
        }
        Shape::Diamond | Shape::Hexagon => (w * 1.4, h * 1.6),
        _ => (w, h),
    }
}

fn group_of(chart: &Flowchart, index: &HashMap<&str, usize>) -> Vec<Option<usize>> {
    let mut group = vec![None; chart.nodes.len()];
    for (g, sub) in chart.subgraphs.iter().enumerate() {
        for id in &sub.nodes {
            if let Some(&n) = index.get(id.as_str()) {
                group[n].get_or_insert(g);
            }
        }
    }
    group
}

/// Edges that close a cycle are turned around. Self loops are dropped.
fn break_cycles(count: usize, links: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let mut out: Vec<Vec<usize>> = vec![Vec::new(); count];
    for &(a, b) in links {
        if a != b {
            out[a].push(b);
        }
    }
    let mut state = vec![0u8; count];
    let mut back: Vec<(usize, usize)> = Vec::new();
    fn visit(n: usize, out: &[Vec<usize>], state: &mut [u8], back: &mut Vec<(usize, usize)>) {
        state[n] = 1;
        for &m in &out[n] {
            match state[m] {
                0 => visit(m, out, state, back),
                1 => back.push((n, m)),
                _ => {}
            }
        }
        state[n] = 2;
    }
    for n in 0..count {
        if state[n] == 0 {
            visit(n, &out, &mut state, &mut back);
        }
    }
    let mut dag = Vec::new();
    let mut flipped = back;
    for &(a, b) in links {
        if a == b {
            continue;
        }
        if let Some(at) = flipped.iter().position(|&e| e == (a, b)) {
            flipped.remove(at);
            dag.push((b, a));
        } else {
            dag.push((a, b));
        }
    }
    dag
}

fn longest_path_ranks(count: usize, dag: &[(usize, usize)]) -> Vec<usize> {
    let mut indegree = vec![0usize; count];
    let mut out: Vec<Vec<usize>> = vec![Vec::new(); count];
    for &(a, b) in dag {
        out[a].push(b);
        indegree[b] += 1;
    }
    let mut queue: VecDeque<usize> = (0..count).filter(|&n| indegree[n] == 0).collect();
    let mut rank = vec![0usize; count];
    while let Some(n) = queue.pop_front() {
        for &m in &out[n] {
            rank[m] = rank[m].max(rank[n] + 1);
            indegree[m] -= 1;
            if indegree[m] == 0 {
                queue.push_back(m);
            }
        }
    }
    rank
}

fn order_layers(count: usize, rank: &[usize], dag: &[(usize, usize)], group: &[Option<usize>]) -> Vec<Vec<usize>> {
    let depth = rank.iter().copied().max().map_or(0, |m| m + 1);
    let mut layers: Vec<Vec<usize>> = vec![Vec::new(); depth];
    for n in 0..count {
        layers[rank[n]].push(n);
    }
    let mut preds: Vec<Vec<usize>> = vec![Vec::new(); count];
    let mut succs: Vec<Vec<usize>> = vec![Vec::new(); count];
    for &(a, b) in dag {
        succs[a].push(b);
        preds[b].push(a);
    }
    let mut pos = vec![0usize; count];
    let refresh = |layers: &[Vec<usize>], pos: &mut [usize]| {
        for layer in layers {
            for (i, &n) in layer.iter().enumerate() {
                pos[n] = i;
            }
        }
    };
    refresh(&layers, &mut pos);
    for _ in 0..SWEEPS {
        for l in 1..depth {
            sort_by_neighbors(&mut layers[l], &preds, l - 1, rank, &pos);
            refresh(&layers, &mut pos);
        }
        for l in (0..depth.saturating_sub(1)).rev() {
            sort_by_neighbors(&mut layers[l], &succs, l + 1, rank, &pos);
            refresh(&layers, &mut pos);
        }
    }
    for layer in &mut layers {
        *layer = cluster_groups(layer, group);
    }
    layers
}

fn sort_by_neighbors(layer: &mut [usize], neighbors: &[Vec<usize>], adjacent: usize, rank: &[usize], pos: &[usize]) {
    let bary = |n: usize| {
        let near: Vec<usize> = neighbors[n].iter().copied().filter(|&m| rank[m] == adjacent).collect();
        if near.is_empty() {
            pos[n] as f32
        } else {
            near.iter().map(|&m| pos[m] as f32).sum::<f32>() / near.len() as f32
        }
    };
    let keys: HashMap<usize, f32> = layer.iter().map(|&n| (n, bary(n))).collect();
    layer.sort_by(|a, b| keys[a].total_cmp(&keys[b]));
}

fn cluster_groups(layer: &[usize], group: &[Option<usize>]) -> Vec<usize> {
    let mut order: Vec<Option<usize>> = Vec::new();
    for &n in layer {
        if group[n].is_none() || !order.contains(&group[n]) {
            order.push(group[n]);
        }
    }
    let mut out = Vec::new();
    let mut placed_ungrouped = 0;
    for key in order {
        match key {
            Some(_) => out.extend(layer.iter().copied().filter(|&n| group[n] == key)),
            None => {
                let next = layer
                    .iter()
                    .copied()
                    .filter(|&n| group[n].is_none())
                    .nth(placed_ungrouped);
                out.extend(next);
                placed_ungrouped += 1;
            }
        }
    }
    out
}

fn clip(rect: Rect, toward: Point) -> Point {
    let c = rect.center();
    let (dx, dy) = (toward.x - c.x, toward.y - c.y);
    if dx == 0.0 && dy == 0.0 {
        return c;
    }
    let tx = if dx == 0.0 { f32::MAX } else { (rect.w / 2.0) / dx.abs() };
    let ty = if dy == 0.0 { f32::MAX } else { (rect.h / 2.0) / dy.abs() };
    let t = tx.min(ty);
    Point::new(c.x + dx * t, c.y + dy * t)
}

fn self_loop(rect: Rect) -> Vec<Point> {
    let c = rect.center();
    let dy = rect.h / 4.0;
    let right = rect.right();
    vec![
        Point::new(right, c.y - dy),
        Point::new(right + LOOP_OUT, c.y - dy),
        Point::new(right + LOOP_OUT, c.y + dy),
        Point::new(right, c.y + dy),
    ]
}

fn normalize(layout: &mut FlowLayout) {
    let mut bounds: Option<Rect> = None;
    let mut grow = |r: Rect| bounds = Some(bounds.map_or(r, |b| b.union(r)));
    for b in &layout.boxes {
        grow(b.rect);
    }
    for g in &layout.groups {
        grow(g.rect);
    }
    for e in &layout.edges {
        for p in &e.points {
            grow(Rect::new(p.x, p.y, 0.0, 0.0));
        }
    }
    let Some(bounds) = bounds else { return };
    let (dx, dy) = (MARGIN - bounds.x, MARGIN - bounds.y);
    for b in &mut layout.boxes {
        b.rect = b.rect.shifted(dx, dy);
    }
    for g in &mut layout.groups {
        g.rect = g.rect.shifted(dx, dy);
    }
    for e in &mut layout.edges {
        for p in &mut e.points {
            *p = p.shifted(dx, dy);
        }
        if let Some((_, at)) = &mut e.label {
            *at = at.shifted(dx, dy);
        }
    }
    layout.width = bounds.w + 2.0 * MARGIN;
    layout.height = bounds.h + 2.0 * MARGIN;
}

#[cfg(test)]
mod tests {
    use super::super::{FakeMeasure, Layout, layout};
    use super::*;
    use crate::guide::mermaid::{Graph, parse};

    fn flow(source: &str) -> FlowLayout {
        let graph = parse(source).unwrap();
        match layout(&graph, &FakeMeasure) {
            Some(Layout::Flow(flow)) => flow,
            other => panic!("not a flow layout: {other:?}"),
        }
    }

    fn center(flow: &FlowLayout, id: &str) -> Point {
        flow.node(id).unwrap().rect.center()
    }

    #[test]
    fn a_top_down_chain_stacks_and_a_left_right_chain_runs_sideways() {
        let td = flow("flowchart TD\n A --> B\n B --> C");
        let (a, b, c) = (center(&td, "A"), center(&td, "B"), center(&td, "C"));
        assert!(a.y < b.y && b.y < c.y);
        assert!((a.x - b.x).abs() < 0.01 && (b.x - c.x).abs() < 0.01);

        let lr = flow("flowchart LR\n A --> B\n B --> C");
        let (a, b, c) = (center(&lr, "A"), center(&lr, "B"), center(&lr, "C"));
        assert!(a.x < b.x && b.x < c.x);
        assert!((a.y - b.y).abs() < 0.01);
    }

    #[test]
    fn bottom_up_and_right_left_reverse_the_flow() {
        let bt = flow("flowchart BT\n A --> B");
        assert!(center(&bt, "A").y > center(&bt, "B").y);
        let rl = flow("flowchart RL\n A --> B");
        assert!(center(&rl, "A").x > center(&rl, "B").x);
    }

    const BUSY: &str = "flowchart TD\n A --> B\n A --> C\n B --> D\n C --> D\n D --> E\n A --> E\n F[Island]";

    #[test]
    fn no_two_boxes_overlap() {
        let flow = flow(BUSY);
        for (i, a) in flow.boxes.iter().enumerate() {
            for b in &flow.boxes[i + 1..] {
                assert!(!a.rect.intersects(&b.rect), "{} overlaps {}", a.id, b.id);
            }
        }
    }

    #[test]
    fn every_edge_starts_on_its_source_and_ends_on_its_target() {
        for source in [BUSY, "flowchart LR\n A --> A\n A --> B\n B --> A"] {
            let flow = flow(source);
            for edge in &flow.edges {
                let from = flow.node(&edge.from).unwrap().rect;
                let to = flow.node(&edge.to).unwrap().rect;
                assert!(from.on_border(edge.points[0]), "{} start", edge.from);
                assert!(to.on_border(*edge.points.last().unwrap()), "{} end", edge.to);
            }
        }
    }

    #[test]
    fn a_cycle_still_places_every_node() {
        let flow = flow("flowchart TD\n A --> B\n B --> C\n C --> A\n C --> C");
        assert_eq!(flow.boxes.len(), 3);
        assert_eq!(flow.edges.len(), 4);
        let (a, b, c) = (center(&flow, "A"), center(&flow, "B"), center(&flow, "C"));
        assert!(a.y < b.y && b.y < c.y);
        assert!(flow.width > 0.0 && flow.height > 0.0);
    }

    #[test]
    fn a_subgraph_rect_holds_its_members() {
        let flow = flow("flowchart TD\n subgraph S[Stage]\n A --> B\n end\n X --> A\n B --> Y");
        let group = &flow.groups[0];
        assert_eq!(group.title, "Stage");
        for id in ["A", "B"] {
            assert!(group.rect.contains(&flow.node(id).unwrap().rect), "{id}");
        }
        for id in ["X", "Y"] {
            assert!(!group.rect.intersects(&flow.node(id).unwrap().rect), "{id}");
        }
    }

    #[test]
    fn a_box_grows_with_its_label_and_keeps_its_class() {
        let flow = flow("flowchart TD\n A[Hi] --> B[A much longer label]:::added");
        let short = flow.node("A").unwrap();
        let long = flow.node("B").unwrap();
        assert!(long.rect.w > short.rect.w);
        assert_eq!(long.class.as_deref(), Some("added"));
        assert_eq!(short.class, None);
    }

    #[test]
    fn an_edge_is_found_in_either_direction() {
        let flow = flow("flowchart LR\n A --> B");
        assert!(flow.edge("A", "B").is_some());
        assert!(flow.edge("B", "A").is_none());
        assert_eq!(flow.edge_between("B", "A"), flow.edge("A", "B"));
        assert!(flow.edge_between("A", "Z").is_none());
    }

    #[test]
    fn source_only_diagrams_have_no_layout() {
        assert_eq!(layout(&Graph::Source, &FakeMeasure), None);
    }
}
