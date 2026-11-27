use crate::claude_activity::ClaudeActivity;
use crate::runs::Runs;
use diffpilot_core::comparison::Comparison;
use diffpilot_core::diff::FileDiff;
use diffpilot_core::fix::{self, Target};
use diffpilot_core::policy::EditPermission;
use diffpilot_core::refs::Refs;
use diffpilot_core::review::{ReviewModel, ReviewSetup, SeverityCounts, plan_threads, review_title};
use diffpilot_core::run::{Run, RunId, RunKind, RunOutcome, RunState, RunSummary};
use diffpilot_core::runner::{RunEngine, RunEvent, RunRequest};
use diffpilot_core::settings::Settings;
use diffpilot_core::store::{SessionId, SessionKey, Store};
use diffpilot_core::threads::{
    CLAUDE_NAME, ReviewThread, ThreadAnchor, ThreadId, ThreadStatus, ThreadStore, mentions_claude,
};
use gpui::{App, Context, Entity, EventEmitter, Subscription};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

pub const NO_REPO_MESSAGE: &str = "Open a Git repository first.";
pub const FIX_NEEDS_DONT_ASK: &str = "Fix needs the Don't ask edit setting for now.";
pub const FIX_NEEDS_WORKING_TREE: &str =
    "Fix edits your working tree. Pick a comparison that includes uncommitted changes.";
