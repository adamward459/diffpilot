use diffpilot_core::cancel::CancelToken;
use diffpilot_core::claude::{Replier, ReviewError, Reviewer};
use diffpilot_core::comparison::Comparison;
use diffpilot_core::diff::FileDiff;
use diffpilot_core::review::{ReviewModel, ReviewRun, ReviewSetup, plan_threads, review_title};
use diffpilot_core::settings::Settings;
use diffpilot_core::threads::{CLAUDE_NAME, ReviewThread, ThreadAnchor, ThreadId, ThreadStore, mentions_claude};
use gpui::{Context, EventEmitter, Task};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

pub const NO_REPO_MESSAGE: &str = "Open a Git repository first.";
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
}

impl EventEmitter<ReviewEvent> for ReviewSession {}

impl Drop for ReviewSession {
    fn drop(&mut self) {
        self.review
            .iter()
            .chain(self.replies.values())
            .for_each(|active| active.cancel.cancel());
    }
}

impl ReviewSession {
    pub fn new(
        host: ReviewHost,
        settings: Arc<Settings>,
        reviewer: Arc<dyn Reviewer>,
        replier: Arc<dyn Replier>,
    ) -> Self {
        let setup = Self::saved_setup(&settings, host.repo.as_deref());
        Self {
            host,
            settings,
            reviewer,
            replier,
            store: ThreadStore::new(),
            run: ReviewRun::Idle,
            setup,
            review_threads: Vec::new(),
            review: None,
            replies: HashMap::new(),
        }
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

    pub fn thread(&self, id: ThreadId) -> Option<&ReviewThread> {
        self.store.get(id)
    }

    pub fn threads(&self) -> impl Iterator<Item = &ReviewThread> {
        self.store.threads()
    }

    pub fn thread_count(&self) -> usize {
        self.store.len()
    }

    pub fn comment_counts(&self) -> std::collections::BTreeMap<String, usize> {
        self.store.comment_counts()
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
