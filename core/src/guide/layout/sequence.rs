use super::{BlockGeom, Measure, MessageGeom, NoteGeom, ParticipantGeom, Point, Rect, SequenceLayout, text_size};
use crate::guide::mermaid::{NotePlace, Sequence, Step};
use std::collections::HashMap;

const PAD_X: f32 = 14.0;
const PAD_Y: f32 = 8.0;
const COLUMN_GAP: f32 = 40.0;
const ROW_GAP: f32 = 16.0;
const LOOP_OUT: f32 = 32.0;
const LOOP_DROP: f32 = 20.0;
const BLOCK_INSET: f32 = 8.0;
const MARGIN: f32 = 16.0;

pub fn sequence_layout(seq: &Sequence, measure: &dyn Measure) -> SequenceLayout {
    let lh = measure.line_height();
    let index: HashMap<&str, usize> = seq
        .participants
        .iter()
        .enumerate()
        .map(|(i, p)| (p.id.as_str(), i))
        .collect();
    let head_sizes: Vec<(f32, f32)> = seq
        .participants
        .iter()
        .map(|p| {
            let (w, h) = text_size(&p.label, measure);
            (w + 2.0 * PAD_X, h + 2.0 * PAD_Y)
        })
        .collect();
    let head_h = head_sizes.iter().map(|s| s.1).fold(0.0, f32::max);

    let mut gaps: Vec<f32> = (0..seq.participants.len().saturating_sub(1))
        .map(|i| (head_sizes[i].0 + head_sizes[i + 1].0) / 2.0 + COLUMN_GAP / 2.0)
        .collect();
    let mut spans: Vec<(usize, usize, f32)> = Vec::new();
    for message in seq.messages() {
        let (Some(&a), Some(&b)) = (index.get(message.from.as_str()), index.get(message.to.as_str())) else {
            continue;
        };
        let (text_w, _) = text_size(&message.text, measure);
        if a == b {
            if a < gaps.len() {
                spans.push((a, a + 1, text_w + LOOP_OUT + COLUMN_GAP));
            }
        } else {
            spans.push((a.min(b), a.max(b), text_w + COLUMN_GAP));
        }
    }
    spans.sort_by_key(|&(from, to, _)| to - from);
    for (from, to, need) in spans {
        let have: f32 = gaps[from..to].iter().sum();
        if have < need {
            gaps[to - 1] += need - have;
        }
    }
    let mut xs = Vec::new();
    let mut x = head_sizes.first().map_or(0.0, |s| s.0 / 2.0);
    for i in 0..seq.participants.len() {
        xs.push(x);
        if i < gaps.len() {
            x += gaps[i];
        }
    }
    let left = xs.first().map_or(0.0, |x| x - head_sizes[0].0 / 2.0);
    let right = xs
        .iter()
        .zip(&head_sizes)
        .map(|(x, s)| x + s.0 / 2.0)
        .fold(0.0, f32::max);

    let mut out = SequenceLayout::default();
    let mut y = head_h + ROW_GAP;
    let mut open: Vec<(usize, f32)> = Vec::new();
    for step in &seq.steps {
        match step {
            Step::Message(message) => {
                let (Some(&a), Some(&b)) = (index.get(message.from.as_str()), index.get(message.to.as_str())) else {
                    continue;
                };
                let (text_w, _) = text_size(&message.text, measure);
                let line_y = y + lh + 4.0;
                let (points, label_x) = if a == b {
                    let x = xs[a];
                    let points = vec![
                        Point::new(x, line_y),
                        Point::new(x + LOOP_OUT, line_y),
                        Point::new(x + LOOP_OUT, line_y + LOOP_DROP),
                        Point::new(x, line_y + LOOP_DROP),
                    ];
                    (points, x + 8.0 + text_w / 2.0)
                } else {
                    let points = vec![Point::new(xs[a], line_y), Point::new(xs[b], line_y)];
                    (points, (xs[a] + xs[b]) / 2.0)
                };
                out.messages.push(MessageGeom {
                    from: message.from.clone(),
                    to: message.to.clone(),
                    text: message.text.clone(),
                    arrow: message.arrow,
                    points,
                    label: Point::new(label_x, y + lh / 2.0),
                });
                y = line_y + if a == b { LOOP_DROP } else { 0.0 } + ROW_GAP;
            }
            Step::Note {
                place,
                participants,
                text,
            } => {
                let cols: Vec<f32> = participants
                    .iter()
                    .filter_map(|id| index.get(id.as_str()).map(|&i| xs[i]))
                    .collect();
                let Some(&first) = cols.first() else { continue };
                let (text_w, text_h) = text_size(text, measure);
                let w = text_w + 2.0 * PAD_X;
                let h = text_h + 2.0 * PAD_Y;
                let last = cols.last().copied().unwrap_or(first);
                let rect = match place {
                    NotePlace::LeftOf => Rect::new(first - 10.0 - w, y, w, h),
                    NotePlace::RightOf => Rect::new(first + 10.0, y, w, h),
                    NotePlace::Over => {
                        let span = (last - first).abs().max(w);
                        Rect::new((first + last) / 2.0 - span / 2.0, y, span, h)
                    }
                };
                out.notes.push(NoteGeom {
                    text: text.clone(),
                    rect,
                });
                y += h + ROW_GAP;
            }
            Step::BlockStart { kind, label } => {
                open.push((out.blocks.len(), y));
                out.blocks.push(BlockGeom {
                    kind: *kind,
                    label: label.clone(),
                    rect: Rect::default(),
                    branches: Vec::new(),
                });
                y += lh + PAD_Y;
            }
            Step::BlockBranch { label } => {
                if let Some(&(at, _)) = open.last() {
                    out.blocks[at].branches.push((y, label.clone()));
                    y += lh + PAD_Y;
                }
            }
            Step::BlockEnd => {
                if let Some((at, top)) = open.pop() {
                    let inset = open.len() as f32 * BLOCK_INSET;
                    out.blocks[at].rect = Rect::new(
                        left - MARGIN / 2.0 + inset,
                        top,
                        right - left + MARGIN - 2.0 * inset,
                        y - top,
                    );
                    y += ROW_GAP / 2.0;
                }
            }
            Step::Activate(_) | Step::Deactivate(_) => {}
        }
    }
    while let Some((at, top)) = open.pop() {
        let inset = open.len() as f32 * BLOCK_INSET;
        out.blocks[at].rect = Rect::new(
            left - MARGIN / 2.0 + inset,
            top,
            right - left + MARGIN - 2.0 * inset,
            y - top,
        );
    }

    for (i, p) in seq.participants.iter().enumerate() {
        let (w, h) = head_sizes[i];
        out.participants.push(ParticipantGeom {
            id: p.id.clone(),
            label: p.label.clone(),
            kind: p.kind,
            rect: Rect::new(xs[i] - w / 2.0, 0.0, w, h),
            lifeline: (Point::new(xs[i], h), Point::new(xs[i], y)),
        });
    }
    out.height = y;
    out.width = right;
    normalize(&mut out);
    out
}

