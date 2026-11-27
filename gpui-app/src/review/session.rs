use crate::runs::Runs;
use diffpilot_core::cancel::CancelToken;
use diffpilot_core::claude::Replier;
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
use gpui::{App, Context, EventEmitter, Task};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

pub const NO_REPO_MESSAGE: &str = "Open a Git repository first.";
pub const FIX_NEEDS_DONT_ASK: &str = "Fix needs the Don't ask edit setting for now.";
pub const FIX_NEEDS_WORKING_TREE: &str =
    "Fix edits your working tree. Pick a comparison that includes uncommitted changes.";
pub const NOTHING_TO_FIX_MESSAGE: &str = "There is no open comment for Claude to fix.";
pub const RUNS_UNAVAILABLE: &str = "Claude runs are not available here.";
const PAST_RUN_WINDOW: usize = 200;
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

struct Active {
    cancel: CancelToken,
    _task: Task<()>,
}

pub struct ReviewSession {
    host: ReviewHost,
    settings: Arc<Settings>,
    replier: Arc<dyn Replier>,
    store: ThreadStore,
    setup: ReviewSetup,
    review_threads: Vec<ThreadId>,
    replies: HashMap<ThreadId, Active>,
    engine: Option<RunEngine>,
    current: Option<Run>,
    queued: Vec<Run>,
    shown: Option<RunId>,
    shown_run: Option<Run>,
    last: Option<RunSummary>,
    submitted: HashSet<RunId>,
    replacing: HashMap<RunId, Vec<ThreadId>>,
    applied: HashSet<RunId>,
    owners: HashMap<RunId, bool>,
    review_failure: Option<String>,
    fix_failure: Option<String>,
    run_events: Option<Task<()>>,
}

impl EventEmitter<ReviewEvent> for ReviewSession {}

impl Drop for ReviewSession {
    fn drop(&mut self) {
        self.replies.values().for_each(|active| active.cancel.cancel());
        if let Some(engine) = &self.engine {
            for id in &self.submitted {
                engine.stop(*id);
            }
        }
    }
}

