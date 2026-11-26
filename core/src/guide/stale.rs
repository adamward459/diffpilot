//! Tells whether a guide still describes the diff, and which of its parts lost their code.
//!
//! diffity hashes the diff stat, HEAD and file times. This hashes the content of every file diff, so a rebase
//! that leaves the diff as it was does not make a guide stale, and an edit on disk that changes a hunk does.

use super::Guide;
use crate::anchor::{self, Placement};
use crate::diff::FileDiff;
use crate::git;
use crate::threads::ThreadAnchor;
use sha2::{Digest, Sha256};
use std::borrow::Borrow;
use std::collections::BTreeMap;
use std::path::Path;

pub fn file_fingerprints<F: Borrow<FileDiff>>(files: &[F]) -> BTreeMap<String, String> {
    files
        .iter()
        .map(|file| (file.borrow().path.clone(), file.borrow().fingerprint()))
        .collect()
}

/// A hash of the whole diff: every path with the fingerprint of its file diff, in path order.
pub fn fingerprint<F: Borrow<FileDiff>>(files: &[F]) -> String {
    let mut hasher = Sha256::new();
    for (path, file) in file_fingerprints(files) {
        hasher.update(path.as_bytes());
        hasher.update([0]);
        hasher.update(file.as_bytes());
        hasher.update([0]);
    }
    hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ChapterStaleness {
    pub gone_files: Vec<String>,
    pub changed_files: Vec<String>,
    /// One per note of the chapter, in order.
    pub notes: Vec<Placement>,
    /// One list per file of the chapter, one placement per span.
    pub spans: Vec<Vec<Placement>>,
}

impl ChapterStaleness {
    pub fn is_current(&self) -> bool {
        self.gone_files.is_empty()
            && self.changed_files.is_empty()
            && self
                .notes
                .iter()
                .chain(self.spans.iter().flatten())
                .all(|placement| *placement == Placement::Current)
    }

    /// Notes and spans whose code is no longer in the diff.
    pub fn lost_anchors(&self) -> usize {
        self.notes
            .iter()
            .chain(self.spans.iter().flatten())
            .filter(|placement| matches!(placement, Placement::Outdated | Placement::FileGone))
            .count()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Staleness {
    pub is_stale: bool,
    pub uncovered_files: Vec<String>,
    pub chapters: Vec<ChapterStaleness>,
}

impl Staleness {
    pub fn needs_attention(&self) -> bool {
        self.is_stale || !self.uncovered_files.is_empty()
    }
}

pub fn check<F: Borrow<FileDiff>>(guide: &Guide, files: &[F]) -> Staleness {
    let now = file_fingerprints(files);
    let chapters = guide
        .chapters
        .iter()
        .map(|chapter| {
            let mut status = ChapterStaleness::default();
            for file in &chapter.files {
                match now.get(&file.path) {
                    None => status.gone_files.push(file.path.clone()),
                    Some(current)
                        if guide
                            .file_fingerprints
                            .get(&file.path)
                            .is_some_and(|then| then != current) =>
                    {
                        status.changed_files.push(file.path.clone());
                    }
                    Some(_) => {}
                }
                status.spans.push(
                    file.spans
                        .iter()
                        .map(|span| {
                            anchor::place_at(
                                &span.anchor(&file.path),
                                span.end_line,
                                span.anchor_text.as_deref(),
                                files,
                            )
                        })
                        .collect(),
                );
            }
            status.notes = chapter
                .notes
                .iter()
                .map(|note| {
                    let (anchor, end_line, text) = match &note.span {
                        Some(span) => (span.anchor(&note.path), span.end_line, span.anchor_text.as_deref()),
                        None => (ThreadAnchor::file(&note.path), None, None),
                    };
                    anchor::place_at(&anchor, end_line, text, files)
                })
                .collect();
            status
        })
        .collect();
    Staleness {
        is_stale: guide.fingerprint != fingerprint(files),
        uncovered_files: files
            .iter()
            .map(Borrow::borrow)
            .filter(|file| !guide.covers(&file.path))
            .map(|file| file.path.clone())
            .collect(),
        chapters,
    }
}

/// How many commits `HEAD` is ahead of the one the guide was written on. None when the guide has no commit, or
/// `HEAD` does not descend from it (a rebase or a switch to another branch).
pub fn commits_since(repo: &Path, guide: &Guide) -> Option<u32> {
    let head = guide.head.as_deref()?;
    git::run(repo, &["merge-base", "--is-ancestor", head, "HEAD"]).ok()?;
    git::run(repo, &["rev-list", "--count", &format!("{head}..HEAD")])
        .ok()?
        .trim()
        .parse()
        .ok()
}

pub fn head(repo: &Path) -> Option<String> {
    git::run(repo, &["rev-parse", "HEAD"])
        .ok()
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

#[cfg(test)]
mod tests {
    use super::super::build::{self, ChapterArgs, NoteArgs};
    use super::super::fixtures::{SEND_NEW, TIMER, file, load, repo};
    use super::*;
    use crate::testutil::{commit, git, write};
    use serde_json::json;

    struct Setup {
        repo: tempfile::TempDir,
        guide: Guide,
    }

    /// A guide over the fixture repo: chapter 1 is `src/send.ts` with notes on the changed line and the whole file,
    /// chapter 2 is `src/timer.ts`. `notes.md` is in no chapter.
    fn setup() -> Setup {
        let repo = repo();
        let files = load(repo.path());
        let mut guide = build::begin(&files, super::head(repo.path()));
        let chapter = |title: &str, file: &str, span: bool| -> ChapterArgs {
            let entry = if span {
                json!({ "path": file, "ranges": [{ "startLine": 5, "endLine": 5 }] })
            } else {
                json!(file)
            };
            serde_json::from_value(json!({ "title": title, "summary": "Why.", "files": [entry] })).unwrap()
        };
        build::add_chapter(&mut guide, &files, &chapter("Send", "src/send.ts", true)).unwrap();
        build::add_chapter(&mut guide, &files, &chapter("Timer", "src/timer.ts", false)).unwrap();
        for line in [Some(5), None] {
            let note: NoteArgs =
                serde_json::from_value(json!({ "chapter": 1, "file": "src/send.ts", "line": line, "text": "Note." }))
                    .unwrap();
            build::add_note(&mut guide, &files, &note).unwrap();
        }
        Setup { repo, guide }
    }

    #[test]
    fn the_fingerprint_follows_the_content_of_the_diff_and_not_its_order() {
        let repo = repo();
        let files = load(repo.path());
        let reversed: Vec<_> = files.iter().rev().cloned().collect();
        assert_eq!(fingerprint(&files), fingerprint(&reversed));
        assert_eq!(fingerprint(&files), fingerprint(&load(repo.path())));
        assert_ne!(fingerprint(&files), fingerprint(&files[..2]));

        write(repo.path(), "notes.md", "# notes\n- undo send, five seconds\n");
        git(repo.path(), &["add", "-A"]);
        assert_ne!(fingerprint(&files), fingerprint(&load(repo.path())));
    }

    #[test]
    fn a_rebase_that_leaves_the_diff_alone_does_not_make_the_guide_stale() {
        let setup = setup();
        git(setup.repo.path(), &["commit", "-q", "-m", "work"]);
        git(setup.repo.path(), &["commit", "-q", "--amend", "-m", "reworded"]);
        let from_base = crate::diff::load_revisions(setup.repo.path(), &["HEAD~1".to_string()]).unwrap();
        let status = check(&setup.guide, &from_base);
        assert!(!status.is_stale, "the content of the diff is the same");
        assert_ne!(
            super::head(setup.repo.path()),
            setup.guide.head,
            "but the commit is another one"
        );
    }

    #[test]
    fn an_untouched_diff_is_current_and_names_the_file_no_chapter_covers() {
        let setup = setup();
        let status = check(&setup.guide, &load(setup.repo.path()));
        assert!(!status.is_stale);
        assert_eq!(status.uncovered_files, ["notes.md"]);
        assert!(status.needs_attention());
        assert!(status.chapters.iter().all(ChapterStaleness::is_current));
        assert_eq!(status.chapters[0].notes, [Placement::Current, Placement::Current]);
        assert_eq!(status.chapters[0].spans, [[Placement::Current]]);
        assert_eq!(status.chapters[1].spans, [Vec::<Placement>::new()]);
    }

    #[test]
    fn a_file_that_changed_makes_its_chapter_and_the_guide_stale() {
        let setup = setup();
        write(
            setup.repo.path(),
            "src/timer.ts",
            &format!("{TIMER}export const SECONDS = 5;\n"),
        );
        git(setup.repo.path(), &["add", "-A"]);
        let status = check(&setup.guide, &load(setup.repo.path()));
        assert!(status.is_stale);
        assert_eq!(status.chapters[1].changed_files, ["src/timer.ts"]);
        assert!(!status.chapters[1].is_current());
        assert!(status.chapters[0].is_current(), "the other chapter did not move");
    }

    #[test]
    fn a_file_that_left_the_diff_is_gone_and_its_notes_are_lost() {
        let setup = setup();
        git(setup.repo.path(), &["reset", "-q", "HEAD", "src/send.ts"]);
        git(setup.repo.path(), &["checkout", "--", "src/send.ts"]);
        let status = check(&setup.guide, &load(setup.repo.path()));
        assert!(status.is_stale);
        assert_eq!(status.chapters[0].gone_files, ["src/send.ts"]);
        assert_eq!(status.chapters[0].notes, [Placement::FileGone, Placement::FileGone]);
        assert_eq!(status.chapters[0].lost_anchors(), 3);
        assert!(!file(&load(setup.repo.path()), "src/timer.ts").hunks.is_empty());
    }

    #[test]
    fn a_note_follows_its_line_when_code_is_added_above_it() {
        let setup = setup();
        let moved = format!("// header\n// header 2\n{SEND_NEW}");
        write(setup.repo.path(), "src/send.ts", &moved);
        git(setup.repo.path(), &["add", "-A"]);
        let status = check(&setup.guide, &load(setup.repo.path()));
        assert_eq!(
            status.chapters[0].notes[0],
            Placement::Moved {
                line: 7,
                end_line: None
            }
        );
        assert_eq!(
            status.chapters[0].spans[0],
            [Placement::Moved {
                line: 7,
                end_line: None
            }]
        );
        assert_eq!(
            status.chapters[0].notes[1],
            Placement::Current,
            "a file note has no line to lose"
        );
        assert_eq!(status.chapters[0].lost_anchors(), 0);
        assert!(status.is_stale);
    }

    #[test]
    fn a_note_whose_line_was_rewritten_is_lost() {
        let setup = setup();
        write(
            setup.repo.path(),
            "src/send.ts",
            &SEND_NEW.replace("startTimer(id);", "scheduleSend(id);"),
        );
        git(setup.repo.path(), &["add", "-A"]);
        let status = check(&setup.guide, &load(setup.repo.path()));
        assert_eq!(status.chapters[0].notes[0], Placement::Outdated);
        assert_eq!(status.chapters[0].spans[0], [Placement::Outdated]);
        assert_eq!(status.chapters[0].lost_anchors(), 2);
    }

    #[test]
    fn a_new_file_is_uncovered_and_makes_the_guide_stale() {
        let setup = setup();
        write(setup.repo.path(), "src/extra.ts", "export const extra = 1;\n");
        git(setup.repo.path(), &["add", "-A"]);
        let status = check(&setup.guide, &load(setup.repo.path()));
        assert!(status.is_stale);
        assert_eq!(status.uncovered_files, ["src/extra.ts", "notes.md"]);
    }

    #[test]
    fn commits_since_counts_the_commits_on_top_of_the_one_the_guide_was_written_on() {
        let mut setup = setup();
        commit(setup.repo.path(), "feature");
        let written = head(setup.repo.path());
        setup.guide.head = written.clone();
        assert_eq!(commits_since(setup.repo.path(), &setup.guide), Some(0));
        write(setup.repo.path(), "notes.md", "# notes\nmore\n");
        commit(setup.repo.path(), "two");
        write(setup.repo.path(), "notes.md", "# notes\nmore\nand more\n");
        commit(setup.repo.path(), "three");
        assert_eq!(commits_since(setup.repo.path(), &setup.guide), Some(2));

        git(setup.repo.path(), &["checkout", "-q", "-b", "other", "main~3"]);
        assert_eq!(
            commits_since(setup.repo.path(), &setup.guide),
            None,
            "HEAD does not descend from the guide's commit"
        );
        setup.guide.head = None;
        assert_eq!(commits_since(setup.repo.path(), &setup.guide), None);
    }
}
