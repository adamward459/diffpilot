#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeMark {
    pub id: String,
    pub reviewed: usize,
    pub total: usize,
    pub linked: bool,
}

impl NodeMark {
    pub fn done(&self) -> bool {
        self.total > 0 && self.reviewed == self.total
    }
}

/// What the lit chapter map shows: the node of the current stop and the review state of the nodes that have stops.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Focus {
    pub node: Option<String>,
    pub marks: Vec<NodeMark>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeState {
    Normal,
    Active,
    Done,
}

impl Focus {
    pub fn mark(&self, id: &str) -> Option<&NodeMark> {
        self.marks.iter().find(|mark| mark.id == id)
    }

    pub fn node_state(&self, id: &str) -> NodeState {
        if self.node.as_deref() == Some(id) {
            NodeState::Active
        } else if self.mark(id).is_some_and(NodeMark::done) {
            NodeState::Done
        } else {
            NodeState::Normal
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fit {
    pub scale: f32,
    pub width: f32,
    pub height: f32,
}

/// Shrinks a diagram to the card width. A diagram is never drawn larger than it was laid out.
pub fn fit(size: (f32, f32), available: f32) -> Fit {
    let (width, height) = size;
    let scale = if width <= 0.0 || available <= 0.0 {
        1.0
    } else {
        (available / width).min(1.0)
    };
    Fit {
        scale,
        width: width * scale,
        height: height * scale,
    }
}

/// A box as fractions of the whole diagram, so a click target can follow the diagram when it is scaled.
pub fn fractions(rect: (f32, f32, f32, f32), size: (f32, f32)) -> (f32, f32, f32, f32) {
    let (x, y, w, h) = rect;
    let (width, height) = size;
    if width <= 0.0 || height <= 0.0 {
        return (0.0, 0.0, 0.0, 0.0);
    }
    (x / width, y / height, w / width, h / height)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn focus() -> Focus {
        Focus {
            node: Some("b".into()),
            marks: vec![
                NodeMark {
                    id: "a".into(),
                    reviewed: 2,
                    total: 2,
                    linked: false,
                },
                NodeMark {
                    id: "b".into(),
                    reviewed: 1,
                    total: 1,
                    linked: false,
                },
                NodeMark {
                    id: "c".into(),
                    reviewed: 0,
                    total: 1,
                    linked: true,
                },
            ],
        }
    }

    #[test]
    fn the_current_node_is_active_even_when_it_is_done_and_a_reviewed_node_is_done() {
        let focus = focus();
        assert_eq!(focus.node_state("b"), NodeState::Active);
        assert_eq!(focus.node_state("a"), NodeState::Done);
        assert_eq!(focus.node_state("c"), NodeState::Normal);
        assert_eq!(focus.node_state("nowhere"), NodeState::Normal);
    }

    #[test]
    fn a_wide_diagram_shrinks_to_the_card_and_a_small_one_keeps_its_size() {
        let wide = fit((800.0, 400.0), 400.0);
        assert_eq!((wide.scale, wide.width, wide.height), (0.5, 400.0, 200.0));
        let small = fit((200.0, 100.0), 400.0);
        assert_eq!((small.scale, small.width, small.height), (1.0, 200.0, 100.0));
        assert_eq!(fit((0.0, 0.0), 400.0).scale, 1.0);
    }

    #[test]
    fn a_box_becomes_fractions_of_the_diagram() {
        assert_eq!(
            fractions((50.0, 20.0, 100.0, 40.0), (200.0, 80.0)),
            (0.25, 0.25, 0.5, 0.5)
        );
        assert_eq!(fractions((1.0, 1.0, 1.0, 1.0), (0.0, 0.0)), (0.0, 0.0, 0.0, 0.0));
    }
}