fn normalize(layout: &mut SequenceLayout) {
    let mut min_x = 0.0f32;
    let mut max_x = layout.width;
    let mut see = |r: Rect| {
        min_x = min_x.min(r.x);
        max_x = max_x.max(r.right());
    };
    for n in &layout.notes {
        see(n.rect);
    }
    for b in &layout.blocks {
        see(b.rect);
    }
    for m in &layout.messages {
        for p in &m.points {
            see(Rect::new(p.x, p.y, 0.0, 0.0));
        }
        see(Rect::new(m.label.x, m.label.y, 0.0, 0.0));
    }
    let (dx, dy) = (MARGIN - min_x, MARGIN);
    for p in &mut layout.participants {
        p.rect = p.rect.shifted(dx, dy);
        p.lifeline = (p.lifeline.0.shifted(dx, dy), p.lifeline.1.shifted(dx, dy));
    }
    for m in &mut layout.messages {
        for p in &mut m.points {
            *p = p.shifted(dx, dy);
        }
        m.label = m.label.shifted(dx, dy);
    }
    for n in &mut layout.notes {
        n.rect = n.rect.shifted(dx, dy);
    }
    for b in &mut layout.blocks {
        b.rect = b.rect.shifted(dx, dy);
        for (y, _) in &mut b.branches {
            *y += dy;
        }
    }
    layout.width = max_x - min_x + 2.0 * MARGIN;
    layout.height += 2.0 * MARGIN;
}