impl ReviewSession {
    pub fn new(
        host: ReviewHost,
        settings: Arc<Settings>,
        store: Arc<Store>,
        replier: Arc<dyn Replier>,
        cx: &mut Context<Self>,
    ) -> Self {
        let setup = Self::saved_setup(&settings, host.repo.as_deref());
        let threads = ThreadStore::open(store, &Self::thread_key(&host)).unwrap_or_default();
        let review_threads = threads
            .threads()
            .filter(|thread| thread.finding.is_some() && thread.status == ThreadStatus::Open)
            .map(|thread| thread.id)
            .collect();
        let mut this = Self {
            host,
            settings,
            replier,
            store: threads,
            setup,
            review_threads,
            replies: HashMap::new(),
            engine: None,
            current: None,
            queued: Vec::new(),
            shown: None,
            shown_run: None,
            last: None,
            submitted: HashSet::new(),
            replacing: HashMap::new(),
            applied: HashSet::new(),
            owners: HashMap::new(),
            review_failure: None,
            fix_failure: None,
            run_events: None,
        };
        if let Some(engine) = Self::global_engine(cx) {
            this.watch_runs(&engine, cx);
            this.last = Self::tracked_runs(&engine, &this.session_key(), 1).into_iter().next();
            this.sync_runs(&engine);
        }
        this
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

    pub fn use_settings(&mut self, replier: Arc<dyn Replier>, cx: &mut Context<Self>) {
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
            Ok(_) => {
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
        self.watch_runs(engine, cx);
        let id = engine.submit(request).map_err(|error| error.to_string())?;
        self.owners.insert(id, true);
        self.submitted.insert(id);
        self.shown = None;
        self.shown_run = None;
        self.sync_runs(engine);
        Ok(id)
    }

    pub fn run_again(&mut self, cx: &mut Context<Self>) {
        let Some(run) = self.current.as_ref().filter(|run| run.state.is_finished()) else {
            return;
        };
        match run.kind {
            RunKind::Review => self.start_review(cx),
            RunKind::FixComments => {
                let open = self.fixable();
                let again: Vec<ThreadId> = run.threads.iter().copied().filter(|id| open.contains(id)).collect();
                if again.is_empty() {
                    return self.fail_fix(NOTHING_TO_FIX_MESSAGE.into(), cx);
                }
                self.fix_threads(&again, cx);
            }
            _ => {}
        }
    }

    pub fn can_run_again(&self) -> bool {
        self.current
            .as_ref()
            .is_some_and(|run| run.state.is_finished() && matches!(run.kind, RunKind::Review | RunKind::FixComments))
    }

    pub fn current(&self) -> Option<&Run> {
        self.current.as_ref()
    }

    pub fn queued(&self) -> &[Run] {
        &self.queued
    }

    pub fn last_run(&self) -> Option<&RunSummary> {
        self.last.as_ref()
    }

    pub fn is_busy(&self) -> bool {
        self.current.as_ref().is_some_and(|run| !run.state.is_finished()) || !self.queued.is_empty()
    }

    pub fn shown(&self) -> Option<&Run> {
        match (&self.shown_run, &self.current) {
            (Some(run), _) if self.shown == Some(run.id) => Some(run),
            _ => self.current.as_ref(),
        }
    }

    pub fn follows_current(&self) -> bool {
        self.shown.is_none()
    }

    pub fn show_run(&mut self, id: RunId, cx: &mut Context<Self>) {
        if self.current.as_ref().is_some_and(|run| run.id == id) {
            return self.follow_current(cx);
        }
        let Some(run) = self.engine.as_ref().and_then(|engine| engine.run(id)) else {
            return;
        };
        self.shown = Some(id);
        self.shown_run = Some(run);
        cx.notify();
    }

    pub fn follow_current(&mut self, cx: &mut Context<Self>) {
        self.shown = None;
        self.shown_run = None;
        cx.notify();
    }

    pub fn past_runs(&self, limit: usize) -> Vec<RunSummary> {
        self.engine
            .as_ref()
            .map(|engine| Self::tracked_runs(engine, &self.session_key(), limit))
            .unwrap_or_default()
    }

    fn tracked_runs(engine: &RunEngine, key: &SessionKey, limit: usize) -> Vec<RunSummary> {
        engine
            .past_runs(key, PAST_RUN_WINDOW)
            .unwrap_or_default()
            .into_iter()
            .filter(|summary| summary.kind != RunKind::Guide)
            .take(limit)
            .collect()
    }

    pub fn review_run(&self) -> Option<&Run> {
        self.current.as_ref().filter(|run| run.kind == RunKind::Review)
    }

    pub fn reviewing(&self) -> bool {
        self.review_run().is_some_and(|run| !run.state.is_finished())
    }

    pub fn review_problem(&self) -> Option<String> {
        if let Some(message) = &self.review_failure {
            return Some(message.clone());
        }
        let run = self.review_run().filter(|run| run.state == RunState::Failed)?;
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

    pub fn stop_claude(&mut self, cx: &mut Context<Self>) {
        let Some(run) = self.current.as_ref().filter(|run| !run.state.is_finished()) else {
            return;
        };
        self.stop_run(run.id, cx);
    }

    pub fn stop_run(&mut self, id: RunId, cx: &mut Context<Self>) {
        if let Some(engine) = &self.engine
            && engine.stop(id)
        {
            cx.notify();
        }
    }

    fn announce_run(&mut self, cx: &mut Context<Self>) {
        cx.emit(ReviewEvent::RunChanged);
        cx.notify();
    }

    fn finish_review(&mut self, run: &Run, cx: &mut Context<Self>) {
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
        self.current.as_ref().filter(|run| run.kind == RunKind::FixComments)
    }

    pub fn fix_failure(&self) -> Option<&str> {
        self.fix_failure.as_deref()
    }

    pub fn fixing(&self) -> bool {
        self.fix_run().is_some_and(|run| !run.state.is_finished())
    }

    pub fn fix_problem(&self) -> Option<String> {
        if let Some(message) = &self.fix_failure {
            return Some(message.clone());
        }
        let run = self.fix_run().filter(|run| run.state == RunState::Failed)?;
        Some(
            run.error
                .clone()
                .unwrap_or_else(|| "Claude could not finish the fix.".into()),
        )
    }

    pub fn dismiss_fix_problem(&mut self, cx: &mut Context<Self>) {
        self.fix_failure = None;
        if self.fix_run().is_some_and(|run| run.state == RunState::Failed) {
            self.current = None;
        }
        self.announce_run(cx);
    }

    /// Threads Claude can fix now. Threads in the running or a queued fix run are left out.
    pub fn fixable(&self) -> Vec<ThreadId> {
        let taken = self.taken_by_fix();
        self.store
            .threads()
            .filter(|thread| fix::skip_reason(thread).is_none() && !taken.contains(&thread.id))
            .map(|thread| thread.id)
            .collect()
    }

    fn taken_by_fix(&self) -> HashSet<ThreadId> {
        self.current
            .iter()
            .filter(|run| !run.state.is_finished())
            .chain(&self.queued)
            .filter(|run| run.kind == RunKind::FixComments)
            .flat_map(|run| run.threads.iter().copied())
            .collect()
    }

    /// Of the threads the last fix run was asked to take: how many Claude answered, and how many are still open.
    pub fn fix_outcome(&self) -> Option<(usize, usize)> {
        let run = self.fix_run()?;
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
        let (Some(repo), Some(engine)) = (self.host.repo.clone(), Self::global_engine(cx)) else {
            return;
        };
        self.refresh_threads(cx);
        let taken = self.taken_by_fix();
        let chosen: Vec<ThreadId> = if chosen.is_empty() {
            self.fixable()
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
            model: self.settings.model_for(RunKind::FixComments),
            edit_permission: self.settings.edit_permission(),
        };
        let threads: Vec<ReviewThread> = self.store.threads().cloned().collect();
        let plan = match fix::fix_comments(&target, &threads, &chosen) {
            Ok(plan) => plan,
            Err(_) => return self.fail_fix(NOTHING_TO_FIX_MESSAGE.into(), cx),
        };
        match self.submit(&engine, plan.request, cx) {
            Ok(_) => {
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

    fn owns(&mut self, engine: &RunEngine, run: RunId) -> bool {
        let session = self.store.session();
        *self.owners.entry(run).or_insert_with(|| {
            engine
                .run(run)
                .is_some_and(|found| found.session == session && found.kind != RunKind::Guide)
        })
    }

    fn apply_run_events(&mut self, events: Vec<RunEvent>, cx: &mut Context<Self>) {
        let Some(engine) = self.engine.clone() else {
            return;
        };
        let mut threads_changed = false;
        let mut touched = false;
        for event in &events {
            match event {
                RunEvent::ThreadsChanged { session, .. } => threads_changed |= *session == self.store.session(),
                RunEvent::Started { run } if self.owns(&engine, *run) => {
                    touched = true;
                    if std::mem::take(&mut threads_changed) {
                        self.take_in_threads(cx);
                    }
                    if engine.run(*run).is_some_and(|found| found.kind == RunKind::Review) {
                        self.replacing.insert(*run, self.review_threads.clone());
                    }
                }
                RunEvent::Finished { run, state } if self.owns(&engine, *run) => {
                    touched = true;
                    if std::mem::take(&mut threads_changed) {
                        self.take_in_threads(cx);
                    }
                    if let Some(found) = engine.run(*run) {
                        if found.kind == RunKind::Review {
                            self.finish_review(&found, cx);
                        }
                        self.last = Some(found.summary());
                    }
                    cx.emit(ReviewEvent::RunFinished(*state));
                }
                RunEvent::Queued { run } | RunEvent::Updated { run } | RunEvent::Log { run, .. }
                    if self.owns(&engine, *run) =>
                {
                    touched = true;
                }
                _ => {}
            }
        }
        if threads_changed {
            self.take_in_threads(cx);
        }
        if touched {
            self.sync_runs(&engine);
            self.announce_run(cx);
        }
    }

    /// The run engine and the MCP tools write these threads.
    fn take_in_threads(&mut self, cx: &mut Context<Self>) {
        let known: HashSet<ThreadId> = self.store.threads().map(|thread| thread.id).collect();
        self.refresh_threads(cx);
        if !self.reviewing() {
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

    fn sync_runs(&mut self, engine: &RunEngine) {
        let session = self.store.session();
        let ours = |run: &Run| run.session == session && run.kind != RunKind::Guide;
        let (active, mut queued) = match &self.host.repo {
            Some(repo) => (
                engine.active(repo).filter(ours),
                engine.queued(repo).into_iter().filter(ours).collect::<Vec<_>>(),
            ),
            None => (None, Vec::new()),
        };
        let kept = self.current.as_ref().and_then(|run| engine.run(run.id));
        self.current = active.or(kept.filter(|run| !run.state.is_finished() || queued.is_empty()));
        if self.current.is_none() && !queued.is_empty() {
            self.current = Some(queued.remove(0));
        }
        if let Some(current) = &self.current {
            queued.retain(|run| run.id != current.id);
        }
        self.queued = queued;
        if let Some(id) = self.shown
            && self.shown_run.as_ref().is_none_or(|run| !run.state.is_finished())
        {
            self.shown_run = engine.run(id);
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
