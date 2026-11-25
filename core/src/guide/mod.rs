//! The guide: what Claude writes to walk a reviewer through a change. An overview, chapters in reading order,
//! notes pinned under diff lines, and Mermaid diagrams, tied to the diff they describe by a fingerprint.
//!
//! The shape follows diffity's `GuideContent` (MIT, see THIRD_PARTY_NOTICES). Chapters name the hunks they cover,
//! and notes and spans keep the text of their lines, so a guide can follow its code with `anchor` as the diff moves.

pub mod build;
#[cfg(test)]
mod fixtures;
pub mod mermaid;
pub mod stale;

pub use mermaid::{DiagramKind, Graph};
pub use stale::{Staleness, check, fingerprint};

use crate::threads::{Side, ThreadAnchor};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// How much of the reviewer's attention a chapter needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Attention {
    /// The core of the change. Read every line.
    High,
    #[default]
    Normal,
    /// Tests, generated files, lockfiles, config. Skim.
    Low,
}

/// Lines of a file on one side of the diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub side: Side,
    pub line: u32,
    pub end_line: Option<u32>,
    /// The text of the lines when the guide was written, so the span can follow them.
    pub anchor_text: Option<String>,
}

impl Span {
    pub fn last_line(&self) -> u32 {
        self.end_line.filter(|end| *end >= self.line).unwrap_or(self.line)
    }

    pub fn anchor(&self, path: &str) -> ThreadAnchor {
        ThreadAnchor::line(path, self.side, self.line)
    }
}

/// A note shown under a line of the diff. Without a span it belongs to the whole file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub path: String,
    pub span: Option<Span>,
    pub text: String,
    /// The reviewer should take extra care here: security, data loss, hard to undo, easy to get wrong.
    pub critical: bool,
}

/// A file a chapter covers. No spans means the whole file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChapterFile {
    pub path: String,
    pub spans: Vec<Span>,
}

/// Code the diff does not touch that depends on what the chapter changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Impact {
    pub site: String,
    pub path: String,
    pub line: Option<u32>,
    pub text: String,
}

/// One step of "Follow the flow": the node (or participant) that is active, and what happens there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlowStep {
    pub node: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagram {
    pub title: Option<String>,
    pub kind: DiagramKind,
    pub source: String,
    pub flow: Vec<FlowStep>,
}

impl Diagram {
    /// The typed graph for the UI. `Graph::Source` for a kind this crate does not read.
    pub fn graph(&self) -> Result<Graph, mermaid::MermaidError> {
        mermaid::parse(&self.source)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Overview {
    pub title: String,
    pub summary: String,
    pub before: Option<String>,
    pub after: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chapter {
    pub title: String,
    pub summary: String,
    /// The stage of the change, such as "Core" or "UI". Chapters of one phase sit together.
    pub phase: Option<String>,
    pub attention: Attention,
    pub points: Vec<String>,
    /// What to check while reading these files.
    pub focus: Vec<String>,
    /// In reading order.
    pub files: Vec<ChapterFile>,
    pub notes: Vec<Note>,
    pub diagrams: Vec<Diagram>,
    pub impact: Vec<Impact>,
}

impl Chapter {
    pub fn covers(&self, path: &str) -> bool {
        self.files.iter().any(|file| file.path == path)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Guide {
    pub overview: Overview,
    pub diagrams: Vec<Diagram>,
    /// In reading order.
    pub chapters: Vec<Chapter>,
    /// Of the whole diff the guide describes. See `stale::fingerprint`.
    pub fingerprint: String,
    /// Per file, so a chapter can tell which of its files changed since.
    pub file_fingerprints: BTreeMap<String, String>,
    /// `HEAD` when the guide was written, for "the branch moved 2 commits".
    pub head: Option<String>,
}

impl Guide {
    pub fn covers(&self, path: &str) -> bool {
        self.chapters.iter().any(|chapter| chapter.covers(path))
    }

    pub fn is_empty(&self) -> bool {
        self.overview.summary.trim().is_empty() || self.chapters.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chapter(path: &str) -> Chapter {
        Chapter {
            title: "C".into(),
            summary: "S".into(),
            phase: None,
            attention: Attention::Normal,
            points: vec![],
            focus: vec![],
            files: vec![ChapterFile {
                path: path.into(),
                spans: vec![],
            }],
            notes: vec![],
            diagrams: vec![],
            impact: vec![],
        }
    }

    #[test]
    fn a_guide_is_empty_until_it_has_a_summary_and_a_chapter() {
        let mut guide = Guide::default();
        assert!(guide.is_empty());
        guide.chapters.push(chapter("a.ts"));
        assert!(guide.is_empty());
        guide.overview.summary = "What it does.".into();
        assert!(!guide.is_empty());
        assert!(guide.covers("a.ts") && !guide.covers("b.ts"));
    }

    #[test]
    fn a_span_ends_where_it_says_and_never_before_it_starts() {
        let span = |end_line| Span {
            side: Side::New,
            line: 5,
            end_line,
            anchor_text: None,
        };
        assert_eq!(span(None).last_line(), 5);
        assert_eq!(span(Some(8)).last_line(), 8);
        assert_eq!(span(Some(2)).last_line(), 5);
        assert_eq!(span(None).anchor("a.ts"), ThreadAnchor::line("a.ts", Side::New, 5));
    }

    #[test]
    fn the_stored_shape_uses_plain_lowercase_names() {
        assert_eq!(serde_json::to_value(Attention::High).unwrap(), "high");
        assert_eq!(serde_json::to_value(DiagramKind::Sequence).unwrap(), "sequence");
        assert_eq!(
            serde_json::to_value(DiagramKind::Other("gantt".into())).unwrap(),
            serde_json::json!({ "other": "gantt" })
        );
        let json = serde_json::to_string(&Guide::default()).unwrap();
        assert_eq!(serde_json::from_str::<Guide>(&json).unwrap(), Guide::default());
    }
}
