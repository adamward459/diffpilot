use super::error::GithubError;
use super::remote::{RepoSlug, choose_pr_remote, parse_remote_url};
use super::types::PullRequest;
use crate::comparison::Comparison;
use crate::exec;
use crate::worktree::{self, Checkout, TempWorktree};
use std::path::Path;

pub fn pr_ref(number: u64) -> String {
    format!("refs/diffpilot/pr/{number}")
}

pub fn pr_remote(repo: &Path) -> Result<(String, String), GithubError> {
    let remotes = exec::git_ok(repo, &["remote"])?;
    let remotes: Vec<String> = remotes.lines().map(String::from).collect();
    let resolved = exec::git(repo, &["config", "--get-regexp", r"^remote\..*\.gh-resolved$"], None)?;
    let name = choose_pr_remote(&remotes, &resolved.stdout).ok_or(GithubError::NoRemote)?;
    let url = exec::git_ok(repo, &["remote", "get-url", &name])?;
    Ok((name, url.trim().to_string()))
}

pub fn repo_slug(repo: &Path, enterprise_hosts: &[String]) -> Result<(String, RepoSlug), GithubError> {
    let (name, url) = pr_remote(repo)?;
    let slug = parse_remote_url(&url, enterprise_hosts).ok_or_else(|| GithubError::NotGithub(name.clone()))?;
    Ok((name, slug))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrReview {
    pub comparison: Comparison,
    pub head_sha: String,
    pub pr_ref: String,
}

fn fetch(repo: &Path, remote: &str, refspec: &str) -> Result<(), GithubError> {
    let output = exec::git(repo, &["fetch", "--quiet", remote, refspec], None)?;
    if output.ok {
        Ok(())
    } else {
        Err(GithubError::FetchFailed(format!("{refspec}: {}", output.text())))
    }
}

fn fetch_base(repo: &Path, remote: &str, base: &str) -> Result<String, GithubError> {
    let tracking = format!("refs/remotes/{remote}/{base}");
    fetch(repo, remote, &format!("+refs/heads/{base}:{tracking}"))?;
    Ok(format!("{remote}/{base}"))
}

fn fetch_pull_head(repo: &Path, remote: &str, number: u64) -> Result<String, GithubError> {
    let target = pr_ref(number);
    fetch(repo, remote, &format!("+refs/pull/{number}/head:{target}"))?;
    Ok(exec::git_ok(repo, &["rev-parse", &target])?.trim().to_string())
}

pub fn review_without_checkout(repo: &Path, remote: &str, pr: &PullRequest) -> Result<PrReview, GithubError> {
    let base = fetch_base(repo, remote, &pr.base_ref)?;
    let head_sha = fetch_pull_head(repo, remote, pr.number)?;
    let pr_ref = pr_ref(pr.number);
    Ok(PrReview {
        comparison: Comparison::branch(&pr_ref, &base),
        head_sha,
        pr_ref,
    })
}

pub fn remove_review_ref(repo: &Path, number: u64) -> Result<(), GithubError> {
    let target = pr_ref(number);
    if exec::git_succeeds(repo, &["rev-parse", "--verify", "-q", &target]) {
        exec::git_ok(repo, &["update-ref", "-d", &target])?;
    }
    Ok(())
}

pub fn review_worktree(repo: &Path, review: &PrReview) -> Result<TempWorktree, GithubError> {
    Ok(TempWorktree::create(repo, &review.head_sha)?)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrBranch {
    pub branch: String,
    pub comparison: Comparison,
}

struct Target {
    local: String,
    start: String,
    track: bool,
}

/// Same-repository pull requests use their own branch. A fork, or a deleted branch, falls back to `pull/<n>/head`.
fn fetch_target(repo: &Path, remote: &str, pr: &PullRequest) -> Result<(Target, String), GithubError> {
    let base = fetch_base(repo, remote, &pr.base_ref)?;
    if !pr.is_cross_repository {
        let tracking = format!("refs/remotes/{remote}/{}", pr.head_ref);
        let refspec = format!("+refs/heads/{}:{tracking}", pr.head_ref);
        if fetch(repo, remote, &refspec).is_ok() {
            let target = Target {
                local: pr.head_ref.clone(),
                start: format!("{remote}/{}", pr.head_ref),
                track: true,
            };
            return Ok((target, base));
        }
    }
    fetch_pull_head(repo, remote, pr.number)?;
    let target = Target {
        local: format!("pr-{}", pr.number),
        start: pr_ref(pr.number),
        track: false,
    };
    Ok((target, base))
}

fn branch_exists(repo: &Path, branch: &str) -> bool {
    exec::git_succeeds(
        repo,
        &["show-ref", "--verify", "--quiet", &format!("refs/heads/{branch}")],
    )
}

fn ensure_free(repo: &Path, branch: &str) -> Result<(), GithubError> {
    let taken = worktree::list(repo)?
        .into_iter()
        .find(|tree| tree.branch.as_deref() == Some(branch));
    match taken {
        Some(tree) => Err(GithubError::InWorktree(tree.path.to_string_lossy().into_owned())),
        None => Ok(()),
    }
}

pub fn checkout_pr(repo: &Path, remote: &str, pr: &PullRequest) -> Result<PrBranch, GithubError> {
    if !exec::git_ok(repo, &["status", "--porcelain", "--untracked-files=no"])?
        .trim()
        .is_empty()
    {
        return Err(GithubError::Dirty);
    }
    let (target, base) = fetch_target(repo, remote, pr)?;
    ensure_free(repo, &target.local)?;
    if branch_exists(repo, &target.local) {
        exec::git_ok(repo, &["checkout", &target.local, "--"])?;
        let _ = exec::git(repo, &["merge", "--ff-only", &target.start], None)?;
    } else {
        let tracking = if target.track { "--track" } else { "--no-track" };
        exec::git_ok(repo, &["checkout", tracking, "-b", &target.local, &target.start])?;
    }
    Ok(PrBranch {
        branch: target.local,
        comparison: Comparison::checked_out(&base, true),
    })
}

pub fn add_pr_worktree(repo: &Path, remote: &str, pr: &PullRequest, dest: &Path) -> Result<PrBranch, GithubError> {
    let (target, base) = fetch_target(repo, remote, pr)?;
    ensure_free(repo, &target.local)?;
    if branch_exists(repo, &target.local) {
        worktree::add(repo, dest, &Checkout::ExistingBranch(target.local.clone()))?;
        let _ = exec::git(dest, &["merge", "--ff-only", &target.start], None)?;
    } else {
        worktree::add(
            repo,
            dest,
            &Checkout::NewBranch {
                name: target.local.clone(),
                start: target.start.clone(),
                track: target.track,
            },
        )?;
    }
    Ok(PrBranch {
        branch: target.local,
        comparison: Comparison::checked_out(&base, true),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::changes;
    use crate::github::types::sample_pr;
    use crate::refs::Refs;
    use crate::testutil::{commit, git, init_repo, write};
    use tempfile::TempDir;

    struct Fixture {
        bare: TempDir,
        local: TempDir,
    }

    fn fixture() -> Fixture {
        let bare = tempfile::tempdir().unwrap();
        git(bare.path(), &["init", "-q", "--bare", "-b", "main"]);
        let local = init_repo();
        let repo = local.path();
        git(repo, &["remote", "add", "origin", &bare.path().to_string_lossy()]);
        write(repo, "a.txt", "base\n");
        commit(repo, "base");
        git(repo, &["push", "-q", "origin", "main"]);
        git(repo, &["checkout", "-q", "-b", "feature"]);
        write(repo, "f.txt", "feature\n");
        commit(repo, "feature");
        git(repo, &["push", "-q", "origin", "feature"]);
        let feature = git(repo, &["rev-parse", "feature"]);
        git(bare.path(), &["update-ref", "refs/pull/1/head", feature.trim()]);

        git(repo, &["checkout", "-q", "-b", "forked", "main"]);
        write(repo, "g.txt", "fork\n");
        commit(repo, "from a fork");
        git(repo, &["push", "-q", "origin", "forked:refs/pull/2/head"]);
        git(repo, &["checkout", "-q", "main"]);
        git(repo, &["branch", "-q", "-D", "forked"]);
        git(repo, &["branch", "-q", "-D", "feature"]);
        git(repo, &["update-ref", "-d", "refs/remotes/origin/feature"]);
        Fixture { bare, local }
    }

    fn paths(repo: &Path, comparison: &Comparison) -> Vec<String> {
        changes::load(repo, comparison)
            .unwrap()
            .into_iter()
            .map(|change| change.path)
            .collect()
    }

    #[test]
    fn reviews_a_pull_request_without_touching_the_checkout() {
        let fixture = fixture();
        let repo = fixture.local.path();
        let before = git(repo, &["rev-parse", "HEAD"]);
        let refs_before = Refs::load(repo);

        let review = review_without_checkout(repo, "origin", &sample_pr(1, "feature")).unwrap();
        assert_eq!(review.pr_ref, "refs/diffpilot/pr/1");
        assert_eq!(
            review.comparison,
            Comparison::branch("refs/diffpilot/pr/1", "origin/main")
        );
        assert_eq!(review.head_sha, git(repo, &["rev-parse", "refs/diffpilot/pr/1"]).trim());
        assert_eq!(paths(repo, &review.comparison), ["f.txt"]);

        assert_eq!(git(repo, &["rev-parse", "HEAD"]), before);
        assert_eq!(git(repo, &["symbolic-ref", "--short", "HEAD"]).trim(), "main");
        assert_eq!(Refs::load(repo).local, refs_before.local);
        assert!(!repo.join("f.txt").exists());

        remove_review_ref(repo, 1).unwrap();
        remove_review_ref(repo, 1).unwrap();
        assert!(git(repo, &["for-each-ref", "refs/diffpilot"]).is_empty());
    }

    #[test]
    fn a_missing_pull_ref_is_a_fetch_failure() {
        let fixture = fixture();
        assert!(matches!(
            review_without_checkout(fixture.local.path(), "origin", &sample_pr(99, "x")),
            Err(GithubError::FetchFailed(_))
        ));
    }

    #[test]
    fn a_review_worktree_has_the_real_files_and_is_removed_after() {
        let fixture = fixture();
        let repo = fixture.local.path();
        let review = review_without_checkout(repo, "origin", &sample_pr(1, "feature")).unwrap();
        let tree = review_worktree(repo, &review).unwrap();
        let path = tree.path().to_path_buf();
        assert_eq!(std::fs::read_to_string(path.join("f.txt")).unwrap(), "feature\n");
        assert!(!repo.join("f.txt").exists());
        tree.remove().unwrap();
        assert!(!path.exists());
        assert_eq!(worktree::list(repo).unwrap().len(), 1);
    }

    #[test]
    fn checks_out_the_branch_of_a_same_repository_pull_request() {
        let fixture = fixture();
        let repo = fixture.local.path();
        let checked = checkout_pr(repo, "origin", &sample_pr(1, "feature")).unwrap();
        assert_eq!(checked.branch, "feature");
        assert_eq!(checked.comparison, Comparison::checked_out("origin/main", true));
        assert_eq!(git(repo, &["symbolic-ref", "--short", "HEAD"]).trim(), "feature");
        assert_eq!(
            git(repo, &["rev-parse", "--abbrev-ref", "feature@{upstream}"]).trim(),
            "origin/feature"
        );
        assert_eq!(paths(repo, &checked.comparison), ["f.txt"]);
    }

    #[test]
    fn checking_out_again_fast_forwards_the_local_branch() {
        let fixture = fixture();
        let repo = fixture.local.path();
        checkout_pr(repo, "origin", &sample_pr(1, "feature")).unwrap();
        git(repo, &["checkout", "-q", "main"]);

        let other = tempfile::tempdir().unwrap();
        git(
            other.path(),
            &[
                "clone",
                "-q",
                "-b",
                "feature",
                &fixture.bare.path().to_string_lossy(),
                ".",
            ],
        );
        write(other.path(), "more.txt", "more\n");
        commit(other.path(), "more");
        git(other.path(), &["push", "-q", "origin", "feature"]);

        checkout_pr(repo, "origin", &sample_pr(1, "feature")).unwrap();
        assert!(repo.join("more.txt").exists());
    }

    #[test]
    fn checkout_refuses_a_dirty_tree_and_a_branch_in_another_worktree() {
        let fixture = fixture();
        let repo = fixture.local.path();
        write(repo, "a.txt", "dirty\n");
        assert_eq!(
            checkout_pr(repo, "origin", &sample_pr(1, "feature")),
            Err(GithubError::Dirty)
        );
        git(repo, &["checkout", "--", "a.txt"]);

        let base = tempfile::tempdir().unwrap();
        let elsewhere = base.path().join("elsewhere");
        add_pr_worktree(repo, "origin", &sample_pr(1, "feature"), &elsewhere).unwrap();
        assert!(matches!(
            checkout_pr(repo, "origin", &sample_pr(1, "feature")),
            Err(GithubError::InWorktree(_))
        ));
    }

    #[test]
    fn a_fork_pull_request_is_checked_out_from_its_pull_ref() {
        let fixture = fixture();
        let repo = fixture.local.path();
        let mut pr = sample_pr(2, "forked");
        pr.is_cross_repository = true;
        let checked = checkout_pr(repo, "origin", &pr).unwrap();
        assert_eq!(checked.branch, "pr-2");
        assert_eq!(git(repo, &["symbolic-ref", "--short", "HEAD"]).trim(), "pr-2");
        assert_eq!(std::fs::read_to_string(repo.join("g.txt")).unwrap(), "fork\n");
    }

    #[test]
    fn a_deleted_head_branch_falls_back_to_the_pull_ref() {
        let fixture = fixture();
        let repo = fixture.local.path();
        let checked = checkout_pr(repo, "origin", &sample_pr(2, "gone-branch")).unwrap();
        assert_eq!(checked.branch, "pr-2");
        assert!(repo.join("g.txt").exists());
    }

    #[test]
    fn a_new_worktree_holds_the_pull_request_and_the_checkout_stays_put() {
        let fixture = fixture();
        let repo = fixture.local.path();
        let base = tempfile::tempdir().unwrap();
        let dest = base.path().join("pr-1");
        let added = add_pr_worktree(repo, "origin", &sample_pr(1, "feature"), &dest).unwrap();
        assert_eq!(added.branch, "feature");
        assert_eq!(std::fs::read_to_string(dest.join("f.txt")).unwrap(), "feature\n");
        assert_eq!(git(&dest, &["symbolic-ref", "--short", "HEAD"]).trim(), "feature");
        assert_eq!(git(repo, &["symbolic-ref", "--short", "HEAD"]).trim(), "main");
        assert!(!repo.join("f.txt").exists());
        worktree::remove(repo, &dest).unwrap();
    }

    #[test]
    fn a_fork_worktree_starts_from_the_pull_ref_without_tracking() {
        let fixture = fixture();
        let repo = fixture.local.path();
        let base = tempfile::tempdir().unwrap();
        let dest = base.path().join("pr-2");
        let mut pr = sample_pr(2, "forked");
        pr.is_cross_repository = true;
        let added = add_pr_worktree(repo, "origin", &pr, &dest).unwrap();
        assert_eq!(added.branch, "pr-2");
        assert!(dest.join("g.txt").exists());
    }

    #[test]
    fn finds_the_pull_request_remote_and_repository() {
        let dir = init_repo();
        let repo = dir.path();
        assert_eq!(pr_remote(repo), Err(GithubError::NoRemote));
        git(repo, &["remote", "add", "origin", "git@github.com:me/app.git"]);
        git(repo, &["remote", "add", "upstream", "https://github.com/team/app"]);
        let (name, slug) = repo_slug(repo, &[]).unwrap();
        assert_eq!((name.as_str(), slug.full_name().as_str()), ("upstream", "team/app"));
        git(repo, &["config", "remote.origin.gh-resolved", "base"]);
        assert_eq!(repo_slug(repo, &[]).unwrap().0, "origin");
        git(repo, &["remote", "set-url", "origin", "git@gitlab.com:me/app.git"]);
        assert_eq!(repo_slug(repo, &[]), Err(GithubError::NotGithub("origin".into())));
    }
}
