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
use diffpilot_core::forge::{
    self, Access, ChangeRequest, Checks, Forge, ForgeError, ForgeKind, JobId, Linked, POLL_INTERVAL,
};
use diffpilot_core::sync::{self, BranchStatus, PullMode};
use gpui::{AppContext, Context, EventEmitter, Task};
use rows::{Words, words};
use std::path::PathBuf;
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
    Synced,
    Review(Comparison),
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
    link: Option<Linked>,
    open_prs: Vec<ChangeRequest>,
    checks: Option<Checks>,
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

    pub fn kind(&self) -> Option<ForgeKind> {
        self.link.as_ref().map(|link| link.forge.kind())
    }

    pub fn words(&self) -> Option<Words> {
        self.kind().map(words)
    }

    pub fn pull_requests(&self) -> &[ChangeRequest] {
        &self.open_prs
    }

    pub fn checks(&self) -> Option<&Checks> {
        self.checks.as_ref()
    }

    pub fn branch_pr(&self) -> Option<&ChangeRequest> {
        let branch = self.branch.as_ref()?.branch.as_deref()?;
        self.open_prs
            .iter()
            .find(|change| change.from_this_repo && change.head_ref == branch)
    }

    pub fn web_url(&self) -> Option<String> {
        match self.branch_pr() {
            Some(change) => Some(change.url.clone()),
            None => self.link.as_ref().map(|link| link.forge.web_url()),
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
            let access = Access {
                http: services.http.clone(),
                credentials: &services.credentials,
                github_host: &services.host,
            };
            let link = forge::connect(&repo, &access).ok()?;
            let changes = link.forge.change_requests().ok()?;
            Some((link, changes))
        });
        self.prs_task = Some(cx.spawn(async move |this, cx| {
            let loaded = task.await;
            this.update(cx, |this, cx| {
                let (link, prs) = match loaded {
                    Some((link, prs)) => (Some(link), prs),
                    None => (None, Vec::new()),
                };
                this.link = link;
                this.open_prs = prs;
                this.sync_checks(cx);
                cx.notify();
            })
            .ok();
        }));
    }

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
        let Some(change) = self.open_prs.iter().find(|change| change.number == number).cloned() else {
            return;
        };
        if matches!(self.review, ReviewState::Running(_)) {
            return;
        }
        self.review = ReviewState::Running(number);
        cx.notify();
        let repo = self.repo.clone();
        let task = cx.background_spawn(async move {
            forge::review_without_checkout(&repo, &link.remote, link.forge.as_ref(), &change)
                .map_err(|error| error.to_string())
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

    pub fn open_on_host(&self, cx: &mut Context<Self>) {
        let Some(url) = self.web_url() else { return };
        open_in_browser(url, cx);
    }

    pub fn rerun_failed(&mut self, cx: &mut Context<Self>) {
        let Some(checks) = self.checks.clone().filter(Checks::can_rerun_failed) else {
            return;
        };
        self.rerun_with(cx, move |forge| forge.rerun_failed(&checks));
    }

    pub fn rerun_job(&mut self, job: JobId, cx: &mut Context<Self>) {
        self.rerun_with(cx, move |forge| forge.rerun_job(job));
    }

    fn rerun_with(
        &mut self,
        cx: &mut Context<Self>,
        work: impl FnOnce(&dyn Forge) -> Result<(), ForgeError> + Send + 'static,
    ) {
        let Some(link) = self.link.clone() else { return };
        if self.rerun == RerunState::Running {
            return;
        }
        self.rerun = RerunState::Running;
        cx.notify();
        let task = cx.background_spawn(async move { work(link.forge.as_ref()).map_err(|error| error.to_string()) });
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

    fn sync_checks(&mut self, cx: &mut Context<Self>) {
        let wanted = self.branch_pr().map(|change| change.head_sha.clone());
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
        let (Some(link), Some(sha)) = (
            self.link.clone(),
            self.branch_pr().map(|change| change.head_sha.clone()),
        ) else {
            self.poll = None;
            return;
        };
        let polled = sha.clone();
        let task = cx.spawn(async move |this, cx| {
            if !settle.is_zero() {
                cx.background_executor().timer(settle).await;
            }
            loop {
                let (forge, sha) = (link.forge.clone(), sha.clone());
                let snapshot = cx.background_spawn(async move { forge.checks(&sha) }).await;
                let finished = this.update(cx, |this, cx| match snapshot {
                    Ok(checks) => {
                        let finished = checks.is_complete();
                        this.checks = Some(checks);
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
