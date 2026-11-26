//! Every in-process `git diff` runs here. A diff whose new side is the working tree also shows untracked files
//! as new files: they go into a temporary copy of the index as intent-to-add, so git's own rename, binary, and
//! line-count logic covers them. diffity builds these patches by hand instead, which duplicates that logic.

use crate::git::{self, GitError};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

pub struct GitDiff<'a> {
    repo: &'a Path,
    revisions: &'a [String],
    index: Option<(tempfile::TempDir, PathBuf)>,
}

impl<'a> GitDiff<'a> {
    pub fn new(repo: &'a Path, revisions: &'a [String]) -> Result<Self, GitError> {
        let index = if reads_working_tree(revisions) {
            untracked_index(repo)?
        } else {
            None
        };
        Ok(Self { repo, revisions, index })
    }

    pub fn run(&self, args: &[&str], paths: &[String]) -> Result<String, GitError> {
        let mut full: Vec<&str> = args.to_vec();
        full.extend(self.revisions.iter().map(String::as_str));
        if !paths.is_empty() {
            full.push("--");
            full.extend(paths.iter().map(String::as_str));
        }
        git::run_io(self.repo, &full, &self.env(), None).map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
    }

    pub fn uncommitted_paths(&self) -> Result<String, GitError> {
        git::run_io(self.repo, &["diff", "-z", "--name-only", "HEAD"], &self.env(), None)
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
    }

    fn env(&self) -> Vec<(&str, &OsStr)> {
        self.index
            .iter()
            .map(|(_, index)| ("GIT_INDEX_FILE", index.as_os_str()))
            .collect()
    }
}

/// Git's own rule: with `--cached`, two commits, or a range, the new side is a commit or the index.
/// Otherwise it is the working tree.
pub fn reads_working_tree(revisions: &[String]) -> bool {
    let mut commits = 0;
    for word in revisions {
        match word.as_str() {
            "--cached" | "--staged" => return false,
            "--merge-base" => {}
            range if range.contains("..") => return false,
            _ => commits += 1,
        }
    }
    commits <= 1
}

fn untracked_index(repo: &Path) -> Result<Option<(tempfile::TempDir, PathBuf)>, GitError> {
    let untracked = git::run_io(repo, &["ls-files", "-z", "--others", "--exclude-standard"], &[], None)?;
    if untracked.iter().all(|byte| *byte == 0) {
        return Ok(None);
    }
    let io_failed = |error: std::io::Error| GitError::Failed(error.to_string());
    let dir = tempfile::tempdir().map_err(io_failed)?;
    let index = dir.path().join("index");
    let real = PathBuf::from(git::run(repo, &["rev-parse", "--absolute-git-dir"])?.trim()).join("index");
    if real.exists() {
        std::fs::copy(&real, &index).map_err(io_failed)?;
    }
    git::run_io(
        repo,
        &[
            "--literal-pathspecs",
            "add",
            "--intent-to-add",
            "--pathspec-from-file=-",
            "--pathspec-file-nul",
        ],
        &[("GIT_INDEX_FILE", index.as_os_str())],
        Some(&untracked),
    )?;
    Ok(Some((dir, index)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{commit, git, init_repo, write};

    fn words(revisions: &[&str]) -> Vec<String> {
        revisions.iter().map(|word| word.to_string()).collect()
    }

    #[test]
    fn only_diffs_whose_new_side_is_the_working_tree_read_it() {
        for revisions in [&[][..], &["HEAD"], &["--merge-base", "main"]] {
            assert!(reads_working_tree(&words(revisions)), "{revisions:?}");
        }
        for revisions in [
            &["--cached"][..],
            &["--cached", "HEAD"],
            &["--merge-base", "main", "topic"],
            &["main", "topic"],
            &["main...topic"],
            &["main..topic"],
        ] {
            assert!(!reads_working_tree(&words(revisions)), "{revisions:?}");
        }
    }

    #[test]
    fn untracked_files_are_new_files_and_the_real_index_is_untouched() {
        let dir = init_repo();
        let repo = dir.path();
        write(repo, "a.txt", "a\n");
        commit(repo, "base");
        write(repo, ".gitignore", "ignored.txt\n");
        write(repo, "ignored.txt", "ignored\n");
        std::fs::create_dir(repo.join("new dir")).unwrap();
        write(repo, "new dir/[b].txt", "one\ntwo\n");
        let index = std::fs::read(repo.join(".git/index")).unwrap();

        let head = words(&["HEAD"]);
        let diff = GitDiff::new(repo, &head).unwrap();
        let names = diff.run(&["diff", "--name-status"], &[]).unwrap();
        assert_eq!(names, "A\t.gitignore\nA\tnew dir/[b].txt\n");
        assert_eq!(diff.uncommitted_paths().unwrap(), ".gitignore\0new dir/[b].txt\0");

        let cached = words(&["--cached"]);
        let staged = GitDiff::new(repo, &cached).unwrap();
        assert_eq!(staged.run(&["diff", "--name-status"], &[]).unwrap(), "");

        drop(diff);
        assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), index);
        assert!(git(repo, &["status", "--porcelain"]).contains("?? .gitignore"));
    }
}
