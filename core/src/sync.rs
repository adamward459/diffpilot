// Adapted from diffity (MIT License, Copyright (c) 2026 Kamran Ahmed).
// The license text is in core/THIRD_PARTY_NOTICES.

//! Fetch, pull and push. Only the user starts these; Claude runs never call them.

use crate::exec;
use crate::git::GitError;
use std::path::Path;
use std::time::SystemTime;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SyncError {
    #[error("HEAD is detached, so there is no branch to sync")]
    DetachedHead,
    #[error("the repository has no remote")]
    NoRemote,
    #[error("`{0}` has no upstream branch")]
    NoUpstream(String),
    #[error("there are uncommitted changes to tracked files; commit or stash them first")]
    Dirty,
    #[error("the branch and its upstream have diverged, so it cannot be fast-forwarded")]
    Diverged,
    #[error("the rebase hit conflicts and was stopped: {0}")]
    Conflict(String),
    #[error("the remote has commits this branch lacks; pull first")]
    Rejected(String),
    #[error(transparent)]
    Git(#[from] GitError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchStatus {
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub dirty: bool,
    pub last_fetch: Option<SystemTime>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullMode {
    FastForward,
    Rebase,
    RebaseMerges,
}

impl PullMode {
    fn args(self) -> &'static [&'static str] {
        match self {
            Self::FastForward => &["--ff-only"],
            Self::Rebase => &["--rebase"],
            Self::RebaseMerges => &["--rebase=merges"],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncOutput {
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchInfo {
    pub name: String,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub upstream_gone: bool,
    pub committed_at: Option<i64>,
    pub is_current: bool,
}

pub fn status(repo: &Path) -> Result<BranchStatus, SyncError> {
    let branch = current_branch(repo)?;
    let upstream = branch.as_deref().and_then(|_| upstream_of(repo));
    let (ahead, behind) = match upstream {
        Some(_) => counts(repo)?,
        None => (0, 0),
    };
    Ok(BranchStatus {
        branch,
        upstream,
        ahead,
        behind,
        dirty: is_dirty(repo)?,
        last_fetch: last_fetch(repo),
    })
}

pub fn fetch(repo: &Path) -> Result<SyncOutput, SyncError> {
    require_remote(repo)?;
    let output = exec::git(repo, &["fetch", "--prune"], None)?;
    finish(output)
}

/// Fast-forward unless git config asks for a rebase (`branch.<name>.rebase`, then `pull.rebase`).
pub fn pull_mode(repo: &Path) -> PullMode {
    let branch = current_branch(repo).ok().flatten();
    let setting = branch
        .and_then(|name| config(repo, &format!("branch.{name}.rebase")))
        .or_else(|| config(repo, "pull.rebase"));
    match setting.as_deref() {
        Some("true" | "interactive") => PullMode::Rebase,
        Some("merges") => PullMode::RebaseMerges,
        _ => PullMode::FastForward,
    }
}

pub fn pull(repo: &Path) -> Result<SyncOutput, SyncError> {
    let branch = current_branch(repo)?.ok_or(SyncError::DetachedHead)?;
    if upstream_of(repo).is_none() {
        return Err(SyncError::NoUpstream(branch));
    }
    if is_dirty(repo)? {
        return Err(SyncError::Dirty);
    }
    let mode = pull_mode(repo);
    let mut args = vec!["pull"];
    args.extend(mode.args());
    let output = exec::git(repo, &args, None)?;
    if output.ok {
        return finish(output);
    }
    let text = output.text();
    if mode == PullMode::FastForward {
        if text.contains("fast-forward") || text.contains("divergent") {
            return Err(SyncError::Diverged);
        }
    } else if rebase_in_progress(repo) {
        let _ = exec::git(repo, &["rebase", "--abort"], None);
        return Err(SyncError::Conflict(text));
    }
    Err(SyncError::Git(GitError::Failed(text)))
}

pub fn push(repo: &Path) -> Result<SyncOutput, SyncError> {
    let branch = current_branch(repo)?.ok_or(SyncError::DetachedHead)?;
    let output = if upstream_of(repo).is_some() {
        exec::git(repo, &["push"], None)?
    } else {
        let remote = push_remote(repo, &branch)?;
        exec::git(repo, &["push", "-u", &remote, &branch], None)?
    };
    if !output.ok {
        let text = output.text();
        if text.contains("rejected") || text.contains("non-fast-forward") || text.contains("fetch first") {
            return Err(SyncError::Rejected(text));
        }
        return Err(SyncError::Git(GitError::Failed(text)));
    }
    finish(output)
}

pub fn local_branches(repo: &Path) -> Result<Vec<BranchInfo>, SyncError> {
    let format = "%(refname:short)%09%(upstream:short)%09%(upstream:track,nobracket)%09%(committerdate:unix)%09%(HEAD)";
    let output = exec::git_ok(
        repo,
        &[
            "for-each-ref",
            "--sort=-committerdate",
            &format!("--format={format}"),
            "refs/heads",
        ],
    )?;
    Ok(parse_branches(&output))
}

pub fn parse_branches(output: &str) -> Vec<BranchInfo> {
    output
        .lines()
        .filter_map(|line| {
            let mut fields = line.split('\t');
            let name = fields.next().filter(|name| !name.is_empty())?;
            let upstream = fields.next().filter(|upstream| !upstream.is_empty());
            let track = fields.next().unwrap_or("");
            let committed_at = fields.next().and_then(|time| time.parse().ok());
            let is_current = fields.next() == Some("*");
            let (ahead, behind) = parse_track(track);
            Some(BranchInfo {
                name: name.to_string(),
                upstream: upstream.map(String::from),
                ahead,
                behind,
                upstream_gone: track == "gone",
                committed_at,
                is_current,
            })
        })
        .collect()
}

fn parse_track(track: &str) -> (u32, u32) {
    let (mut ahead, mut behind) = (0, 0);
    for part in track.split(',') {
        let part = part.trim();
        if let Some(count) = part.strip_prefix("ahead ") {
            ahead = count.parse().unwrap_or(0);
        } else if let Some(count) = part.strip_prefix("behind ") {
            behind = count.parse().unwrap_or(0);
        }
    }
    (ahead, behind)
}

fn finish(output: exec::Captured) -> Result<SyncOutput, SyncError> {
    if output.ok {
        Ok(SyncOutput { text: output.text() })
    } else {
        Err(SyncError::Git(GitError::Failed(output.text())))
    }
}

fn current_branch(repo: &Path) -> Result<Option<String>, SyncError> {
    let output = exec::git(repo, &["symbolic-ref", "-q", "--short", "HEAD"], None)?;
    Ok(output
        .ok
        .then(|| output.stdout.trim().to_string())
        .filter(|name| !name.is_empty()))
}

fn upstream_of(repo: &Path) -> Option<String> {
    let output = exec::git(
        repo,
        &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{upstream}"],
        None,
    )
    .ok()?;
    let name = output.stdout.trim();
    (output.ok && !name.is_empty()).then(|| name.to_string())
}

fn counts(repo: &Path) -> Result<(u32, u32), SyncError> {
    let output = exec::git_ok(repo, &["rev-list", "--left-right", "--count", "HEAD...@{upstream}"])?;
    let mut numbers = output.split_whitespace().map(|number| number.parse().unwrap_or(0));
    Ok((numbers.next().unwrap_or(0), numbers.next().unwrap_or(0)))
}

fn is_dirty(repo: &Path) -> Result<bool, SyncError> {
    let output = exec::git_ok(repo, &["status", "--porcelain", "--untracked-files=no"])?;
    Ok(!output.trim().is_empty())
}

fn last_fetch(repo: &Path) -> Option<SystemTime> {
    let path = exec::git_ok(repo, &["rev-parse", "--git-path", "FETCH_HEAD"]).ok()?;
    let path = Path::new(path.trim());
    let full = if path.is_absolute() {
        path.to_path_buf()
    } else {
        repo.join(path)
    };
    full.metadata().ok()?.modified().ok()
}

fn config(repo: &Path, key: &str) -> Option<String> {
    let output = exec::git(repo, &["config", "--get", key], None).ok()?;
    let value = output.stdout.trim();
    (output.ok && !value.is_empty()).then(|| value.to_string())
}

fn remotes(repo: &Path) -> Result<Vec<String>, SyncError> {
    Ok(exec::git_ok(repo, &["remote"])?.lines().map(String::from).collect())
}

fn require_remote(repo: &Path) -> Result<(), SyncError> {
    if remotes(repo)?.is_empty() {
        return Err(SyncError::NoRemote);
    }
    Ok(())
}

fn push_remote(repo: &Path, branch: &str) -> Result<String, SyncError> {
    let remotes = remotes(repo)?;
    let configured = config(repo, &format!("branch.{branch}.pushRemote"))
        .or_else(|| config(repo, "remote.pushDefault"))
        .filter(|name| remotes.contains(name));
    let origin = remotes.iter().find(|name| *name == "origin").cloned();
    configured
        .or(origin)
        .or_else(|| (remotes.len() == 1).then(|| remotes[0].clone()))
        .ok_or(SyncError::NoRemote)
}

fn rebase_in_progress(repo: &Path) -> bool {
    ["rebase-merge", "rebase-apply"].iter().any(|name| {
        exec::git_ok(repo, &["rev-parse", "--git-path", name])
            .ok()
            .is_some_and(|path| {
                let path = Path::new(path.trim());
                let full = if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    repo.join(path)
                };
                full.exists()
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{commit, git, init_repo, write};
    use tempfile::TempDir;

    struct Remote {
        bare: TempDir,
    }

    impl Remote {
        fn new() -> Self {
            let bare = tempfile::tempdir().unwrap();
            git(bare.path(), &["init", "-q", "--bare", "-b", "main"]);
            Self { bare }
        }

        fn url(&self) -> String {
            self.bare.path().to_string_lossy().into_owned()
        }

        fn local(&self) -> TempDir {
            let dir = init_repo();
            git(dir.path(), &["config", "user.name", "t"]);
            git(dir.path(), &["config", "user.email", "t@t"]);
            git(dir.path(), &["remote", "add", "origin", &self.url()]);
            dir
        }

        fn clone_of(&self) -> TempDir {
            let dir = tempfile::tempdir().unwrap();
            git(dir.path(), &["clone", "-q", &self.url(), "."]);
            git(dir.path(), &["config", "user.name", "t"]);
            git(dir.path(), &["config", "user.email", "t@t"]);
            dir
        }
    }

    fn seeded() -> (Remote, TempDir) {
        let remote = Remote::new();
        let local = remote.local();
        write(local.path(), "a.txt", "one\n");
        commit(local.path(), "one");
        (remote, local)
    }

    fn pushed() -> (Remote, TempDir) {
        let (remote, local) = seeded();
        push(local.path()).unwrap();
        (remote, local)
    }

    #[test]
    fn a_branch_without_upstream_has_no_counts() {
        let (_remote, local) = seeded();
        let state = status(local.path()).unwrap();
        assert_eq!(state.branch.as_deref(), Some("main"));
        assert_eq!((state.upstream, state.ahead, state.behind), (None, 0, 0));
        assert_eq!(pull(local.path()), Err(SyncError::NoUpstream("main".into())));
    }

    #[test]
    fn the_first_push_sets_the_upstream_and_later_pushes_clear_ahead() {
        let (remote, local) = seeded();
        push(local.path()).unwrap();
        let state = status(local.path()).unwrap();
        assert_eq!(state.upstream.as_deref(), Some("origin/main"));
        assert_eq!((state.ahead, state.behind), (0, 0));
        assert_eq!(git(remote.bare.path(), &["rev-parse", "main"]).trim().len(), 40);

        write(local.path(), "a.txt", "two\n");
        commit(local.path(), "two");
        write(local.path(), "a.txt", "three\n");
        commit(local.path(), "three");
        assert_eq!(status(local.path()).unwrap().ahead, 2);
        push(local.path()).unwrap();
        assert_eq!(status(local.path()).unwrap().ahead, 0);
    }

    #[test]
    fn fetch_updates_behind_and_the_fetch_time() {
        let (remote, local) = pushed();
        assert!(status(local.path()).unwrap().last_fetch.is_none());
        let other = remote.clone_of();
        write(other.path(), "b.txt", "b\n");
        commit(other.path(), "b");
        push(other.path()).unwrap();

        assert_eq!(status(local.path()).unwrap().behind, 0);
        fetch(local.path()).unwrap();
        let state = status(local.path()).unwrap();
        assert_eq!((state.ahead, state.behind), (0, 1));
        assert!(state.last_fetch.is_some());
    }

    #[test]
    fn pull_fast_forwards() {
        let (remote, local) = pushed();
        let other = remote.clone_of();
        write(other.path(), "b.txt", "b\n");
        commit(other.path(), "b");
        push(other.path()).unwrap();
        fetch(local.path()).unwrap();

        assert_eq!(pull_mode(local.path()), PullMode::FastForward);
        pull(local.path()).unwrap();
        assert!(local.path().join("b.txt").exists());
        assert_eq!(status(local.path()).unwrap().behind, 0);
    }

    fn diverge(remote: &Remote, local: &TempDir, theirs: (&str, &str), mine: (&str, &str)) {
        let other = remote.clone_of();
        write(other.path(), theirs.0, theirs.1);
        commit(other.path(), "theirs");
        push(other.path()).unwrap();
        write(local.path(), mine.0, mine.1);
        commit(local.path(), "mine");
        fetch(local.path()).unwrap();
    }

    #[test]
    fn pull_refuses_to_merge_when_branches_diverged() {
        let (remote, local) = pushed();
        diverge(&remote, &local, ("b.txt", "b\n"), ("c.txt", "c\n"));
        assert_eq!(pull(local.path()), Err(SyncError::Diverged));
        let state = status(local.path()).unwrap();
        assert_eq!((state.ahead, state.behind), (1, 1));
        assert!(!local.path().join("b.txt").exists());
    }

    #[test]
    fn pull_rebases_when_git_config_says_so() {
        let (remote, local) = pushed();
        git(local.path(), &["config", "pull.rebase", "true"]);
        diverge(&remote, &local, ("b.txt", "b\n"), ("c.txt", "c\n"));
        assert_eq!(pull_mode(local.path()), PullMode::Rebase);
        pull(local.path()).unwrap();
        let state = status(local.path()).unwrap();
        assert_eq!((state.ahead, state.behind), (1, 0));
        assert!(local.path().join("b.txt").exists() && local.path().join("c.txt").exists());
    }

    #[test]
    fn a_branch_setting_beats_pull_rebase() {
        let (_remote, local) = pushed();
        git(local.path(), &["config", "pull.rebase", "true"]);
        git(local.path(), &["config", "branch.main.rebase", "merges"]);
        assert_eq!(pull_mode(local.path()), PullMode::RebaseMerges);
        git(local.path(), &["config", "branch.main.rebase", "false"]);
        assert_eq!(pull_mode(local.path()), PullMode::FastForward);
    }

    #[test]
    fn a_conflicting_rebase_is_stopped_and_leaves_the_branch_as_it_was() {
        let (remote, local) = pushed();
        git(local.path(), &["config", "pull.rebase", "true"]);
        diverge(&remote, &local, ("a.txt", "theirs\n"), ("a.txt", "mine\n"));
        let before = git(local.path(), &["rev-parse", "HEAD"]);
        assert!(matches!(pull(local.path()), Err(SyncError::Conflict(_))));
        assert_eq!(git(local.path(), &["rev-parse", "HEAD"]), before);
        assert_eq!(git(local.path(), &["status", "--porcelain"]), "");
        assert_eq!(std::fs::read_to_string(local.path().join("a.txt")).unwrap(), "mine\n");
    }

    #[test]
    fn push_is_rejected_while_behind() {
        let (remote, local) = pushed();
        diverge(&remote, &local, ("b.txt", "b\n"), ("c.txt", "c\n"));
        assert!(matches!(push(local.path()), Err(SyncError::Rejected(_))));
    }

    #[test]
    fn pull_and_push_refuse_unsafe_states() {
        let (_remote, local) = pushed();
        write(local.path(), "a.txt", "dirty\n");
        assert_eq!(pull(local.path()), Err(SyncError::Dirty));
        git(local.path(), &["checkout", "-q", "--detach"]);
        assert_eq!(push(local.path()), Err(SyncError::DetachedHead));
        assert_eq!(pull(local.path()), Err(SyncError::DetachedHead));
    }

    #[test]
    fn fetch_needs_a_remote() {
        let dir = init_repo();
        assert_eq!(fetch(dir.path()), Err(SyncError::NoRemote));
        write(dir.path(), "a.txt", "x\n");
        commit(dir.path(), "x");
        assert_eq!(push(dir.path()), Err(SyncError::NoRemote));
    }

    #[test]
    fn lists_branches_with_their_counts() {
        let (remote, local) = pushed();
        git(local.path(), &["branch", "topic"]);
        diverge(&remote, &local, ("b.txt", "b\n"), ("c.txt", "c\n"));
        let branches = local_branches(local.path()).unwrap();
        let main = branches.iter().find(|branch| branch.name == "main").unwrap();
        assert_eq!((main.ahead, main.behind, main.is_current), (1, 1, true));
        assert_eq!(main.upstream.as_deref(), Some("origin/main"));
        assert!(main.committed_at.is_some());
        let topic = branches.iter().find(|branch| branch.name == "topic").unwrap();
        assert_eq!(
            (topic.upstream.as_deref(), topic.ahead, topic.behind, topic.is_current),
            (None, 0, 0, false)
        );
    }

    #[test]
    fn parses_branch_tracking_text() {
        let parsed = parse_branches(
            "main\torigin/main\tahead 2, behind 3\t1700000000\t*\ngone\torigin/gone\tgone\t1600000000\t \nplain\t\t\t1500000000\t \n",
        );
        assert_eq!((parsed[0].ahead, parsed[0].behind, parsed[0].is_current), (2, 3, true));
        assert!(parsed[1].upstream_gone && parsed[1].ahead == 0);
        assert_eq!(parsed[2].upstream, None);
        assert_eq!(parsed[2].committed_at, Some(1_500_000_000));
    }
}