#[cfg(test)]
mod tests {
    use super::super::{FakeMeasure, Layout, layout};
    use super::*;
    use crate::guide::mermaid::parse;

    fn seq(source: &str) -> SequenceLayout {
        match layout(&parse(source).unwrap(), &FakeMeasure) {
            Some(Layout::Sequence(seq)) => seq,
            other => panic!("not a sequence layout: {other:?}"),
        }
    }

    const SOURCE: &str = "sequenceDiagram\n participant A as Client\n participant B as Server\n A->>B: request\n B->>B: check\n Note over A,B: hello there\n loop retry\n B-->>A: reply\n end";

    fn y_of(message: &MessageGeom) -> f32 {
        message.points[0].y
    }

    #[test]
    fn messages_keep_source_order_and_move_down() {
        let seq = seq(SOURCE);
        let texts: Vec<&str> = seq.messages.iter().map(|m| m.text.as_str()).collect();
        assert_eq!(texts, ["request", "check", "reply"]);
        assert!(y_of(&seq.messages[0]) < y_of(&seq.messages[1]));
        assert!(y_of(&seq.messages[1]) < y_of(&seq.messages[2]));
    }

    #[test]
    fn a_message_runs_between_its_lifelines() {
        let seq = seq(SOURCE);
        let x_of = |id: &str| seq.participants.iter().find(|p| p.id == id).unwrap().lifeline.0.x;
        let request = &seq.messages[0];
        assert_eq!(request.points[0].x, x_of("A"));
        assert_eq!(request.points[1].x, x_of("B"));
        let reply = &seq.messages[2];
        assert_eq!(reply.points[0].x, x_of("B"));
        assert_eq!(reply.points[1].x, x_of("A"));
        assert!(x_of("A") < x_of("B"));
    }

    #[test]
    fn a_self_message_loops_back_to_its_participant() {
        let seq = seq(SOURCE);
        let x = seq.participants[1].lifeline.0.x;
        let points = &seq.messages[1].points;
        assert_eq!(points.len(), 4);
        assert_eq!(points[0].x, x);
        assert_eq!(points[3].x, x);
        assert!(points[1].x > x && points[3].y > points[0].y);
    }

    #[test]
    fn lifelines_cover_every_message_and_notes_and_blocks_are_placed() {
        let seq = seq(SOURCE);
        for p in &seq.participants {
            assert_eq!(p.lifeline.0.y, p.rect.bottom());
            for m in &seq.messages {
                assert!(p.lifeline.1.y >= m.points.last().unwrap().y);
            }
        }
        assert_eq!(seq.notes.len(), 1);
        assert_eq!(seq.blocks.len(), 1);
        let block = &seq.blocks[0];
        assert!(block.rect.h > 0.0);
        assert!(block.rect.y <= y_of(&seq.messages[2]) && block.rect.bottom() >= y_of(&seq.messages[2]));
        assert!(seq.notes[0].rect.bottom() <= block.rect.y);
    }

    #[test]
    fn long_message_text_pushes_the_participants_apart() {
        let near = seq("sequenceDiagram\n A->>B: hi");
        let far = seq("sequenceDiagram\n A->>B: a rather long message text here");
        let gap = |s: &SequenceLayout| s.participants[1].rect.x - s.participants[0].rect.x;
        assert!(gap(&far) > gap(&near));
    }

    #[test]
    fn nothing_sits_left_of_the_origin() {
        let seq = seq("sequenceDiagram\n A->>B: hi\n Note left of A: a wide note on the left");
        assert!(seq.notes[0].rect.x >= 0.0);
        assert!(seq.participants[0].rect.x >= 0.0);
    }
}
