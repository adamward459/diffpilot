use crate::guide::diagram::focus::NodeMark;
use crate::guide::stops::Stops;
use diffpilot_core::guide::mermaid::Graph;
use diffpilot_core::guide::{Diagram, Note, StopKind};

/// The first sentence of a stop's text, as the stop's title.
pub fn title(text: &str) -> String {
    let text = text.trim();
    let end = text
        .char_indices()
        .find(|&(at, mark)| {
            matches!(mark, '.' | '!' | '?')
                && text[at + mark.len_utf8()..]
                    .chars()
                    .next()
                    .is_none_or(char::is_whitespace)
        })
        .map_or(text.len(), |(at, _)| at);
    text[..end].replace('`', "")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Counts {
    pub read: usize,
    pub check: usize,
    pub skim: usize,
    pub critical: usize,
    pub linked: usize,
}

impl Counts {
    pub fn of(notes: &[Note], stops: &Stops) -> Self {
        let mut counts = Self::default();
        for note in stops.iter().filter_map(|index| notes.get(index)) {
            match note.kind {
                StopKind::Read => counts.read += 1,
                StopKind::Check => counts.check += 1,
                StopKind::Skim => counts.skim += 1,
            }
            counts.critical += usize::from(note.critical);
            counts.linked += usize::from(note.is_linked());
        }
        counts
    }

    pub fn add(&mut self, other: Self) {
        self.read += other.read;
        self.check += other.check;
        self.skim += other.skim;
        self.critical += other.critical;
        self.linked += other.linked;
    }

    pub fn total(&self) -> usize {
        self.read + self.check + self.skim
    }

    fn kinds(&self) -> Vec<String> {
        [
            (self.read, "to read closely"),
            (self.check, "to check"),
            (self.skim, "to skim"),
        ]
        .into_iter()
        .filter(|(count, _)| *count > 0)
        .map(|(count, what)| format!("{count} {what}"))
        .collect()
    }

    fn extras(&self) -> Vec<String> {
        let mut extras = Vec::new();
        match self.critical {
            0 => {}
            1 => extras.push("1 needs extra care".to_string()),
            count => extras.push(format!("{count} need extra care")),
        }
        if self.linked > 0 {
            extras.push(format!("{} in a linked folder", self.linked));
        }
        extras
    }

    /// For the overview banner: "8 to read closely, 3 to skim. 1 needs extra care, 1 in a linked folder."
    pub fn sentence(&self) -> String {
        let mut text = self.kinds().join(", ");
        if !text.is_empty() {
            text.push('.');
        }
        let extras = self.extras();
        if !extras.is_empty() {
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(&extras.join(", "));
            text.push('.');
        }
        text
    }

    /// For a chapter row: "4 stops · 2 to read closely · 1 needs extra care".
    pub fn line(&self) -> String {
        let total = self.total();
        let head = match total {
            1 => "1 stop".to_string(),
            count => format!("{count} stops"),
        };
        std::iter::once(head)
            .chain(self.kinds())
            .chain(self.extras())
            .collect::<Vec<_>>()
            .join(" \u{b7} ")
    }
}

pub fn stop_count(count: usize) -> String {
    match count {
        1 => "1 stop".into(),
        count => format!("{count} stops"),
    }
}

fn flow_ids(diagram: &Diagram) -> Vec<String> {
    match diagram.graph() {
        Ok(Graph::Flowchart(chart)) => chart.nodes.into_iter().map(|node| node.id).collect(),
        _ => Vec::new(),
    }
}

/// The first flowchart of the chapter that a stop names a node of. That diagram is the chapter map.
pub fn map_diagram(diagrams: &[Diagram], notes: &[Note], stops: &Stops) -> Option<usize> {
    diagrams.iter().position(|diagram| {
        let ids = flow_ids(diagram);
        stops
            .iter()
            .filter_map(|index| notes.get(index)?.node.as_deref())
            .any(|node| ids.iter().any(|id| id == node))
    })
}

/// One mark per node that has stops, with how many of them are reviewed.
pub fn node_marks(notes: &[Note], stops: &Stops, reviewed: impl Fn(usize) -> bool) -> Vec<NodeMark> {
    let mut marks: Vec<NodeMark> = Vec::new();
    for index in stops.iter() {
        let Some(note) = notes.get(index) else { continue };
        let Some(node) = note.node.as_deref() else { continue };
        let at = match marks.iter().position(|mark| mark.id == node) {
            Some(at) => at,
            None => {
                marks.push(NodeMark {
                    id: node.to_string(),
                    reviewed: 0,
                    total: 0,
                    linked: true,
                });
                marks.len() - 1
            }
        };
        let mark = &mut marks[at];
        mark.total += 1;
        mark.reviewed += usize::from(reviewed(index));
        mark.linked &= note.is_linked();
    }
    marks
}

/// Each node that has stops, with the note index of its first stop.
pub fn node_targets(notes: &[Note], stops: &Stops) -> Vec<(String, usize)> {
    let mut targets: Vec<(String, usize)> = Vec::new();
    for index in stops.iter() {
        if let Some(node) = notes.get(index).and_then(|note| note.node.as_deref())
            && !targets.iter().any(|(seen, _)| seen == node)
        {
            targets.push((node.to_string(), index));
        }
    }
    targets
}

/// The chapter's files that no stop points at.
pub fn off_path(files: &[String], notes: &[Note], stops: &Stops) -> Vec<String> {
    files
        .iter()
        .filter(|file| {
            !stops
                .iter()
                .filter_map(|index| notes.get(index))
                .any(|note| !note.is_linked() && &note.path == *file)
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn note(path: &str, kind: StopKind, critical: bool, node: Option<&str>, folder: Option<&str>) -> Note {
        Note {
            path: path.into(),
            span: None,
            text: String::new(),
            critical,
            kind,
            check: None,
            node: node.map(str::to_owned),
            folder: folder.map(PathBuf::from),
        }
    }

    fn notes() -> Vec<Note> {
        vec![
            note("a.rs", StopKind::Read, false, Some("A"), None),
            note("b.rs", StopKind::Check, true, Some("B"), None),
            note("a.rs", StopKind::Skim, false, Some("A"), None),
            note("shared.rs", StopKind::Check, false, Some("C"), Some("/lib")),
        ]
    }

    fn stops(notes: &[Note]) -> Stops {
        Stops::of(notes, &["a.rs".into(), "b.rs".into(), "c.rs".into()])
    }

    #[test]
    fn the_title_is_the_first_sentence_without_its_stop_or_code_ticks() {
        assert_eq!(title("Why `x` is cached. Then more."), "Why x is cached");
        assert_eq!(title("Is it safe? Check the lock."), "Is it safe");
        assert_eq!(title("No stop here"), "No stop here");
        assert_eq!(title("Version 1.2 is read. Next."), "Version 1.2 is read");
        assert_eq!(title("  Spaces around.  "), "Spaces around");
        assert_eq!(title(""), "");
    }

    #[test]
    fn counts_split_by_kind_and_count_risky_and_linked_stops() {
        let notes = notes();
        let counts = Counts::of(&notes, &stops(&notes));
        assert_eq!(
            counts,
            Counts {
                read: 1,
                check: 2,
                skim: 1,
                critical: 1,
                linked: 1
            }
        );
        assert_eq!(
            counts.sentence(),
            "1 to read closely, 2 to check, 1 to skim. 1 needs extra care, 1 in a linked folder."
        );
        assert_eq!(
            counts.line(),
            "4 stops \u{b7} 1 to read closely \u{b7} 2 to check \u{b7} 1 to skim \u{b7} 1 needs extra care \u{b7} 1 in a linked folder"
        );
        let mut all = Counts::default();
        all.add(counts);
        all.add(counts);
        assert_eq!((all.total(), all.critical), (8, 2));
        assert!(all.sentence().contains("2 need extra care"));
    }

    #[test]
    fn the_map_is_the_first_flowchart_that_a_stop_names_a_node_of() {
        let notes = notes();
        let stops = stops(&notes);
        let diagram = |source: &str| Diagram {
            title: None,
            kind: diffpilot_core::guide::DiagramKind::Flowchart,
            source: source.into(),
        };
        let diagrams = [
            diagram("flowchart LR\n  X --> Y"),
            diagram("flowchart LR\n  A --> B"),
            diagram("flowchart LR\n  B --> C"),
        ];
        assert_eq!(map_diagram(&diagrams, &notes, &stops), Some(1));
        assert_eq!(map_diagram(&diagrams[..1], &notes, &stops), None);
        let sequence = Diagram {
            title: None,
            kind: diffpilot_core::guide::DiagramKind::Sequence,
            source: "sequenceDiagram\n  A->>B: hi".into(),
        };
        assert_eq!(map_diagram(&[sequence], &notes, &stops), None);
    }

    #[test]
    fn a_node_is_marked_with_its_reviewed_and_total_stops_and_is_linked_only_when_all_its_stops_are() {
        let notes = notes();
        let stops = stops(&notes);
        let marks = node_marks(&notes, &stops, |index| index == 0);
        let shown: Vec<_> = marks
            .iter()
            .map(|mark| (mark.id.as_str(), mark.reviewed, mark.total, mark.linked))
            .collect();
        assert_eq!(shown, [("A", 1, 2, false), ("B", 0, 1, false), ("C", 0, 1, true)]);
        assert_eq!(
            node_targets(&notes, &stops),
            [("A".to_string(), 0), ("B".to_string(), 1), ("C".to_string(), 3)]
        );
    }

    #[test]
    fn files_that_no_stop_points_at_are_off_the_path_and_a_linked_stop_covers_no_file() {
        let notes = notes();
        let stops = stops(&notes);
        let files: Vec<String> = ["a.rs", "b.rs", "c.rs", "shared.rs"].map(String::from).to_vec();
        assert_eq!(off_path(&files, &notes, &stops), ["c.rs", "shared.rs"]);
    }
}
