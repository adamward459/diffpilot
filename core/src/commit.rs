use crate::exec;
use crate::git::GitError;
use crate::git_diff::GitDiff;
use crate::review::ReviewModel;
use crate::run::RunKind;
use crate::runner::RunRequest;
use crate::store::SessionKey;
use std::path::Path;

const MAX_DIFF_BYTES: usize = 60_000;
const UNCOMMITTED: &str = "HEAD";

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

    /// Claude's answer, without a code fence if it wrapped the message in one.
    pub fn from_reply(text: &str) -> Option<Self> {
        let text = text.trim();
        let inner = text
            .strip_prefix("```")
            .and_then(|rest| rest.strip_suffix("```"))
            .map(|rest| rest.split_once('\n').map_or("", |(_, body)| body))
            .unwrap_or(text);
        Self::parse(inner)
    }
}

fn cut_at_line(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let cut = &text[..end];
    cut.rfind('\n').map_or(cut, |at| &cut[..at])
}

pub fn message_prompt(stat: &str, diff: &str) -> String {
    let diff_part = if diff.len() <= MAX_DIFF_BYTES {
        format!("## Diff\n\n```diff\n{}\n```", diff.trim_end())
    } else {
        format!(
            "## Diff (cut after {MAX_DIFF_BYTES} bytes)\n\nThe diff is too long to show in full. Use the file list above for the rest. You may run `git diff HEAD -- <path>` to read one file.\n\n```diff\n{}\n```",
            cut_at_line(diff, MAX_DIFF_BYTES)
        )
    };
    format!(
        "Write a git commit message for the uncommitted changes below. They include new files that git does not track yet.

Rules:
- The first line is a subject of at most 72 characters, in the imperative mood (\"Add\", \"Fix\", not \"Added\"), with no trailing period.
- Add a short body after a blank line only when the subject does not say enough. Explain why, not what.
- Match the style of recent commits if you can see it with `git log --oneline -10`.
- Reply with the commit message only. No quotes, no code fence, no preface.

## Files

{stat}

{diff_part}
"
    )
}

/// A read-only run that writes a commit message for everything uncommitted, untracked files included.
pub fn message_request(repo: &Path, session: SessionKey, model: ReviewModel) -> Result<RunRequest, GitError> {
    let revisions = [UNCOMMITTED.to_string()];
    let changes = GitDiff::new(repo, &revisions)?;
    let stat = changes.run(&["diff", "--no-color", "--stat=120"], &[])?;
    let diff = changes.run(&["diff", "--no-color", "-U3", "-M"], &[])?;
    let mut request = RunRequest::new(
        repo,
        session,
        RunKind::CommitMessage,
        RunKind::CommitMessage.title(),
        &message_prompt(&stat, &diff),
        model,
    );
    request.revisions = revisions.to_vec();
    Ok(request)
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
    fn the_prompt_holds_edits_and_untracked_files() {
        let dir = repo_with_one_commit();
        let repo = dir.path();
        write(repo, "a.txt", "one\ntwo\n");
        write(repo, "b.txt", "brand new\n");

        let session = SessionKey::new("/r", "main", "HEAD");
        let request = message_request(repo, session, ReviewModel::COMMIT_MESSAGE).expect("request");

        assert_eq!(request.kind, RunKind::CommitMessage);
        for needle in ["a.txt", "b.txt", "+two", "+brand new", "72 characters"] {
            assert!(request.prompt.contains(needle), "{needle}: {}", request.prompt);
        }
    }

    #[test]
    fn a_long_diff_is_cut_but_the_file_list_stays() {
        let diff = format!("{}\n", "+line of added text".repeat(10)).repeat(MAX_DIFF_BYTES / 100);
        let prompt = message_prompt(" big.txt | 9999 +", &diff);

        assert!(prompt.contains("big.txt | 9999 +"));
        assert!(prompt.contains("cut after"));
        assert!(prompt.len() < MAX_DIFF_BYTES + 2_000);
    }

    #[test]
    fn a_fenced_reply_loses_its_fence() {
        let text = |reply: &str| CommitMessage::from_reply(reply).map(|message| message.as_str().to_string());
        assert_eq!(text("```\nAdd b\n\nWhy\n```").as_deref(), Some("Add b\n\nWhy"));
        assert_eq!(text("```text\nAdd b\n```").as_deref(), Some("Add b"));
        assert_eq!(text("  Add b \n").as_deref(), Some("Add b"));
        assert_eq!(text("```\n```"), None);
    }

    #[test]
    fn nothing_to_commit_reports_git_text() {
        let dir = repo_with_one_commit();

        let error = commit_all(dir.path(), &message("again")).expect_err("nothing to commit");

        assert!(error.to_string().contains("nothing to commit"), "{error}");
    }
}
