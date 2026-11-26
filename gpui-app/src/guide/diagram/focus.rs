#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Focus {
    pub node: String,
    pub edge: Option<(String, String)>,
    pub visited: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeState {
    Normal,
    Active,
    Dim,
}

impl Focus {
    pub fn node_state(&self, id: &str, visited: bool) -> NodeState {
        if id == self.node {
            NodeState::Active
        } else if visited {
            NodeState::Normal
        } else {
            NodeState::Dim
        }
    }

    pub fn has_visited(&self, id: &str) -> bool {
        self.visited.iter().any(|seen| seen == id)
    }
}

/// Port of diffity's `flowFocus`. `nodes` are the node ids of the walkthrough steps in order.
pub fn flow_focus(nodes: &[String], active: Option<usize>) -> Option<Focus> {
    let active = active?;
    let node = nodes.get(active)?;
    let previous = active
        .checked_sub(1)
        .map(|at| &nodes[at])
        .filter(|previous| *previous != node);
    Some(Focus {
        node: node.clone(),
        edge: previous.map(|previous| (previous.clone(), node.clone())),
        visited: nodes[..active].to_vec(),
    })
}

pub fn next_step(step: Option<usize>, total: usize) -> usize {
    step.map_or(0, |at| at + 1).min(total.saturating_sub(1))
}

pub fn previous_step(step: Option<usize>) -> Option<usize> {
    step.and_then(|at| at.checked_sub(1))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|id| (*id).to_string()).collect()
    }

    #[test]
    fn nothing_active_gives_no_focus() {
        assert_eq!(flow_focus(&ids(&["a", "b"]), None), None);
        assert_eq!(flow_focus(&ids(&["a", "b"]), Some(5)), None);
        assert_eq!(flow_focus(&[], Some(0)), None);
    }

    #[test]
    fn the_third_step_lights_the_edge_from_the_second_and_has_visited_the_first_two() {
        let focus = flow_focus(&ids(&["a", "b", "c"]), Some(2)).unwrap();
        assert_eq!(focus.node, "c");
        assert_eq!(focus.edge, Some(("b".into(), "c".into())));
        assert_eq!(focus.visited, ids(&["a", "b"]));
    }

    #[test]
    fn the_first_step_has_no_edge_and_nothing_visited() {
        let focus = flow_focus(&ids(&["a", "b"]), Some(0)).unwrap();
        assert_eq!(focus.edge, None);
        assert!(focus.visited.is_empty());
    }

    #[test]
    fn a_repeated_node_lights_no_edge() {
        let focus = flow_focus(&ids(&["a", "a"]), Some(1)).unwrap();
        assert_eq!(focus.edge, None);
    }

    #[test]
    fn nodes_are_active_visited_or_dim() {
        let focus = flow_focus(&ids(&["a", "b", "c"]), Some(1)).unwrap();
        assert_eq!(focus.node_state("b", false), NodeState::Active);
        assert_eq!(focus.node_state("a", focus.has_visited("a")), NodeState::Normal);
        assert_eq!(focus.node_state("c", focus.has_visited("c")), NodeState::Dim);
    }

    #[test]
    fn stepping_starts_at_the_first_node_stops_at_the_last_and_goes_back_to_none() {
        assert_eq!(next_step(None, 3), 0);
        assert_eq!(next_step(Some(1), 3), 2);
        assert_eq!(next_step(Some(2), 3), 2);
        assert_eq!(previous_step(Some(2)), Some(1));
        assert_eq!(previous_step(Some(0)), None);
        assert_eq!(previous_step(None), None);
    }

    #[test]
    fn a_wide_diagram_shrinks_to_the_card_and_a_small_one_keeps_its_size() {
        let wide = fit((800.0, 400.0), 400.0);
        assert_eq!((wide.scale, wide.width, wide.height), (0.5, 400.0, 200.0));
        let small = fit((200.0, 100.0), 400.0);
        assert_eq!((small.scale, small.width, small.height), (1.0, 200.0, 100.0));
        assert_eq!(fit((0.0, 0.0), 400.0).scale, 1.0);
    }
}
