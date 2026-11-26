use crate::changes;
use crate::comparison::Comparison;
use crate::git::{self, GitError};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FolderStatus {
    pub branch: Option<String>,
    pub files: usize,
    pub added: u32,
    pub removed: u32,
    pub dirty: bool,
}

pub fn folder_status(repo: &Path, comparison: &Comparison) -> Result<FolderStatus, GitError> {
    let branch = git::run(repo, &["symbolic-ref", "--quiet", "--short", "HEAD"])
        .ok()
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty());
    let changes = changes::load(repo, comparison)?;
    let dirty = !git::run(repo, &["status", "--porcelain"])?.trim().is_empty();
    Ok(FolderStatus {
        branch,
        files: changes.len(),
        added: changes.iter().map(|change| change.additions).sum(),
        removed: changes.iter().map(|change| change.deletions).sum(),
        dirty,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{commit, git, init_repo, write};

    #[test]
    fn a_clean_repo_has_a_branch_and_no_changes() {
        let repo = init_repo();
        write(repo.path(), "a.txt", "one\n");
        commit(repo.path(), "first");
        let status = folder_status(repo.path(), &Comparison::checked_out("main", true)).unwrap();
        assert_eq!(
            status,
            FolderStatus {
                branch: Some("main".into()),
                ..FolderStatus::default()
            }
        );
    }

    #[test]
    fn counts_files_and_lines_against_the_base_and_flags_a_dirty_tree() {
        let repo = init_repo();
        write(repo.path(), "a.txt", "one\ntwo\n");
        commit(repo.path(), "first");
        git(repo.path(), &["checkout", "-q", "-b", "topic"]);
        write(repo.path(), "a.txt", "one\n");
        write(repo.path(), "b.txt", "x\ny\nz\n");
        commit(repo.path(), "second");
        let comparison = Comparison::checked_out("main", true);

        let committed = folder_status(repo.path(), &comparison).unwrap();
        assert_eq!(committed.branch.as_deref(), Some("topic"));
        assert_eq!((committed.files, committed.added, committed.removed), (2, 3, 1));
        assert!(!committed.dirty);

        write(repo.path(), "a.txt", "one\nmore\n");
        let dirty = folder_status(repo.path(), &comparison).unwrap();
        assert!(dirty.dirty);
    }

    #[test]
    fn a_detached_head_has_no_branch() {
        let repo = init_repo();
        write(repo.path(), "a.txt", "one\n");
        commit(repo.path(), "first");
        git(repo.path(), &["checkout", "-q", "--detach"]);
        let status = folder_status(repo.path(), &Comparison::checked_out("main", true)).unwrap();
        assert_eq!(status.branch, None);
    }

    #[test]
    fn a_folder_that_is_not_a_repo_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(folder_status(dir.path(), &Comparison::WorkingTree).is_err());
    }
}
