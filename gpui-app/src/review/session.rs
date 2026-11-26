use crate::runs::Runs;
use diffpilot_core::cancel::CancelToken;
use diffpilot_core::claude::{Replier, ReviewError, Reviewer};
use diffpilot_core::comparison::Comparison;
use diffpilot_core::diff::FileDiff;
use diffpilot_core::fix::{self, Target};
use diffpilot_core::policy::EditPermission;
use diffpilot_core::refs::Refs;
use diffpilot_core::review::{ReviewModel, ReviewRun, ReviewSetup, plan_threads, review_title};
use diffpilot_core::run::{Run, RunId, RunKind, RunState};
use diffpilot_core::runner::{RunEngine, RunEvent};
use diffpilot_core::settings::Settings;
use diffpilot_core::store::{SessionId, SessionKey, Store};
use diffpilot_core::threads::{
    CLAUDE_NAME, ReviewThread, ThreadAnchor, ThreadId, ThreadStatus, ThreadStore, mentions_claude,
};
use gpui::{App, Context, EventEmitter, Task};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

pub const NO_REPO_MESSAGE: &str = "Open a Git repository first.";
pub const FIX_NEEDS_DONT_ASK: &str = "Fix needs the Don't ask edit setting for now.";
pub const FIX_NEEDS_WORKING_TREE: &str =
    "Fix edits your working tree. Pick a comparison that includes uncommitted changes.";
pub const NOTHING_TO_FIX_MESSAGE: &str = "There is no open comment for Claude to fix.";
pub const NO_REPLY_MESSAGE: &str = "Claude did not reply. Check that the claude command is installed, then ask again.";

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
    FixChanged,
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

struct Active {
    cancel: CancelToken,
    _task: Task<()>,
}

pub struct ReviewSession {
    host: ReviewHost,
    settings: Arc<Settings>,
    reviewer: Arc<dyn Reviewer>,
    replier: Arc<dyn Replier>,
    store: ThreadStore,
    run: ReviewRun,
    setup: ReviewSetup,
    review_threads: Vec<ThreadId>,
    review: Option<Active>,
    replies: HashMap<ThreadId, Active>,
    engine: Option<RunEngine>,
    fix_run: Option<Run>,
    fix_failure: Option<String>,
    run_events: Option<Task<()>>,
}

impl EventEmitter<ReviewEvent> for ReviewSession {}

impl Drop for ReviewSession {
    fn drop(&mut self) {
        self.review
            .iter()
            .chain(self.replies.values())
            .for_each(|active| active.cancel.cancel());
        if let (Some(engine), Some(run)) = (&self.engine, &self.fix_run)
            && !run.state.is_finished()
        {
            engine.stop(run.id);
        }
    }
}

impl ReviewSession {
    pub fn new(
        host: ReviewHost,
        settings: Arc<Settings>,
        store: Arc<Store>,
        reviewer: Arc<dyn Reviewer>,
        replier: Arc<dyn Replier>,
    ) -> Self {
        let setup = Self::saved_setup(&settings, host.repo.as_deref());
        let threads = ThreadStore::open(store, &Self::thread_key(&host)).unwrap_or_default();
        let review_threads = threads
            .threads()
            .filter(|thread| thread.finding.is_some() && thread.status == ThreadStatus::Open)
            .map(|thread| thread.id)
            .collect();
        Self {
            host,
            settings,
            reviewer,
            replier,
            store: threads,
            run: ReviewRun::Idle,
            setup,
            review_threads,
            review: None,
            replies: HashMap::new(),
            engine: None,
            fix_run: None,
            fix_failure: None,
            run_events: None,
        }
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

    pub fn run(&self) -> &ReviewRun {
        &self.run
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

    pub fn is_replying(&self, id: ThreadId) -> bool {
        self.replies.contains_key(&id)
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

    pub fn can_start(&self) -> bool {
        !matches!(self.run, ReviewRun::Running { .. })
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

    pub fn use_settings(&mut self, reviewer: Arc<dyn Reviewer>, replier: Arc<dyn Replier>, cx: &mut Context<Self>) {
        self.reviewer = reviewer;
        self.replier = replier;
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
        if !self.can_start() {
            return;
        }
        let Some(repo) = self.host.repo.clone() else {
            self.set_run(ReviewRun::Failed(NO_REPO_MESSAGE.into()), cx);
            return;
        };
        self.setup.save(&self.settings, &repo);
        let request = self
            .setup
            .request(&repo, &self.host.comparison, self.host.visible_path.as_deref());
        let cancel = CancelToken::new();
        let reviewer = self.reviewer.clone();
        let token = cancel.clone();
        let task = cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move { reviewer.review(&request, &token) })
                .await;
            this.update(cx, |session, cx| session.finish_review(outcome, cx)).ok();
        });
        self.review = Some(Active { cancel, _task: task });
        self.set_run(ReviewRun::Running { since: Instant::now() }, cx);
    }

    pub fn stop_review(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.run, ReviewRun::Running { .. }) {
            return;
        }
        if let Some(active) = self.review.take() {
            active.cancel.cancel();
        }
        self.set_run(ReviewRun::Idle, cx);
    }

