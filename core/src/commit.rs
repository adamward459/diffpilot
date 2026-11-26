use crate::exec;
use crate::git::GitError;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitMessage(String);

impl CommitMessage {
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        (!text.is_empty()).then(|| Self(text.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub fn commit_all(repo: &Path, message: &CommitMessage) -> Result<(), GitError> {
    exec::git_ok(repo, &["add", "-A"])?;
    exec::git(repo, &["commit", "-F", "-"], Some(message.as_str().as_bytes()))?.into_result()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{commit, git, init_repo, write};

    fn message(text: &str) -> CommitMessage {
        CommitMessage::parse(text).expect("non-empty")
    }

    fn repo_with_one_commit() -> tempfile::TempDir {
        let dir = init_repo();
        let repo = dir.path();
        git(repo, &["config", "user.name", "t"]);
        git(repo, &["config", "user.email", "t@t"]);
        write(repo, "a.txt", "one\n");
        commit(repo, "first");
        dir
    }

    #[test]
    fn commits_edits_and_new_files_with_the_message() {
        let dir = repo_with_one_commit();
        let repo = dir.path();
        write(repo, "a.txt", "one\ntwo\n");
        write(repo, "b.txt", "new\n");

        commit_all(repo, &message("Add b\n\nBody line")).expect("commit");

        assert_eq!(git(repo, &["log", "-1", "--format=%B"]).trim(), "Add b\n\nBody line");
        assert_eq!(git(repo, &["status", "--porcelain"]), "");
    }

    #[test]
    fn parse_rejects_blank_text_and_trims() {
        assert_eq!(CommitMessage::parse(" \n\t "), None);
        assert_eq!(CommitMessage::parse("  hi \n").expect("parsed").as_str(), "hi");
    }

    #[test]
    fn nothing_to_commit_reports_git_text() {
        let dir = repo_with_one_commit();

        let error = commit_all(dir.path(), &message("again")).expect_err("nothing to commit");

        assert!(error.to_string().contains("nothing to commit"), "{error}");
    }
}
