use crate::comparison::Comparison;
use crate::git;
use crate::threads::Side;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PreviewKind {
    Image,
    Markdown,
}

pub fn preview_kind(path: &str) -> Option<PreviewKind> {
    let (_, extension) = path.rsplit_once('.')?;
    match extension.to_ascii_lowercase().as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico" | "svg" => Some(PreviewKind::Image),
        "md" | "markdown" => Some(PreviewKind::Markdown),
        _ => None,
    }
}

/// The bytes of `path` on one side of the comparison, or `None` when the file does not exist there.
/// For the old side of a renamed file, pass the old path.
pub fn read_side(repo: &Path, comparison: &Comparison, side: Side, path: &str) -> Option<Vec<u8>> {
    match side {
        Side::Old => show(repo, &old_revision(repo, comparison)?, path),
        Side::New => match new_source(comparison) {
            NewSource::WorkingTree => std::fs::read(repo.join(path)).ok(),
            NewSource::Revision(revision) => show(repo, &revision, path),
        },
    }
}

enum NewSource {
    WorkingTree,
    Revision(String),
}

fn new_source(comparison: &Comparison) -> NewSource {
    match comparison {
        Comparison::CheckedOut {
            includes_uncommitted: true,
            ..
        }
        | Comparison::WorkingTree => NewSource::WorkingTree,
        Comparison::CheckedOut { .. } => NewSource::Revision("HEAD".into()),
        Comparison::Staged => NewSource::Revision(String::new()),
        Comparison::Branch { branch, .. } => NewSource::Revision(branch.clone()),
    }
}

fn old_revision(repo: &Path, comparison: &Comparison) -> Option<String> {
    let (base, head) = match comparison {
        Comparison::CheckedOut { base, .. } => (base.as_str(), "HEAD"),
        Comparison::Branch { branch, base } => (base.as_str(), branch.as_str()),
        Comparison::WorkingTree | Comparison::Staged => return Some("HEAD".into()),
    };
    let merge_base = git::run(repo, &["merge-base", base, head]).ok()?;
    Some(merge_base.trim().to_string())
}

fn show(repo: &Path, revision: &str, path: &str) -> Option<Vec<u8>> {
    git::run_io(repo, &["show", &format!("{revision}:{path}")], &[], None).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{commit, git as run_git, init_repo, write};
    use tempfile::TempDir;

    #[test]
    fn picks_the_preview_by_extension() {
        assert_eq!(preview_kind("a/b.PNG"), Some(PreviewKind::Image));
        assert_eq!(preview_kind("logo.svg"), Some(PreviewKind::Image));
        assert_eq!(preview_kind("README.md"), Some(PreviewKind::Markdown));
        assert_eq!(preview_kind("notes.markdown"), Some(PreviewKind::Markdown));
        assert_eq!(preview_kind("main.rs"), None);
        assert_eq!(preview_kind("png"), None);
    }

    fn fixture() -> TempDir {
        let dir = init_repo();
        let repo = dir.path();
        write(repo, "a.md", "one\n");
        write(repo, "gone.md", "gone\n");
        write(repo, "old-name.md", "renamed\n");
        commit(repo, "base");
        run_git(repo, &["checkout", "-q", "-b", "feature"]);
        write(repo, "a.md", "two\n");
        write(repo, "new.md", "new\n");
        std::fs::remove_file(repo.join("gone.md")).unwrap();
        run_git(repo, &["mv", "old-name.md", "new-name.md"]);
        commit(repo, "committed");
        write(repo, "a.md", "three\n");
        write(repo, "staged.md", "staged\n");
        run_git(repo, &["add", "staged.md"]);
        dir
    }

    fn text(bytes: Option<Vec<u8>>) -> Option<String> {
        bytes.map(|bytes| String::from_utf8(bytes).unwrap())
    }

    fn read(dir: &TempDir, comparison: &Comparison, side: Side, path: &str) -> Option<String> {
        text(read_side(dir.path(), comparison, side, path))
    }

    #[test]
    fn reads_each_side_of_a_checked_out_comparison() {
        let dir = fixture();
        let with = Comparison::checked_out("main", true);
        assert_eq!(read(&dir, &with, Side::Old, "a.md").as_deref(), Some("one\n"));
        assert_eq!(read(&dir, &with, Side::New, "a.md").as_deref(), Some("three\n"));
        let without = Comparison::checked_out("main", false);
        assert_eq!(read(&dir, &without, Side::New, "a.md").as_deref(), Some("two\n"));
    }

    #[test]
    fn a_side_that_does_not_exist_is_none() {
        let dir = fixture();
        let comparison = Comparison::checked_out("main", true);
        assert_eq!(read(&dir, &comparison, Side::Old, "new.md"), None);
        assert_eq!(read(&dir, &comparison, Side::New, "gone.md"), None);
    }

    #[test]
    fn a_renamed_file_is_read_by_its_old_path_on_the_old_side() {
        let dir = fixture();
        let comparison = Comparison::checked_out("main", true);
        assert_eq!(
            read(&dir, &comparison, Side::Old, "old-name.md").as_deref(),
            Some("renamed\n")
        );
        assert_eq!(
            read(&dir, &comparison, Side::New, "new-name.md").as_deref(),
            Some("renamed\n")
        );
    }

    #[test]
    fn staged_and_working_tree_compare_against_head() {
        let dir = fixture();
        run_git(dir.path(), &["add", "a.md"]);
        write(dir.path(), "a.md", "four\n");
        assert_eq!(
            read(&dir, &Comparison::Staged, Side::Old, "a.md").as_deref(),
            Some("two\n")
        );
        assert_eq!(
            read(&dir, &Comparison::Staged, Side::New, "a.md").as_deref(),
            Some("three\n")
        );
        assert_eq!(
            read(&dir, &Comparison::WorkingTree, Side::Old, "a.md").as_deref(),
            Some("two\n")
        );
        assert_eq!(
            read(&dir, &Comparison::WorkingTree, Side::New, "a.md").as_deref(),
            Some("four\n")
        );
    }

    #[test]
    fn a_branch_comparison_reads_the_branch_tip_and_the_merge_base() {
        let dir = fixture();
        let comparison = Comparison::branch("feature", "main");
        assert_eq!(read(&dir, &comparison, Side::Old, "a.md").as_deref(), Some("one\n"));
        assert_eq!(read(&dir, &comparison, Side::New, "a.md").as_deref(), Some("two\n"));
        assert_eq!(read(&dir, &comparison, Side::New, "staged.md"), None);
    }
}