pub const NOTHING_TO_FIX_MESSAGE: &str = "There is no open comment for Claude to fix.";
pub const RUNS_UNAVAILABLE: &str = "Claude runs are not available here.";
pub const NO_REPLY_MESSAGE: &str = "Claude did not reply. Open the Claude area to see what happened, then ask again.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadChange {
    Added(ThreadId),
    Updated(ThreadId),
    Removed(ThreadId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewEvent {
    ThreadsChanged(ThreadChange),
    RunChanged,
    RunFinished(RunState),
}

#[derive(Debug, Clone)]
pub struct ReviewHost {
    pub repo: Option<PathBuf>,
    pub comparison: Comparison,
    pub branch_name: Option<String>,
    pub files: Arc<[Arc<FileDiff>]>,
    pub visible_path: Option<String>,
}

impl ReviewHost {
    pub fn new(repo: Option<PathBuf>, comparison: Comparison) -> Self {
        Self {
            repo,
            comparison,
            branch_name: None,
            files: Arc::from([]),
            visible_path: None,
        }
    }
}

pub struct ReviewSession {
    host: ReviewHost,
    settings: Arc<Settings>,
    store: ThreadStore,
    setup: ReviewSetup,
    review_threads: Vec<ThreadId>,
    /// Reply runs of this session, with their thread and how many comments it had when the run was asked for.
    asks: HashMap<RunId, (ThreadId, usize)>,
    engine: Option<RunEngine>,
    activity: Entity<ClaudeActivity>,
    /// The last review and fix runs this session started.
    review_run: Option<RunId>,
    fix_run: Option<RunId>,
    /// The newest run on this branch, for the status line before any run goes in the repo.
    last: Option<RunSummary>,
    submitted: HashSet<RunId>,
    replacing: HashMap<RunId, Vec<ThreadId>>,
    applied: HashSet<RunId>,
    review_failure: Option<String>,
    fix_failure: Option<String>,
    _activity: Vec<Subscription>,
}

impl EventEmitter<ReviewEvent> for ReviewSession {}

impl Drop for ReviewSession {
    fn drop(&mut self) {
        if let Some(engine) = &self.engine {
            for id in &self.submitted {
                engine.stop(*id);
            }
        }
    }
}

impl ReviewSession {
    pub fn new(host: ReviewHost, settings: Arc<Settings>, store: Arc<Store>, cx: &mut Context<Self>) -> Self {
        let setup = Self::saved_setup(&settings, host.repo.as_deref());
        let threads = ThreadStore::open(store, &Self::thread_key(&host)).unwrap_or_default();
        let review_threads = threads
            .threads()
            .filter(|thread| thread.finding.is_some() && thread.status == ThreadStatus::Open)
            .map(|thread| thread.id)
            .collect();
        let activity = Self::activity_of(&host, cx);
        let mut this = Self {
            host,
            settings,
            store: threads,
            setup,
            review_threads,
            asks: HashMap::new(),
            engine: None,
            activity: activity.clone(),
            review_run: None,
            fix_run: None,
            last: None,
            submitted: HashSet::new(),
            replacing: HashMap::new(),
            applied: HashSet::new(),
            review_failure: None,
            fix_failure: None,
            _activity: Vec::new(),
        };
        this.last = activity.read(cx).past_runs(&this.session_key(), 1).into_iter().next();
        this.watch_activity(activity, cx);
        this
    }

    pub fn last_run(&self) -> Option<&RunSummary> {
        self.last.as_ref()
    }

    /// The Claude runs of the repo.
    pub fn activity(&self) -> &Entity<ClaudeActivity> {
        &self.activity
    }

    fn activity_of(host: &ReviewHost, cx: &mut App) -> Entity<ClaudeActivity> {
        ClaudeActivity::for_repo(host.repo.as_deref().unwrap_or(std::path::Path::new("")), cx)
    }

    fn watch_activity(&mut self, activity: Entity<ClaudeActivity>, cx: &mut Context<Self>) {
        self._activity = vec![
            cx.observe(&activity, |_, _, cx| cx.notify()),
            cx.subscribe(&activity, |this, activity, event: &RunEvent, cx| {
                this.run_event(&activity, event, cx)
            }),
        ];
        self.activity = activity;
    }

    fn global_engine(cx: &App) -> Option<RunEngine> {
        cx.try_global::<Runs>().map(|runs| runs.engine().clone())
    }

    fn thread_key(host: &ReviewHost) -> SessionKey {
        let Some(repo) = host.repo.as_deref() else {
            return SessionKey::new("", "", "");
        };
        let refs = Refs {
            checked_out: host.branch_name.clone(),
            ..Refs::default()
        };
        SessionKey::for_comparison(repo, &host.comparison, Some(&refs))
    }

    fn saved_setup(settings: &Settings, repo: Option<&std::path::Path>) -> ReviewSetup {
        match repo {
            Some(repo) => ReviewSetup::load(settings, repo),
            None => ReviewSetup {
                model: Some(settings.reviews_model()),
                ..ReviewSetup::default()
            },
        }
    }

    pub fn setup(&self) -> &ReviewSetup {
        &self.setup
    }

    pub fn host(&self) -> &ReviewHost {
        &self.host
    }

    pub fn session_key(&self) -> SessionKey {
        Self::thread_key(&self.host)
    }

    pub fn session_id(&self) -> SessionId {
        self.store.session()
    }

    pub fn thread(&self, id: ThreadId) -> Option<&ReviewThread> {
        self.store.get(id)
    }

    pub fn threads(&self) -> impl Iterator<Item = &ReviewThread> {
        self.store
            .threads()
            .filter(|thread| thread.status != ThreadStatus::Resolved)
    }

    /// Every thread of this view, resolved and dismissed ones included.
    pub fn all_threads(&self) -> impl Iterator<Item = &ReviewThread> {
        self.store.threads()
    }

    fn open_threads(&self) -> impl Iterator<Item = &ReviewThread> {
        self.store
            .threads()
            .filter(|thread| thread.status == ThreadStatus::Open)
    }

    pub fn thread_count(&self) -> usize {
        self.open_threads().count()
    }

    pub fn comment_counts(&self) -> std::collections::BTreeMap<String, usize> {
        let mut counts = std::collections::BTreeMap::new();
        for thread in self.open_threads() {
            *counts.entry(thread.anchor.path().to_string()).or_insert(0) += 1;
        }
        counts
    }

    /// A reply run for the thread is waiting or going.
    pub fn is_replying(&self, id: ThreadId, cx: &App) -> bool {
        !self.reply_runs(id, cx).is_empty()
    }

    fn reply_runs(&self, id: ThreadId, cx: &App) -> Vec<RunId> {
        self.live_runs(cx)
            .filter(|run| run.kind == RunKind::Reply && run.threads.contains(&id))
            .map(|run| run.id)
            .collect()
    }

    /// This session's runs that are going or waiting.
    fn live_runs<'a>(&'a self, cx: &'a App) -> impl Iterator<Item = &'a Run> {
        let session = self.store.session();
        self.activity.read(cx).live().filter(move |run| run.session == session)
    }

    /// The repo's current run, when this session started it.
    fn own_current<'a>(&self, cx: &'a App) -> Option<&'a Run> {
        let session = self.store.session();
        self.activity.read(cx).current().filter(|run| run.session == session)
    }

    pub fn file_count(&self) -> usize {
        self.host.files.len()
    }

    pub fn title(&self) -> String {
        review_title(
            &self.host.comparison,
            self.host.branch_name.as_deref(),
            self.file_count(),
        )
    }

    pub fn set_visible_path(&mut self, path: Option<String>, cx: &mut Context<Self>) {
        if self.host.visible_path != path {
            self.host.visible_path = path;
            cx.notify();
        }
    }

    pub fn set_files(&mut self, files: Arc<[Arc<FileDiff>]>, cx: &mut Context<Self>) {
        self.host.files = files;
        if !self.host.files.is_empty() && !self.store.is_empty() {
            let files: Vec<FileDiff> = self.host.files.iter().map(|file| (**file).clone()).collect();
            let before = self.visible();
            self.store.reanchor(&files);
            self.announce_changes(before, cx);
        }
        cx.notify();
    }

    /// Picks up what the run engine or the MCP tools wrote to this review since the last read.
    pub fn refresh_threads(&mut self, cx: &mut Context<Self>) {
        let before = self.visible();
        if self.store.refresh() {
            self.announce_changes(before, cx);
        }
    }

    fn visible(&self) -> HashMap<ThreadId, ReviewThread> {
        self.threads().map(|thread| (thread.id, thread.clone())).collect()
    }

    fn announce_changes(&mut self, before: HashMap<ThreadId, ReviewThread>, cx: &mut Context<Self>) {
        let after = self.visible();
        let mut changes: Vec<ThreadChange> = before
            .keys()
            .filter(|id| !after.contains_key(id))
            .map(|id| ThreadChange::Removed(*id))
            .collect();
        for (id, thread) in &after {
            match before.get(id) {
                None => changes.push(ThreadChange::Added(*id)),
                Some(old) if old != thread => changes.push(ThreadChange::Updated(*id)),
                Some(_) => {}
            }
        }
        changes.sort_by_key(|change| match change {
            ThreadChange::Added(id) | ThreadChange::Updated(id) | ThreadChange::Removed(id) => id.0,
        });
        for change in changes {
            cx.emit(ReviewEvent::ThreadsChanged(change));
        }
        cx.notify();
    }

    pub fn set_comparison(&mut self, comparison: Comparison, cx: &mut Context<Self>) {
        if self.host.comparison != comparison {
            self.host.comparison = comparison;
            cx.notify();
        }
    }

    pub fn set_branch_name(&mut self, name: Option<String>, cx: &mut Context<Self>) {
        if self.host.branch_name != name {
            self.host.branch_name = name;
            cx.notify();
        }
    }

    pub fn set_repo(&mut self, repo: Option<PathBuf>, cx: &mut Context<Self>) {
        if self.host.repo != repo {
            self.setup = Self::saved_setup(&self.settings, repo.as_deref());
            self.host.repo = repo;
            let activity = Self::activity_of(&self.host, cx);
            self.watch_activity(activity, cx);
            cx.notify();
        }
    }

    pub fn update_setup(&mut self, change: impl FnOnce(&mut ReviewSetup), cx: &mut Context<Self>) {
        let before = self.setup.clone();
        change(&mut self.setup);
        if self.setup == before {
            return;
        }
        if self.setup.model != before.model
            && let Some(model) = self.setup.model
        {
            self.settings.set_reviews_model(model);
        }
        cx.notify();
    }

    pub fn use_settings(&mut self, cx: &mut Context<Self>) {
        let model = Some(self.settings.reviews_model());
        if self.setup.model != model {
            self.setup.model = model;
            cx.notify();
        }
    }

    pub fn model(&self) -> ReviewModel {
        self.setup.model.unwrap_or(ReviewModel::STANDARD)
    }

    pub fn start_review(&mut self, cx: &mut Context<Self>) {
        let Some(repo) = self.host.repo.clone() else {
            return self.fail_review(NO_REPO_MESSAGE.into(), cx);
        };
        let Some(engine) = Self::global_engine(cx) else {
            return self.fail_review(RUNS_UNAVAILABLE.into(), cx);
        };
        if let Some(reason) = cx.global::<Runs>().unavailable() {
            return self.fail_review(reason.into(), cx);
        }
        self.setup.save(&self.settings, &repo);
        let request = self
            .setup
            .request(&repo, &self.host.comparison, self.host.visible_path.as_deref());
        let request = engine.review_request(&request, self.session_key(), &self.title());
        match self.submit(&engine, request, cx) {
            Ok(run) => {
                self.review_run = Some(run);
                self.review_failure = None;
                self.announce_run(cx);
            }
            Err(message) => self.fail_review(message, cx),
        }
    }

    fn fail_review(&mut self, message: String, cx: &mut Context<Self>) {
        self.review_failure = Some(message);
        self.announce_run(cx);
        cx.emit(ReviewEvent::RunFinished(RunState::Failed));
    }

    fn submit(&mut self, engine: &RunEngine, request: RunRequest, cx: &mut Context<Self>) -> Result<RunId, String> {
        if self.activity.read(cx).engine().is_none() {
            let activity = Self::activity_of(&self.host, cx);
            self.watch_activity(activity, cx);
        }
        self.engine = Some(engine.clone());
        let id = engine.submit(request).map_err(|error| error.to_string())?;
        self.submitted.insert(id);
        Ok(id)
    }

    /// Starts the repo's last run again, when this session started it and it was a review, a fix or a reply.
    pub fn run_again(&mut self, cx: &mut Context<Self>) {
        let Some(run) = self.own_current(cx).filter(|run| run.state.is_finished()).cloned() else {
            return;
        };
        match run.kind {
            RunKind::Review => self.start_review(cx),
            RunKind::Reply => {
                if let Some(id) = run.threads.first().copied() {
                    self.ask_claude(id, cx);
                }
            }
            RunKind::FixComments => {
                let open = self.fixable(cx);
                let again: Vec<ThreadId> = run.threads.iter().copied().filter(|id| open.contains(id)).collect();
                if again.is_empty() {
                    return self.fail_fix(NOTHING_TO_FIX_MESSAGE.into(), cx);
                }
                self.fix_threads(&again, cx);
            }
            _ => {}
        }
    }

    pub fn can_run_again(&self, cx: &App) -> bool {
        self.own_current(cx).is_some_and(|run| {
            run.state.is_finished() && matches!(run.kind, RunKind::Review | RunKind::FixComments | RunKind::Reply)
        })
    }

    /// The repo's current run, when it is the last review this session started.
    pub fn review_run<'a>(&self, cx: &'a App) -> Option<&'a Run> {
        self.own_current(cx).filter(|run| Some(run.id) == self.review_run)
    }

    pub fn reviewing(&self, cx: &App) -> bool {
        self.review_run
            .is_some_and(|id| self.live_runs(cx).any(|run| run.id == id))
    }

    pub fn review_problem(&self, cx: &App) -> Option<String> {
        if let Some(message) = &self.review_failure {
            return Some(message.clone());
        }
        let run = self.review_run(cx).filter(|run| run.state == RunState::Failed)?;
        Some(
            run.error
                .clone()
                .unwrap_or_else(|| "Claude could not finish the review.".into()),
        )
    }

    pub fn review_counts(&self) -> SeverityCounts {
        let mut counts = SeverityCounts::new();
        for id in &self.review_threads {
            if let Some(finding) = self.store.get(*id).and_then(|thread| thread.finding.as_ref()) {
                *counts.entry(finding.severity).or_insert(0) += 1;
            }
        }
        counts
    }

    fn announce_run(&mut self, cx: &mut Context<Self>) {
        cx.emit(ReviewEvent::RunChanged);
        cx.notify();
    }

    fn finish_review(&mut self, run: &Run, cx: &mut Context<Self>) {
        if run.kind != RunKind::Review {
            return;
        }
        let stale = self.replacing.remove(&run.id).unwrap_or_default();
        if run.state != RunState::Done || !self.applied.insert(run.id) {
            return;
        }
        let reviewed = matches!(run.outcome, Some(RunOutcome::Review(_)))
            || self.review_threads.iter().any(|id| !stale.contains(id));
        if reviewed {
            for id in stale {
                self.remove_thread(id, cx);
            }
        }
        if let Some(RunOutcome::Review(result)) = &run.outcome {
            for planned in plan_threads(result, &self.host.files) {
                let thread = self.store.add_finding(planned.anchor, planned.finding, &planned.body);
                self.review_threads.push(thread.id);
                cx.emit(ReviewEvent::ThreadsChanged(ThreadChange::Added(thread.id)));
            }
        }
    }

    pub fn create_thread(&mut self, anchor: ThreadAnchor, cx: &mut Context<Self>) -> ThreadId {
        let known = self.store.len();
        let thread = self.store.create(anchor);
        if self.store.len() != known {
            cx.emit(ReviewEvent::ThreadsChanged(ThreadChange::Added(thread.id)));
            cx.notify();
        }
        thread.id
    }

    /// Blank text is ignored. `@claude` in the text starts a reply.
    pub fn post(&mut self, id: ThreadId, text: &str, cx: &mut Context<Self>) {
        let text = text.trim();
        if text.is_empty() || self.store.post(id, text).is_none() {
            return;
        }
        self.thread_updated(id, cx);
        if mentions_claude(text) {
            self.ask_claude(id, cx);
        }
    }

    /// Blank text is ignored, so an edit cannot empty a comment.
    pub fn edit_comment(&mut self, id: ThreadId, comment: u32, text: &str, cx: &mut Context<Self>) {
        let text = text.trim();
        if !text.is_empty() && self.store.edit(id, comment, text).is_some() {
            self.thread_updated(id, cx);
        }
    }

    /// Queues a reply run for the thread. Claude answers with the MCP tools, so its reply shows when it is written.
    pub fn ask_claude(&mut self, id: ThreadId, cx: &mut Context<Self>) {
        self.refresh_threads(cx);
        if self.is_replying(id, cx) {
            return;
        }
        let Some(thread) = self.store.get(id).cloned() else {
            return;
        };
        let (Some(repo), Some(engine)) = (self.host.repo.clone(), Self::global_engine(cx)) else {
            let reason = if self.host.repo.is_none() {
                NO_REPO_MESSAGE
            } else {
                RUNS_UNAVAILABLE
            };
            return self.claude_says(id, reason, cx);
        };
        if let Some(reason) = cx.global::<Runs>().unavailable() {
            let reason = reason.to_string();
            return self.claude_says(id, &reason, cx);
        }
        let target = Target {
            repo,
            session: self.session_key(),
            revisions: self.host.comparison.revisions(),
            model: RunKind::Reply.activity().model(&self.settings),
            edit_permission: self.settings.edit_permission(),
        };
        let Ok(request) = fix::reply_to(&target, &thread, self.edit_blocker().is_none()) else {
            return;
        };
        match self.submit(&engine, request, cx) {
            Ok(run) => {
                self.asks.insert(run, (id, thread.comments.len()));
                self.thread_updated(id, cx);
                self.announce_run(cx);
            }
            Err(message) => self.claude_says(id, &message, cx),
        }
    }

    fn claude_says(&mut self, id: ThreadId, text: &str, cx: &mut Context<Self>) {
        if self.store.add_claude_comment(id, text).is_some() {
            self.thread_updated(id, cx);
        }
    }

    /// A reply run that ended without a word from Claude leaves a note in the thread, unless the reviewer stopped it.
    fn finish_reply(&mut self, run: RunId, state: RunState, cx: &mut Context<Self>) {
        let Some((id, before)) = self.asks.remove(&run) else {
            return;
        };
        self.refresh_threads(cx);
        let Some(thread) = self.store.get(id) else {
            return;
        };
        let answered = thread.comments.iter().skip(before).any(|c| c.author == CLAUDE_NAME);
        if answered || state == RunState::Stopped {
            self.thread_updated(id, cx);
        } else {
            self.claude_says(id, NO_REPLY_MESSAGE, cx);
        }
    }

    fn stop_replies(&mut self, id: ThreadId, cx: &App) {
        let runs = self.reply_runs(id, cx);
        if let Some(engine) = &self.engine {
            for run in runs {
                engine.stop(run);
            }
        }
    }

    /// Clears what Claude reported for an open thread. A resolved or dismissed thread goes back to open and
    /// returns to the diff.
    pub fn reopen_thread(&mut self, id: ThreadId, cx: &mut Context<Self>) {
        let Some(status) = self.store.get(id).map(|thread| thread.status) else {
            return;
        };
        if status == ThreadStatus::Open {
            if self.store.clear_addressed(id).is_some() {
                self.thread_updated(id, cx);
            }
            return;
        }
        self.store.clear_addressed(id);
        if let Some(thread) = self.store.set_status(id, ThreadStatus::Open) {
            if thread.finding.is_some() && !self.review_threads.contains(&id) {
                self.review_threads.push(id);
            }
            cx.emit(ReviewEvent::ThreadsChanged(ThreadChange::Added(id)));
            cx.notify();
        }
    }

    /// The repo's current run, when it is the last fix this session started.
    pub fn fix_run<'a>(&self, cx: &'a App) -> Option<&'a Run> {
        self.own_current(cx).filter(|run| Some(run.id) == self.fix_run)
    }

    pub fn fix_failure(&self) -> Option<&str> {
        self.fix_failure.as_deref()
    }

    pub fn fixing(&self, cx: &App) -> bool {
        self.fix_run(cx).is_some_and(|run| !run.state.is_finished())
    }

    pub fn fix_problem(&self, cx: &App) -> Option<String> {
        if let Some(message) = &self.fix_failure {
            return Some(message.clone());
        }
        let run = self.fix_run(cx).filter(|run| run.state == RunState::Failed)?;
        Some(
            run.error
                .clone()
                .unwrap_or_else(|| "Claude could not finish the fix.".into()),
        )
    }

    pub fn dismiss_fix_problem(&mut self, cx: &mut Context<Self>) {
        self.fix_failure = None;
        if self.fix_run(cx).is_some_and(|run| run.state == RunState::Failed) {
            self.fix_run = None;
        }
        self.announce_run(cx);
    }

    /// Threads Claude can fix now. Threads in the running or a queued fix or reply run are left out.
    pub fn fixable(&self, cx: &App) -> Vec<ThreadId> {
        let taken = self.taken_by_fix(cx);
        self.store
            .threads()
            .filter(|thread| fix::skip_reason(thread).is_none() && !taken.contains(&thread.id))
            .map(|thread| thread.id)
            .collect()
    }

    fn taken_by_fix(&self, cx: &App) -> HashSet<ThreadId> {
        self.live_runs(cx)
            .filter(|run| matches!(run.kind, RunKind::FixComments | RunKind::Reply))
            .flat_map(|run| run.threads.iter().copied())
            .collect()
    }

    /// Of the threads the last fix run was asked to take: how many Claude answered, and how many are still open.
    pub fn fix_outcome(&self, cx: &App) -> Option<(usize, usize)> {
        let run = self.fix_run(cx)?;
        let fixed = run
            .threads
            .iter()
            .filter(|id| {
                self.store
                    .get(**id)
                    .is_some_and(|thread| thread.addressed.as_ref().is_some_and(|mark| mark.run == run.id))
            })
            .count();
        Some((fixed, run.threads.len() - fixed))
    }

    pub fn fix_blocker(&self, cx: &App) -> Option<String> {
        let Some(runs) = cx.try_global::<Runs>() else {
            return Some(RUNS_UNAVAILABLE.into());
        };
        if let Some(reason) = runs.unavailable() {
            return Some(reason.into());
        }
        if self.host.repo.is_none() {
            return Some(NO_REPO_MESSAGE.into());
        }
        self.edit_blocker().map(String::from)
    }

    /// Why Claude may not edit files from this review: it edits the working tree, and only the Don't ask setting
    /// works for now.
    fn edit_blocker(&self) -> Option<&'static str> {
        let on_working_tree = matches!(
            self.host.comparison,
            Comparison::WorkingTree
                | Comparison::CheckedOut {
                    includes_uncommitted: true,
                    ..
                }
        );
        if !on_working_tree {
            return Some(FIX_NEEDS_WORKING_TREE);
        }
        if self.settings.edit_permission() != EditPermission::DontAsk {
            return Some(FIX_NEEDS_DONT_ASK);
        }
        None
    }

    /// Hands the threads in `chosen` to Claude, or every fixable thread when `chosen` is empty.
    pub fn fix_threads(&mut self, chosen: &[ThreadId], cx: &mut Context<Self>) {
        if let Some(reason) = self.fix_blocker(cx) {
            self.fail_fix(reason, cx);
            return;
        }
        let (Some(repo), Some(engine)) = (self.host.repo.clone(), Self::global_engine(cx)) else {
            return;
        };
        self.refresh_threads(cx);
        let taken = self.taken_by_fix(cx);
        let chosen: Vec<ThreadId> = if chosen.is_empty() {
            self.fixable(cx)
        } else {
            chosen.iter().copied().filter(|id| !taken.contains(id)).collect()
        };
        if chosen.is_empty() {
            return self.fail_fix(NOTHING_TO_FIX_MESSAGE.into(), cx);
        }
        let target = Target {
            repo,
            session: Self::thread_key(&self.host),
            revisions: self.host.comparison.revisions(),
            model: RunKind::FixComments.activity().model(&self.settings),
            edit_permission: self.settings.edit_permission(),
        };
        let threads: Vec<ReviewThread> = self.store.threads().cloned().collect();
        let plan = match fix::fix_comments(&target, &threads, &chosen) {
            Ok(plan) => plan,
            Err(_) => return self.fail_fix(NOTHING_TO_FIX_MESSAGE.into(), cx),
        };
        match self.submit(&engine, plan.request, cx) {
            Ok(run) => {
                self.fix_run = Some(run);
                self.fix_failure = None;
                self.announce_run(cx);
            }
            Err(message) => self.fail_fix(message, cx),
        }
    }

    fn fail_fix(&mut self, message: String, cx: &mut Context<Self>) {
        self.fix_failure = Some(message);
        self.announce_run(cx);
    }

    fn run_event(&mut self, activity: &Entity<ClaudeActivity>, event: &RunEvent, cx: &mut Context<Self>) {
        let session = self.store.session();
        if let RunEvent::ThreadsChanged { session: changed, .. } = event {
            if *changed == session {
                self.take_in_threads(cx);
            }
            return;
        }
        let Some(run) = activity
            .read(cx)
            .run(event.run())
            .filter(|run| run.session == session)
            .cloned()
        else {
            return;
        };
        match event {
            RunEvent::Started { .. } if run.kind == RunKind::Review => {
                self.replacing.insert(run.id, self.review_threads.clone());
            }
            RunEvent::Finished { state, .. } => {
                self.last = Some(run.summary());
                self.finish_review(&run, cx);
                cx.emit(ReviewEvent::RunFinished(*state));
                self.announce_run(cx);
                return self.finish_reply(run.id, *state, cx);
            }
            _ => {}
        }
        self.announce_run(cx);
    }

    /// The run engine and the MCP tools write these threads.
    fn take_in_threads(&mut self, cx: &mut Context<Self>) {
        let known: HashSet<ThreadId> = self.store.threads().map(|thread| thread.id).collect();
        self.refresh_threads(cx);
        let reviewing = self.review_run.is_some_and(|id| self.replacing.contains_key(&id));
        if !reviewing {
            return;
        }
        let added: Vec<ThreadId> = self
            .store
            .threads()
            .filter(|thread| !known.contains(&thread.id))
            .filter(|thread| thread.comments.first().is_some_and(|first| first.author == CLAUDE_NAME))
            .map(|thread| thread.id)
            .collect();
        self.review_threads.extend(added);
    }

    pub fn resolve_thread(&mut self, id: ThreadId, cx: &mut Context<Self>) {
        if self.store.get(id).is_none() {
            return;
        }
        self.stop_replies(id, cx);
        if self.store.set_status(id, ThreadStatus::Resolved).is_some() {
            self.review_threads.retain(|known| *known != id);
            cx.emit(ReviewEvent::ThreadsChanged(ThreadChange::Removed(id)));
            cx.notify();
        }
    }

    pub fn remove_thread(&mut self, id: ThreadId, cx: &mut Context<Self>) {
        if self.store.get(id).is_none() {
            return;
        }
        self.stop_replies(id, cx);
        self.store.remove(id);
        self.review_threads.retain(|known| *known != id);
        cx.emit(ReviewEvent::ThreadsChanged(ThreadChange::Removed(id)));
        cx.notify();
    }

    fn thread_updated(&mut self, id: ThreadId, cx: &mut Context<Self>) {
        cx.emit(ReviewEvent::ThreadsChanged(ThreadChange::Updated(id)));
        cx.notify();
    }
}
