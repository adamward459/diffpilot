mod github;

use crate::github::Github;
use crate::github::auth::Credentials;
use crate::github::error::GithubError;
use crate::github::http::Http;
use crate::github::pr_git;
use crate::github::remote::{parse_remote_url, split_remote_url};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

pub use crate::github::pr_git::{PrReview, pr_number, pr_ref};
pub use github::GithubForge;

pub const POLL_INTERVAL: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForgeKind {
    GitHub,
    GitLab,
}

impl ForgeKind {
    pub fn name(self) -> &'static str {
        match self {
            ForgeKind::GitHub => "GitHub",
            ForgeKind::GitLab => "GitLab",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ForgeError {
    #[error("{} is not supported yet", .0.name())]
    Unsupported(ForgeKind),
    #[error("the `{0}` remote is not on a supported forge")]
    UnknownHost(String),
    #[error(transparent)]
    Github(#[from] GithubError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Review {
    None,
    Requested,
    Approved,
    ChangesRequested,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Outcome {
    Queued,
    Running,
    Passed,
    Failed,
    Skipped,
    Cancelled,
    ActionRequired,
}

impl Outcome {
    pub fn is_finished(self) -> bool {
        !matches!(self, Self::Queued | Self::Running)
    }
}

/// A GitHub pull request or a GitLab merge request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeRequest {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub author: String,
    pub head_ref: String,
    pub base_ref: String,
    pub head_sha: String,
    pub from_this_repo: bool,
    pub review: Review,
    pub requested_from_you: bool,
    pub checks: Option<Outcome>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct JobId(pub u64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub name: String,
    pub outcome: Outcome,
    pub duration_seconds: Option<u64>,
    pub log_url: Option<String>,
    pub rerun: Option<JobId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stage {
    pub name: String,
    pub jobs: Vec<Job>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Counts {
    pub passed: u32,
    pub failed: u32,
    pub running: u32,
    pub queued: u32,
    pub other: u32,
}

impl Counts {
    pub fn total(&self) -> u32 {
        self.passed + self.failed + self.running + self.queued + self.other
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overall {
    None,
    Passing,
    Pending,
    Failing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checks {
    pub sha: String,
    pub stages: Vec<Stage>,
    /// What the forge needs to re-run the failed jobs together: GitHub workflow runs, GitLab pipelines.
    pub failed_runs: Vec<u64>,
}

impl Checks {
    fn outcomes(&self) -> impl Iterator<Item = Outcome> + '_ {
        self.stages.iter().flat_map(|stage| &stage.jobs).map(|job| job.outcome)
    }

    pub fn counts(&self) -> Counts {
        let mut counts = Counts::default();
        for outcome in self.outcomes() {
            match outcome {
                Outcome::Passed => counts.passed += 1,
                Outcome::Failed => counts.failed += 1,
                Outcome::Running => counts.running += 1,
                Outcome::Queued => counts.queued += 1,
                _ => counts.other += 1,
            }
        }
        counts
    }

    /// True when every job has finished. A commit without checks counts as finished.
    pub fn is_complete(&self) -> bool {
        self.outcomes().all(Outcome::is_finished)
    }

    pub fn overall(&self) -> Overall {
        let counts = self.counts();
        if counts.total() == 0 {
            Overall::None
        } else if counts.failed > 0 {
            Overall::Failing
        } else if !self.is_complete() {
            Overall::Pending
        } else {
            Overall::Passing
        }
    }

    pub fn can_rerun_failed(&self) -> bool {
        !self.failed_runs.is_empty()
    }
}

pub trait Forge: Send + Sync {
    fn kind(&self) -> ForgeKind;
    fn web_url(&self) -> String;
    fn change_requests(&self) -> Result<Vec<ChangeRequest>, ForgeError>;
    fn checks(&self, sha: &str) -> Result<Checks, ForgeError>;
    fn rerun_failed(&self, checks: &Checks) -> Result<(), ForgeError>;
    fn rerun_job(&self, job: JobId) -> Result<(), ForgeError>;
    fn head_refspec(&self, number: u64) -> String;
}

#[derive(Clone)]
pub struct Linked {
    pub remote: String,
    pub forge: Arc<dyn Forge>,
}

pub struct Access<'a> {
    pub http: Arc<dyn Http>,
    pub credentials: &'a Credentials,
    pub github_host: &'a str,
}

fn kind_of_host(host: &str) -> Option<ForgeKind> {
    let host = host.to_ascii_lowercase();
    (host == "gitlab.com" || host.starts_with("gitlab.")).then_some(ForgeKind::GitLab)
}

pub fn connect(repo: &Path, access: &Access) -> Result<Linked, ForgeError> {
    let (remote, url) = pr_git::pr_remote(repo)?;
    let github_hosts = [access.github_host.to_string()];
    if let Some(slug) = parse_remote_url(&url, &github_hosts) {
        let github = Github::connect(access.http.clone(), access.credentials, &slug.host)?;
        return Ok(Linked {
            remote,
            forge: Arc::new(GithubForge::new(github, slug)),
        });
    }
    match split_remote_url(&url).and_then(|(host, _)| kind_of_host(host)) {
        Some(kind) => Err(ForgeError::Unsupported(kind)),
        None => Err(ForgeError::UnknownHost(remote)),
    }
}

pub fn review_without_checkout(
    repo: &Path,
    remote: &str,
    forge: &dyn Forge,
    change: &ChangeRequest,
) -> Result<PrReview, ForgeError> {
    Ok(pr_git::review_without_checkout(
        repo,
        remote,
        &change.base_ref,
        change.number,
        &forge.head_refspec(change.number),
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::auth::{GhCli, MemoryStore};
    use crate::testutil::{git, init_repo};

    struct NoGh;

    impl GhCli for NoGh {
        fn token(&self, _host: &str) -> Option<String> {
            None
        }
    }

    fn connect_to(url: &str, host: &str) -> Result<Linked, ForgeError> {
        let dir = init_repo();
        git(dir.path(), &["remote", "add", "origin", url]);
        let credentials = Credentials::new(Arc::new(MemoryStore::default()), Arc::new(NoGh));
        credentials.save("github.com", "ghp_x").unwrap();
        let access = Access {
            http: Arc::new(crate::github::http::fake::FakeHttp::new()),
            credentials: &credentials,
            github_host: host,
        };
        connect(dir.path(), &access)
    }

    #[test]
    fn a_github_remote_connects_to_github() {
        let linked = connect_to("git@github.com:o/r.git", "github.com").ok().unwrap();
        assert_eq!(linked.remote, "origin");
        assert_eq!(linked.forge.kind(), ForgeKind::GitHub);
        assert_eq!(linked.forge.web_url(), "https://github.com/o/r");
    }

    #[test]
    fn a_gitlab_remote_is_not_supported_yet() {
        for url in ["git@gitlab.com:o/r.git", "https://gitlab.example.org/o/r"] {
            assert_eq!(
                connect_to(url, "github.com").err(),
                Some(ForgeError::Unsupported(ForgeKind::GitLab)),
                "{url}"
            );
        }
    }

    #[test]
    fn an_unknown_host_or_a_missing_token_does_not_connect() {
        assert_eq!(
            connect_to("git@example.com:o/r.git", "github.com").err(),
            Some(ForgeError::UnknownHost("origin".into()))
        );
        assert_eq!(
            connect_to("git@git.corp.example:o/r.git", "git.corp.example").err(),
            Some(ForgeError::Github(GithubError::NoToken("git.corp.example".into())))
        );
    }

    #[test]
    fn the_overall_state_follows_the_jobs() {
        let job = |outcome| Job {
            name: "j".into(),
            outcome,
            duration_seconds: None,
            log_url: None,
            rerun: None,
        };
        let checks = |outcomes: &[Outcome]| Checks {
            sha: "s".into(),
            stages: vec![Stage {
                name: "s".into(),
                jobs: outcomes.iter().copied().map(job).collect(),
            }],
            failed_runs: Vec::new(),
        };
        assert_eq!(checks(&[]).overall(), Overall::None);
        assert!(checks(&[]).is_complete());
        assert_eq!(checks(&[Outcome::Passed, Outcome::Skipped]).overall(), Overall::Passing);
        assert_eq!(checks(&[Outcome::Passed, Outcome::Running]).overall(), Overall::Pending);
        assert_eq!(checks(&[Outcome::Failed, Outcome::Running]).overall(), Overall::Failing);
        assert_eq!(
            checks(&[
                Outcome::Passed,
                Outcome::Failed,
                Outcome::Running,
                Outcome::Queued,
                Outcome::Cancelled
            ])
            .counts(),
            Counts {
                passed: 1,
                failed: 1,
                running: 1,
                queued: 1,
                other: 1
            }
        );
    }
}