    fn finish_review(
        &mut self,
        outcome: Result<diffpilot_core::review::ReviewResult, ReviewError>,
        cx: &mut Context<Self>,
    ) {
        let Some(active) = self.review.take() else { return };
        if active.cancel.is_cancelled() {
            return;
        }
        match outcome {
            Ok(result) => {
                for id in std::mem::take(&mut self.review_threads) {
                    self.remove_thread(id, cx);
                }
                for planned in plan_threads(&result, &self.host.files) {
                    let thread = self.store.add_finding(planned.anchor, planned.finding, &planned.body);
                    self.review_threads.push(thread.id);
                    cx.emit(ReviewEvent::ThreadsChanged(ThreadChange::Added(thread.id)));
                }
                self.set_run(
                    ReviewRun::Done {
                        counts: result.counts(),
                    },
                    cx,
                );
            }
            Err(ReviewError::Cancelled) => self.set_run(ReviewRun::Idle, cx),
            Err(error) => self.set_run(ReviewRun::Failed(error.to_string()), cx),
        }
    }

    fn set_run(&mut self, run: ReviewRun, cx: &mut Context<Self>) {
        self.run = run;
        cx.emit(ReviewEvent::RunChanged);
        cx.notify();
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

    pub fn comment_on_visible_file(&mut self, cx: &mut Context<Self>) -> Option<ThreadId> {
        let path = self.host.visible_path.clone()?;
        Some(self.create_thread(ThreadAnchor::file(&path), cx))
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

    pub fn ask_claude(&mut self, id: ThreadId, cx: &mut Context<Self>) {
        let Some(thread) = self.store.begin_claude_reply(id) else {
            return;
        };
        self.thread_updated(id, cx);
        let cancel = CancelToken::new();
        let chunks = self.replier.reply(&thread, &cancel);
        let task = cx.spawn(async move |this, cx| {
            while let Ok(chunk) = chunks.recv().await {
                if this
                    .update(cx, |session, cx| session.append_reply(id, &chunk, cx))
                    .is_err()
                {
                    return;
                }
            }
            this.update(cx, |session, cx| session.finish_reply(id, cx)).ok();
        });
        self.replies.insert(id, Active { cancel, _task: task });
    }

    fn append_reply(&mut self, id: ThreadId, chunk: &str, cx: &mut Context<Self>) {
        if self.store.append_claude(id, chunk, false).is_some() {
            self.thread_updated(id, cx);
        }
    }

    fn finish_reply(&mut self, id: ThreadId, cx: &mut Context<Self>) {
        self.replies.remove(&id);
        let silent = self
            .store
            .get(id)
            .and_then(|thread| {
                thread
                    .comments
                    .iter()
                    .rev()
                    .find(|c| c.author == CLAUDE_NAME && c.is_streaming)
            })
            .is_some_and(|comment| comment.body.is_empty());
        let fallback = if silent { NO_REPLY_MESSAGE } else { "" };
        if self.store.append_claude(id, fallback, true).is_some() {
            self.thread_updated(id, cx);
        }
    }

    pub fn reopen_thread(&mut self, id: ThreadId, cx: &mut Context<Self>) {
        if self.store.get(id).is_some() && self.store.clear_addressed(id).is_some() {
            self.thread_updated(id, cx);
        }
    }

    pub fn fix_run(&self) -> Option<&Run> {
        self.fix_run.as_ref()
    }

    pub fn fix_failure(&self) -> Option<&str> {
        self.fix_failure.as_deref()
    }

    pub fn fixing(&self) -> bool {
        self.fix_run.as_ref().is_some_and(|run| !run.state.is_finished())
    }

    pub fn fix_problem(&self) -> Option<String> {
        if let Some(message) = &self.fix_failure {
            return Some(message.clone());
        }
        let run = self.fix_run.as_ref().filter(|run| run.state == RunState::Failed)?;
        Some(
            run.error
                .clone()
                .unwrap_or_else(|| "Claude could not finish the fix.".into()),
        )
    }

    pub fn dismiss_fix_problem(&mut self, cx: &mut Context<Self>) {
        self.fix_failure = None;
        if self.fix_run.as_ref().is_some_and(|run| run.state == RunState::Failed) {
            self.fix_run = None;
        }
        self.announce_fix(cx);
    }

    pub fn fixable(&self) -> Vec<ThreadId> {
        self.store
            .threads()
            .filter(|thread| fix::skip_reason(thread).is_none())
            .map(|thread| thread.id)
            .collect()
    }

    /// Of the threads the last fix run was asked to take: how many Claude answered, and how many are still open.
    pub fn fix_outcome(&self) -> Option<(usize, usize)> {
        let run = self.fix_run.as_ref()?;
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
        if self.fixing() {
            return Some("Claude is already fixing comments.".into());
        }
        let Some(runs) = cx.try_global::<Runs>() else {
            return Some("Claude runs are not available here.".into());
        };
        if let Some(reason) = runs.unavailable() {
            return Some(reason.into());
        }
        if self.host.repo.is_none() {
            return Some(NO_REPO_MESSAGE.into());
        }
        let on_working_tree = matches!(
            self.host.comparison,
            Comparison::WorkingTree
                | Comparison::CheckedOut {
                    includes_uncommitted: true,
                    ..
                }
        );
        if !on_working_tree {
            return Some(FIX_NEEDS_WORKING_TREE.into());
        }
        if self.settings.edit_permission() != EditPermission::DontAsk {
            return Some(FIX_NEEDS_DONT_ASK.into());
        }
        None
    }

    /// Hands the threads in `chosen` to Claude, or every fixable thread when `chosen` is empty.
    pub fn fix_threads(&mut self, chosen: &[ThreadId], cx: &mut Context<Self>) {
        if let Some(reason) = self.fix_blocker(cx) {
            self.fail_fix(reason, cx);
            return;
        }
        let (Some(repo), Some(runs)) = (self.host.repo.clone(), cx.try_global::<Runs>()) else {
            return;
        };
        let engine = runs.engine().clone();
        self.refresh_threads(cx);
        let target = Target {
            repo,
            session: Self::thread_key(&self.host),
            revisions: self.host.comparison.revisions(),
            model: self.settings.model_for(RunKind::FixComments),
            edit_permission: self.settings.edit_permission(),
        };
        let threads: Vec<ReviewThread> = self.store.threads().cloned().collect();
        let plan = match fix::fix_comments(&target, &threads, chosen) {
            Ok(plan) => plan,
            Err(_) => return self.fail_fix(NOTHING_TO_FIX_MESSAGE.into(), cx),
        };
        self.watch_runs(&engine, cx);
        match engine.submit(plan.request) {
            Ok(id) => {
                self.fix_failure = None;
                self.fix_run = engine.run(id);
                self.announce_fix(cx);
            }
            Err(error) => self.fail_fix(error.to_string(), cx),
        }
    }

    pub fn stop_fix(&mut self, cx: &mut Context<Self>) {
        if let (Some(engine), Some(run)) = (&self.engine, &self.fix_run)
            && !run.state.is_finished()
        {
            engine.stop(run.id);
            cx.notify();
        }
    }

    pub fn stop_claude(&mut self, cx: &mut Context<Self>) {
        self.stop_review(cx);
        self.stop_fix(cx);
    }

    fn fail_fix(&mut self, message: String, cx: &mut Context<Self>) {
        self.fix_run = None;
        self.fix_failure = Some(message);
        self.announce_fix(cx);
    }

    fn announce_fix(&mut self, cx: &mut Context<Self>) {
        cx.emit(ReviewEvent::FixChanged);
        cx.notify();
    }

    fn watch_runs(&mut self, engine: &RunEngine, cx: &mut Context<Self>) {
        if self.run_events.is_some() {
            return;
        }
        self.engine = Some(engine.clone());
        let events = engine.subscribe();
        self.run_events = Some(cx.spawn(async move |this, cx| {
            while let Ok(first) = events.recv().await {
                let mut batch = vec![first];
                while let Ok(more) = events.try_recv() {
                    batch.push(more);
                }
                if this
                    .update(cx, |session, cx| session.apply_run_events(batch, cx))
                    .is_err()
                {
                    return;
                }
            }
        }));
    }

    fn apply_run_events(&mut self, events: Vec<RunEvent>, cx: &mut Context<Self>) {
        let own = self.fix_run.as_ref().map(|run| run.id);
        let ours = |run: &RunId| Some(*run) == own;
        let mut threads_changed = false;
        let mut fix_changed = false;
        for event in &events {
            match event {
                RunEvent::ThreadsChanged { session, .. } => threads_changed |= *session == self.store.session(),
                RunEvent::Finished { run, .. } => {
                    fix_changed |= ours(run);
                    threads_changed |= ours(run);
                }
                RunEvent::Queued { run } | RunEvent::Started { run } | RunEvent::Updated { run } => {
                    fix_changed |= ours(run);
                }
                _ => {}
            }
        }
        if threads_changed {
            self.refresh_threads(cx);
        }
        if fix_changed && let (Some(engine), Some(id)) = (&self.engine, own) {
            self.fix_run = engine.run(id);
            self.announce_fix(cx);
        }
    }

    pub fn resolve_thread(&mut self, id: ThreadId, cx: &mut Context<Self>) {
        if self.store.get(id).is_none() {
            return;
        }
        if let Some(active) = self.replies.remove(&id) {
            active.cancel.cancel();
        }
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
        if let Some(active) = self.replies.remove(&id) {
            active.cancel.cancel();
        }
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
