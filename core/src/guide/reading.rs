//! The guide's chapters over the diff as it is now. Ported from diffity's `readingChapters` and `phaseGroups`
//! (MIT, see THIRD_PARTY_NOTICES).

use super::{Attention, Guide, Note};
use crate::diff::FileDiff;
use std::borrow::Borrow;
use std::collections::{HashMap, HashSet};
use std::ops::Range;

pub const NOT_IN_GUIDE_TITLE: &str = "Not in the guide";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChapterKey {
    /// Index into `Guide::chapters`.
    Guide(usize),
    Extra,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadingChapter {
    pub key: ChapterKey,
    pub title: String,
    pub summary: String,
    pub phase: Option<String>,
    pub attention: Attention,
    /// In reading order, each path once. Only paths still in the diff.
    pub files: Vec<String>,
    pub notes: Vec<Note>,
    pub additions: u32,
    pub deletions: u32,
    pub critical: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Reading {
    pub chapters: Vec<ReadingChapter>,
}

/// Files that left the diff are dropped, chapters left empty are dropped, and files the guide never saw are
/// collected in a last chapter.
pub fn reading<F: Borrow<FileDiff>>(guide: &Guide, files: &[F]) -> Reading {
    let mut by_path: HashMap<&str, &FileDiff> = HashMap::new();
    for diff in files.iter().map(Borrow::borrow) {
        by_path.entry(diff.path.as_str()).or_insert(diff);
    }
    let mut placed: HashSet<&str> = HashSet::new();
    let mut chapters = Vec::new();
    for (index, chapter) in guide.chapters.iter().enumerate() {
        let kept: Vec<&FileDiff> = chapter
            .files
            .iter()
            .filter_map(|file| by_path.get(file.path.as_str()).copied())
            .filter(|diff| placed.insert(diff.path.as_str()))
            .collect();
        if kept.is_empty() {
            continue;
        }
        let notes: Vec<Note> = chapter
            .notes
            .iter()
            .filter(|note| kept.iter().any(|diff| diff.path == note.path))
            .cloned()
            .collect();
        chapters.push(ReadingChapter {
            key: ChapterKey::Guide(index),
            title: chapter.title.clone(),
            summary: chapter.summary.clone(),
            phase: chapter.phase.clone(),
            attention: chapter.attention,
            critical: notes.iter().filter(|note| note.critical).count(),
            notes,
            ..counted(&kept)
        });
    }
    let rest: Vec<&FileDiff> = files
        .iter()
        .map(Borrow::borrow)
        .filter(|diff| !placed.contains(diff.path.as_str()))
        .collect();
    if !rest.is_empty() {
        chapters.push(ReadingChapter {
            key: ChapterKey::Extra,
            title: NOT_IN_GUIDE_TITLE.into(),
            summary: "These files changed after the guide was written, so it does not explain them.".into(),
            phase: None,
            attention: Attention::Normal,
            notes: Vec::new(),
            critical: 0,
            ..counted(&rest)
        });
    }
    Reading { chapters }
}

fn counted(files: &[&FileDiff]) -> ReadingChapter {
    ReadingChapter {
        key: ChapterKey::Extra,
        title: String::new(),
        summary: String::new(),
        phase: None,
        attention: Attention::Normal,
        files: files.iter().map(|diff| diff.path.clone()).collect(),
        notes: Vec::new(),
        additions: files.iter().map(|diff| diff.additions).sum(),
        deletions: files.iter().map(|diff| diff.deletions).sum(),
        critical: 0,
    }
}

impl Reading {
    /// The chapters that come from the guide, not the "Not in the guide" one.
    pub fn guided(&self) -> usize {
        self.chapters
            .iter()
            .filter(|chapter| chapter.key != ChapterKey::Extra)
            .count()
    }
}

/// Neighbouring chapters of one phase, in order. A chapter with no phase stands alone with no heading.
pub fn phase_groups(chapters: &[ReadingChapter]) -> Vec<(Option<String>, Range<usize>)> {
    let mut groups: Vec<(Option<String>, Range<usize>)> = Vec::new();
    for (index, chapter) in chapters.iter().enumerate() {
        let phase = chapter
            .phase
            .as_deref()
            .map(str::trim)
            .filter(|phase| !phase.is_empty());
        match groups.last_mut() {
            Some((Some(last), range)) if phase == Some(last.as_str()) => range.end = index + 1,
            _ => groups.push((phase.map(str::to_string), index..index + 1)),
        }
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::changes::FileStatus;
    use crate::guide::{Chapter, ChapterFile};

    fn diff(path: &str, additions: u32, deletions: u32) -> FileDiff {
        let mut file = FileDiff::new(path, FileStatus::Modified);
        file.additions = additions;
        file.deletions = deletions;
        file
    }

    fn chapter(title: &str, phase: Option<&str>, paths: &[&str]) -> Chapter {
        Chapter {
            title: title.into(),
            summary: String::new(),
            phase: phase.map(str::to_string),
            attention: Attention::Normal,
            points: vec![],
            focus: vec![],
            files: paths
                .iter()
                .map(|path| ChapterFile {
                    path: (*path).into(),
                    spans: vec![],
                })
                .collect(),
            notes: vec![],
            diagrams: vec![],
            impact: vec![],
        }
    }

    fn note(path: &str, critical: bool) -> Note {
        Note {
            path: path.into(),
            span: None,
            text: "text".into(),
            critical,
            kind: Default::default(),
            check: None,
            node: None,
            folder: None,
        }
    }

    fn guide(chapters: Vec<Chapter>) -> Guide {
        Guide {
            chapters,
            ..Guide::default()
        }
    }

    fn titles(reading: &Reading) -> Vec<&str> {
        reading.chapters.iter().map(|chapter| chapter.title.as_str()).collect()
    }

    #[test]
    fn chapters_keep_their_order_and_sum_their_line_counts() {
        let guide = guide(vec![
            chapter("One", None, &["b.ts", "a.ts"]),
            chapter("Two", None, &["c.ts"]),
        ]);
        let files = [diff("a.ts", 3, 1), diff("b.ts", 2, 0), diff("c.ts", 5, 4)];
        let reading = reading(&guide, &files);
        assert_eq!(titles(&reading), ["One", "Two"]);
        assert_eq!(reading.chapters[0].files, ["b.ts", "a.ts"]);
        assert_eq!((reading.chapters[0].additions, reading.chapters[0].deletions), (5, 1));
        assert_eq!((reading.chapters[1].additions, reading.chapters[1].deletions), (5, 4));
        assert_eq!(reading.chapters[1].key, ChapterKey::Guide(1));
    }

    #[test]
    fn a_file_that_left_the_diff_is_dropped_with_its_notes() {
        let mut one = chapter("One", None, &["a.ts", "gone.ts"]);
        one.notes = vec![note("a.ts", false), note("gone.ts", true)];
        let reading = reading(&guide(vec![one]), &[diff("a.ts", 1, 0)]);
        assert_eq!(reading.chapters[0].files, ["a.ts"]);
        assert_eq!(reading.chapters[0].notes.len(), 1);
        assert_eq!(reading.chapters[0].critical, 0);
    }

    #[test]
    fn a_chapter_left_empty_is_dropped() {
        let guide = guide(vec![
            chapter("Gone", None, &["gone.ts"]),
            chapter("Kept", None, &["a.ts"]),
        ]);
        let reading = reading(&guide, &[diff("a.ts", 1, 0)]);
        assert_eq!(titles(&reading), ["Kept"]);
        assert_eq!(reading.chapters[0].key, ChapterKey::Guide(1));
    }

    #[test]
    fn the_first_chapter_to_name_a_file_keeps_it() {
        let guide = guide(vec![
            chapter("One", None, &["a.ts"]),
            chapter("Two", None, &["a.ts", "b.ts"]),
        ]);
        let reading = reading(&guide, &[diff("a.ts", 1, 0), diff("b.ts", 1, 0)]);
        assert_eq!(reading.chapters[0].files, ["a.ts"]);
        assert_eq!(reading.chapters[1].files, ["b.ts"]);
    }

    #[test]
    fn files_the_guide_never_saw_land_in_a_last_chapter() {
        let guide = guide(vec![chapter("One", None, &["a.ts"])]);
        let reading = reading(&guide, &[diff("a.ts", 1, 0), diff("new.ts", 7, 2)]);
        let extra = reading.chapters.last().unwrap();
        assert_eq!(extra.key, ChapterKey::Extra);
        assert_eq!(extra.title, NOT_IN_GUIDE_TITLE);
        assert_eq!(extra.files, ["new.ts"]);
        assert_eq!((extra.additions, extra.deletions), (7, 2));
        assert_eq!(reading.guided(), 1);
    }

    #[test]
    fn critical_notes_are_counted_per_chapter() {
        let mut one = chapter("One", None, &["a.ts"]);
        one.notes = vec![note("a.ts", true), note("a.ts", false), note("a.ts", true)];
        let reading = reading(&guide(vec![one]), &[diff("a.ts", 1, 0)]);
        assert_eq!(reading.chapters[0].critical, 2);
    }

    #[test]
    fn neighbours_of_one_phase_group_and_phase_less_chapters_stand_alone() {
        let guide = guide(vec![
            chapter("A", Some("Core"), &["a.ts"]),
            chapter("B", Some(" Core "), &["b.ts"]),
            chapter("C", None, &["c.ts"]),
            chapter("D", None, &["d.ts"]),
            chapter("E", Some("Core"), &["e.ts"]),
        ]);
        let files: Vec<FileDiff> = ["a", "b", "c", "d", "e"]
            .iter()
            .map(|n| diff(&format!("{n}.ts"), 1, 0))
            .collect();
        let groups = phase_groups(&reading(&guide, &files).chapters);
        let shape: Vec<(Option<&str>, Range<usize>)> = groups.iter().map(|(p, r)| (p.as_deref(), r.clone())).collect();
        assert_eq!(
            shape,
            [(Some("Core"), 0..2), (None, 2..3), (None, 3..4), (Some("Core"), 4..5)]
        );
    }
}
