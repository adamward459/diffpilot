use crate::exec;
use crate::git::GitError;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    pub path: PathBuf,
    pub head: String,
    pub branch: Option<String>,
    pub is_main: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Checkout {
    Detached(String),
    ExistingBranch(String),
    NewBranch { name: String, start: String, track: bool },
}

pub fn list(repo: &Path) -> Result<Vec<Worktree>, GitError> {
    Ok(parse_porcelain(&exec::git_ok(
        repo,
        &["worktree", "list", "--porcelain"],
    )?))
}

pub fn parse_porcelain(text: &str) -> Vec<Worktree> {
    let mut trees: Vec<Worktree> = Vec::new();
    for block in text.split("\n\n") {
        let mut path = None;
        let mut head = String::new();
        let mut branch = None;
        for line in block.lines() {
            if let Some(value) = line.strip_prefix("worktree ") {
                path = Some(PathBuf::from(value));
            } else if let Some(value) = line.strip_prefix("HEAD ") {
                head = value.to_string();
            } else if let Some(value) = line.strip_prefix("branch ") {
                branch = Some(value.strip_prefix("refs/heads/").unwrap_or(value).to_string());
            }
        }
        if let Some(path) = path {
            let is_main = trees.is_empty();
            trees.push(Worktree {
                path,
                head,
                branch,
                is_main,
            });
        }
    }
    trees
}

pub fn add(repo: &Path, path: &Path, checkout: &Checkout) -> Result<(), GitError> {
    let path = path.to_string_lossy();
    match checkout {
        Checkout::Detached(rev) => exec::git_ok(repo, &["worktree", "add", "--detach", &path, rev]),
        Checkout::ExistingBranch(branch) => exec::git_ok(repo, &["worktree", "add", &path, branch]),
        Checkout::NewBranch { name, start, track } => {
            let tracking = if *track { "--track" } else { "--no-track" };
            exec::git_ok(repo, &["worktree", "add", tracking, "-b", name, &path, start])
        }
    }
    .map(|_| ())
}

pub fn remove(repo: &Path, path: &Path) -> Result<(), GitError> {
    let removed = exec::git(repo, &["worktree", "remove", "--force", &path.to_string_lossy()], None)?;
    if !removed.ok {
        if path.exists() {
            std::fs::remove_dir_all(path).map_err(|error| GitError::Failed(error.to_string()))?;
        }
        exec::git_ok(repo, &["worktree", "prune"])?;
    }
    Ok(())
}

/// A detached worktree that is removed when it is dropped, so a run can read real files of another revision.
pub struct TempWorktree {
    repo: PathBuf,
    path: PathBuf,
    removed: bool,
}

static COUNTER: AtomicU32 = AtomicU32::new(0);

impl TempWorktree {
    pub fn create(repo: &Path, rev: &str) -> Result<Self, GitError> {
        Self::create_in(&std::env::temp_dir().join("diffpilot-worktrees"), repo, rev)
    }

    pub fn create_in(base: &Path, repo: &Path, rev: &str) -> Result<Self, GitError> {
        let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
        let safe: String = rev
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .take(24)
            .collect();
        let path = base.join(format!("{safe}-{}-{sequence}", std::process::id()));
        std::fs::create_dir_all(base).map_err(|error| GitError::Failed(error.to_string()))?;
        add(repo, &path, &Checkout::Detached(rev.to_string()))?;
        Ok(Self {
            repo: repo.to_path_buf(),
            path,
            removed: false,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn remove(mut self) -> Result<(), GitError> {
        self.removed = true;
        remove(&self.repo, &self.path)
    }
}

impl Drop for TempWorktree {
    fn drop(&mut self) {
        if !self.removed {
            let _ = remove(&self.repo, &self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{commit, git, init_repo, write};

    fn repo() -> tempfile::TempDir {
        let dir = init_repo();
        write(dir.path(), "a.txt", "one\n");
        commit(dir.path(), "one");
        git(dir.path(), &["branch", "topic"]);
        write(dir.path(), "a.txt", "two\n");
        commit(dir.path(), "two");
        dir
    }

    #[test]
    fn parses_the_porcelain_list() {
        let text = "worktree /r\nHEAD aaa\nbranch refs/heads/main\n\nworktree /w\nHEAD bbb\ndetached\n\n";
        let trees = parse_porcelain(text);
        assert_eq!(trees.len(), 2);
        assert!(trees[0].is_main && trees[0].branch.as_deref() == Some("main"));
        assert!(!trees[1].is_main && trees[1].branch.is_none());
        assert_eq!(trees[1].path, Path::new("/w"));
    }

    #[test]
    fn a_temp_worktree_holds_another_revision_and_goes_away() {
        let dir = repo();
        let base = tempfile::tempdir().unwrap();
        let tree = TempWorktree::create_in(base.path(), dir.path(), "topic").unwrap();
        let path = tree.path().to_path_buf();
        assert_eq!(std::fs::read_to_string(path.join("a.txt")).unwrap(), "one\n");
        assert_eq!(std::fs::read_to_string(dir.path().join("a.txt")).unwrap(), "two\n");
        assert_eq!(
            git(&path, &["rev-parse", "HEAD"]),
            git(dir.path(), &["rev-parse", "topic"])
        );
        assert!(!git(&path, &["status", "--branch", "--porcelain"]).contains("main"));
        assert_eq!(list(dir.path()).unwrap().len(), 2);

        tree.remove().unwrap();
        assert!(!path.exists());
        assert_eq!(list(dir.path()).unwrap().len(), 1);
    }

    #[test]
    fn dropping_a_temp_worktree_removes_it() {
        let dir = repo();
        let base = tempfile::tempdir().unwrap();
        let path = {
            let tree = TempWorktree::create_in(base.path(), dir.path(), "HEAD").unwrap();
            tree.path().to_path_buf()
        };
        assert!(!path.exists());
        assert_eq!(list(dir.path()).unwrap().len(), 1);
    }

    #[test]
    fn removing_works_after_the_folder_was_deleted_by_hand() {
        let dir = repo();
        let base = tempfile::tempdir().unwrap();
        let tree = TempWorktree::create_in(base.path(), dir.path(), "topic").unwrap();
        std::fs::remove_dir_all(tree.path()).unwrap();
        tree.remove().unwrap();
        assert_eq!(list(dir.path()).unwrap().len(), 1);
    }

    #[test]
    fn creates_a_worktree_on_a_new_or_existing_branch() {
        let dir = repo();
        let base = tempfile::tempdir().unwrap();
        let new = base.path().join("new");
        add(
            dir.path(),
            &new,
            &Checkout::NewBranch {
                name: "pr-1".into(),
                start: "topic".into(),
                track: false,
            },
        )
        .unwrap();
        let existing = base.path().join("existing");
        git(dir.path(), &["branch", "other"]);
        add(dir.path(), &existing, &Checkout::ExistingBranch("other".into())).unwrap();
        let trees = list(dir.path()).unwrap();
        let mut branches: Vec<_> = trees.iter().filter_map(|tree| tree.branch.as_deref()).collect();
        branches.sort_unstable();
        assert_eq!(branches, ["main", "other", "pr-1"]);
        assert!(
            add(
                dir.path(),
                &base.path().join("again"),
                &Checkout::ExistingBranch("other".into())
            )
            .is_err()
        );
        remove(dir.path(), &new).unwrap();
        remove(dir.path(), &existing).unwrap();
    }
}
