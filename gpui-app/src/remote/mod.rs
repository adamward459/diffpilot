//! Git remote and GitHub state of one repository: ahead and behind counts, fetch, pull and push, the open pull
//! requests, and the checks of the checked-out branch's pull request.

mod capsule;
mod checks_popover;
mod menu;
pub mod rows;
mod sync_popover;

pub use capsule::RemoteCapsule;
pub use menu::RemoteMenu;

use crate::settings;
use diffpilot_core::comparison::Comparison;
use diffpilot_core::editor;
use diffpilot_core::github::checks::{ChecksSnapshot, POLL_INTERVAL};
use diffpilot_core::github::pr_git;
use diffpilot_core::github::remote::RepoSlug;
use diffpilot_core::github::types::PullRequest;
use diffpilot_core::github::{Github, GithubError};
use diffpilot_core::sync::{self, BranchStatus, PullMode};
use gpui::{AppContext, Context, EventEmitter, Task};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

const RERUN_SETTLE: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncKind {
    Fetch,
    Pull,
    Push,
}

impl SyncKind {
    pub fn name(self) -> &'static str {
        match self {
            SyncKind::Fetch => "fetch",
            SyncKind::Pull => "pull",
            SyncKind::Push => "push",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncState {
    Idle,
    Running(SyncKind),
    Failed { kind: SyncKind, message: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RerunState {
    Idle,
    Running,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewState {
    Idle,
    Running(u64),
    Failed { number: u64, message: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum RemoteEvent {
    /// A fetch, pull or push finished. The refs may have changed.
    Synced,
    /// A pull request is fetched and ready to compare with its base.
    Review(Comparison),
}

#[derive(Clone)]
struct GithubLink {
    github: Arc<Github>,
    slug: RepoSlug,
    remote: String,
}

struct ChecksPoll {
    sha: String,
    _task: Task<()>,
}

pub struct RemoteStatus {
    repo: PathBuf,
    branch: Option<BranchStatus>,
    pull_mode: PullMode,
    sync: SyncState,
    rerun: RerunState,
    review: ReviewState,
    link: Option<GithubLink>,
    open_prs: Vec<PullRequest>,
    checks: Option<ChecksSnapshot>,
    poll: Option<ChecksPoll>,
    status_task: Option<Task<()>>,
    prs_task: Option<Task<()>>,
    sync_task: Option<Task<()>>,
    rerun_task: Option<Task<()>>,
    review_task: Option<Task<()>>,
}

impl EventEmitter<RemoteEvent> for RemoteStatus {}

impl RemoteStatus {
    pub fn new(repo: PathBuf, cx: &mut Context<Self>) -> Self {
        cx.observe_global::<settings::SettingsChanged>(|this, cx| {
            if this.link.is_none() {
                this.refresh_pull_requests(cx);
            }
        })
        .detach();
        Self {
            repo,
            branch: None,
            pull_mode: PullMode::FastForward,
            sync: SyncState::Idle,
            rerun: RerunState::Idle,
            review: ReviewState::Idle,
            link: None,
            open_prs: Vec::new(),
            checks: None,
            poll: None,
            status_task: None,
            prs_task: None,
            sync_task: None,
            rerun_task: None,
            review_task: None,
        }
    }

    pub fn branch(&self) -> Option<&BranchStatus> {
        self.branch.as_ref()
    }

    pub fn pull_mode(&self) -> PullMode {
        self.pull_mode
    }

    pub fn sync(&self) -> &SyncState {
        &self.sync
    }

    pub fn rerun_state(&self) -> &RerunState {
        &self.rerun
    }

    pub fn review_state(&self) -> &ReviewState {
        &self.review
    }

    pub fn is_github(&self) -> bool {
        self.link.is_some()
    }

    pub fn pull_requests(&self) -> &[PullRequest] {
        &self.open_prs
    }

    pub fn checks(&self) -> Option<&ChecksSnapshot> {
        self.checks.as_ref()
    }

    /// The open pull request whose head is the checked-out branch.
    pub fn branch_pr(&self) -> Option<&PullRequest> {
        let link = self.link.as_ref()?;
        let branch = self.branch.as_ref()?.branch.as_deref()?;
        let repo = link.slug.full_name();
        self.open_prs.iter().find(|pr| {
            pr.head_ref == branch
                && pr
                    .head_repo
                    .as_deref()
                    .is_some_and(|head| head.eq_ignore_ascii_case(&repo))
        })
    }

    pub fn github_url(&self) -> Option<String> {
        match self.branch_pr() {
            Some(pr) => Some(pr.url.clone()),
            None => self.link.as_ref().map(|link| link.slug.web_url()),
        }
    }

    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.refresh_status(cx);
        self.refresh_pull_requests(cx);
    }

    pub fn refresh_status(&mut self, cx: &mut Context<Self>) {
        let repo = self.repo.clone();
        let task = cx.background_spawn(async move { (sync::status(&repo).ok(), sync::pull_mode(&repo)) });
        self.status_task = Some(cx.spawn(async move |this, cx| {
            let (status, mode) = task.await;
            this.update(cx, |this, cx| {
                this.branch = status;
                this.pull_mode = mode;
                this.sync_checks(cx);
                cx.notify();
            })
            .ok();
        }));
    }

    fn refresh_pull_requests(&mut self, cx: &mut Context<Self>) {
        let repo = self.repo.clone();
        let services = settings::services(cx);
        let task = cx.background_spawn(async move {
            let services = services?;
            let hosts = [services.host.clone()];
            let (remote, slug) = pr_git::repo_slug(&repo, &hosts).ok()?;
            let github = Github::connect(services.http.clone(), &services.credentials, &services.host).ok()?;
            let prs = github.list_pull_requests(&slug).ok()?;
            let link = GithubLink {
                github: Arc::new(github),
                slug,
                remote,
            };
            Some((link, prs))
        });
        self.prs_task = Some(cx.spawn(async move |this, cx| {
            let loaded = task.await;
            this.update(cx, |this, cx| {
                let (link, prs) = match loaded {
                    Some((link, prs)) => (Some(link), prs),
                    None => (None, Vec::new()),
                };
                this.link = link;
                this.open_prs = prs.into_iter().filter(PullRequest::is_open).collect();
                this.sync_checks(cx);
                cx.notify();
            })
            .ok();
        }));
    }

    /// Runs one operation at a time. A second request while one runs is ignored.
    pub fn run(&mut self, kind: SyncKind, cx: &mut Context<Self>) {
        if matches!(self.sync, SyncState::Running(_)) {
            return;
        }
        self.sync = SyncState::Running(kind);
        cx.notify();
        let repo = self.repo.clone();
        let task = cx.background_spawn(async move {
            match kind {
                SyncKind::Fetch => sync::fetch(&repo),
                SyncKind::Pull => sync::pull(&repo),
                SyncKind::Push => sync::push(&repo),
            }
            .map(|_| ())
            .map_err(|error| error.to_string())
        });
        self.sync_task = Some(cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                this.sync = match result {
                    Ok(()) => SyncState::Idle,
                    Err(message) => SyncState::Failed { kind, message },
                };
                this.refresh_status(cx);
                cx.emit(RemoteEvent::Synced);
                cx.notify();
            })
            .ok();
        }));
    }

    pub fn review(&mut self, number: u64, cx: &mut Context<Self>) {
        let Some(link) = self.link.clone() else { return };
        let Some(pr) = self.open_prs.iter().find(|pr| pr.number == number).cloned() else {
            return;
        };
        if matches!(self.review, ReviewState::Running(_)) {
            return;
        }
        self.review = ReviewState::Running(number);
        cx.notify();
        let repo = self.repo.clone();
        let task = cx.background_spawn(async move {
            pr_git::review_without_checkout(&repo, &link.remote, &pr).map_err(|error| error.to_string())
        });
        self.review_task = Some(cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(review) => {
                        this.review = ReviewState::Idle;
                        cx.emit(RemoteEvent::Review(review.comparison));
                    }
                    Err(message) => this.review = ReviewState::Failed { number, message },
                }
                cx.notify();
            })
            .ok();
        }));
    }

    pub fn open_on_github(&self, cx: &mut Context<Self>) {
        let Some(url) = self.github_url() else { return };
        open_in_browser(url, cx);
    }

    pub fn rerun_failed(&mut self, cx: &mut Context<Self>) {
        let ids = self
            .checks
            .as_ref()
            .map(ChecksSnapshot::failed_run_ids)
            .unwrap_or_default();
        if ids.is_empty() {
            return;
        }
        self.rerun_with(cx, move |github, slug| {
            for id in ids {
                github.rerun_failed_jobs(slug, id)?;
            }
            Ok(())
        });
    }

    pub fn rerun_job(&mut self, job_id: u64, cx: &mut Context<Self>) {
        self.rerun_with(cx, move |github, slug| github.rerun_job(slug, job_id).map(|_| ()));
    }

    fn rerun_with(
        &mut self,
        cx: &mut Context<Self>,
        work: impl FnOnce(&Github, &RepoSlug) -> Result<(), GithubError> + Send + 'static,
    ) {
        let Some(link) = self.link.clone() else { return };
        if self.rerun == RerunState::Running {
            return;
        }
        self.rerun = RerunState::Running;
        cx.notify();
        let task =
            cx.background_spawn(async move { work(&link.github, &link.slug).map_err(|error| error.to_string()) });
        self.rerun_task = Some(cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(()) => {
                        this.rerun = RerunState::Idle;
                        this.restart_poll(RERUN_SETTLE, cx);
                    }
                    Err(message) => this.rerun = RerunState::Failed(message),
                }
                cx.notify();
            })
            .ok();
        }));
    }

    /// Follows the head of the branch's pull request. A new head starts a new poll.
    fn sync_checks(&mut self, cx: &mut Context<Self>) {
        let wanted = self.branch_pr().map(|pr| pr.head_sha.clone());
        if self.poll.as_ref().map(|poll| &poll.sha) == wanted.as_ref() {
            return;
        }
        self.poll = None;
        self.checks = None;
        if wanted.is_some() {
            self.restart_poll(Duration::ZERO, cx);
        }
    }

    /// Polls until every check finishes. The first fetch waits `settle`, so a re-run has time to start.
    fn restart_poll(&mut self, settle: Duration, cx: &mut Context<Self>) {
        let (Some(link), Some(sha)) = (self.link.clone(), self.branch_pr().map(|pr| pr.head_sha.clone())) else {
            self.poll = None;
            return;
        };
        let polled = sha.clone();
        let task = cx.spawn(async move |this, cx| {
            if !settle.is_zero() {
                cx.background_executor().timer(settle).await;
            }
            loop {
                let (github, slug, sha) = (link.github.clone(), link.slug.clone(), sha.clone());
                let snapshot = cx.background_spawn(async move { github.checks(&slug, &sha) }).await;
                let finished = this.update(cx, |this, cx| match snapshot {
                    Ok(snapshot) => {
                        let finished = snapshot.is_complete();
                        this.checks = Some(snapshot);
                        cx.notify();
                        finished
                    }
                    Err(_) => false,
                });
                if finished.unwrap_or(true) {
                    break;
                }
                cx.background_executor().timer(POLL_INTERVAL).await;
            }
        });
        self.poll = Some(ChecksPoll {
            sha: polled,
            _task: task,
        });
    }
}

pub fn open_in_browser(url: String, cx: &mut gpui::App) {
    cx.background_spawn(async move {
        if let Err(error) = editor::open_url(&url) {
            eprintln!("diffpilot: {error}");
        }
    })
    .detach();
}
