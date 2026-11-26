use crate::diff_pane::PaneNote;
use crate::review::ReviewSession;
use crate::review::session::NO_REPO_MESSAGE;
use crate::runs::Runs;
use diffpilot_core::diff::FileDiff;
use diffpilot_core::fix::Target;
use diffpilot_core::guide::prompt;
use diffpilot_core::guide::reading::{ChapterKey, Reading, ReadingChapter, reading};
use diffpilot_core::guide::stale::{self, Staleness};
use diffpilot_core::policy::EditPermission;
use diffpilot_core::run::{RunKind, RunState};
use diffpilot_core::runner::{RunEngine, RunEvent};
use diffpilot_core::settings::Settings;
use diffpilot_core::store::{GuideId, RunId, SessionId, Store, StoredGuide};
use gpui::{App, Context, Entity, EventEmitter, Subscription, Task};
use std::collections::HashSet;
use std::sync::Arc;

pub const NOT_FINISHED_MESSAGE: &str = "Claude stopped before the guide was done. Try again.";
pub const FAILED_MESSAGE: &str = "Claude could not write the guide.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuideRun {
    Idle,
    Writing { run: RunId, chapters: usize },
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Overview,
    /// Index into `Reading::chapters`.
    Chapter(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuideEvent {
    Changed,
}

pub struct GuideSession {
    review: Entity<ReviewSession>,
    store: Arc<Store>,
    settings: Arc<Settings>,
    session: SessionId,
    stored: Option<StoredGuide>,
    files: Vec<Arc<FileDiff>>,
    reading: Reading,
    staleness: Option<Staleness>,
    commits: Option<u32>,
    kept: Option<GuideId>,
    run: GuideRun,
    step: Step,
    viewed: HashSet<String>,
    engine: Option<RunEngine>,
    events: Option<Task<()>>,
    _review: Subscription,
}

impl EventEmitter<GuideEvent> for GuideSession {}

impl GuideSession {
    pub fn new(
        review: Entity<ReviewSession>,
        settings: Arc<Settings>,
        store: Arc<Store>,
        cx: &mut Context<Self>,
    ) -> Self {
        let session = review.read(cx).session_id();
        let observed = cx.observe(&review, |this, _, cx| this.files_may_have_changed(cx));
        let mut this = Self {
            review,
            store,
            settings,
            session,
            stored: None,
            files: Vec::new(),
            reading: Reading::default(),
            staleness: None,
            commits: None,
            kept: None,
            run: GuideRun::Idle,
            step: Step::Overview,
            viewed: HashSet::new(),
            engine: None,
            events: None,
            _review: observed,
        };
        this.stored = this.store.current_guide(session).ok().flatten();
        this.files_may_have_changed(cx);
        this
    }

    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.stored = self.store.current_guide(self.session).ok().flatten();
        self.recompute(cx);
    }

    pub fn stored(&self) -> Option<&StoredGuide> {
        self.stored.as_ref()
    }

    pub fn reading(&self) -> &Reading {
        &self.reading
    }

    pub fn run(&self) -> &GuideRun {
        &self.run
    }

    pub fn step(&self) -> Step {
        self.step
    }

    pub fn viewed(&self) -> &HashSet<String> {
        &self.viewed
    }

    pub fn chapter(&self) -> Option<&ReadingChapter> {
        match self.step {
            Step::Chapter(index) => self.reading.chapters.get(index),
            Step::Overview => None,
        }
    }

    pub fn is_read(&self, chapter: &ReadingChapter) -> bool {
        chapter.key != ChapterKey::Extra && chapter.files.iter().all(|path| self.viewed.contains(path))
    }

    pub fn read_count(&self) -> usize {
        self.reading
            .chapters
            .iter()
            .filter(|chapter| self.is_read(chapter))
            .count()
    }

    pub fn first_unread(&self) -> usize {
        self.reading
            .chapters
            .iter()
            .position(|chapter| !self.is_read(chapter))
            .unwrap_or(0)
    }

    pub fn has_progress(&self) -> bool {
        self.read_count() > 0
    }

    /// The guide no longer matches the diff, and the reviewer did not choose to keep reading it.
    pub fn out_of_date(&self) -> bool {
        let (Some(stored), Some(staleness)) = (&self.stored, &self.staleness) else {
            return false;
        };
        staleness.needs_attention() && self.kept != Some(stored.id)
    }

    pub fn out_of_date_text(&self) -> String {
        match self.commits {
            Some(1) => "The branch moved 1 commit after Claude wrote this guide.".into(),
            Some(count) if count > 1 => format!("The branch moved {count} commits after Claude wrote this guide."),
            _ => "The diff changed after Claude wrote this guide.".into(),
        }
    }

    pub fn keep_reading(&mut self, cx: &mut Context<Self>) {
        self.kept = self.stored.as_ref().map(|stored| stored.id);
        self.announce(cx);
    }

    pub fn set_viewed(&mut self, viewed: HashSet<String>, cx: &mut Context<Self>) {
        if self.viewed != viewed {
            self.viewed = viewed;
            self.announce(cx);
        }
    }

    pub fn go(&mut self, step: Step, cx: &mut Context<Self>) {
        let step = match step {
            Step::Chapter(index) if index < self.reading.chapters.len() => step,
            Step::Chapter(_) => Step::Overview,
            Step::Overview => Step::Overview,
        };
        if step != self.step {
            self.step = step;
            self.announce(cx);
        }
    }

    pub fn start_reading(&mut self, cx: &mut Context<Self>) {
        self.go(Step::Chapter(self.first_unread()), cx);
    }

    pub fn next(&mut self, cx: &mut Context<Self>) {
        match self.step {
            Step::Overview => self.go(Step::Chapter(0), cx),
            Step::Chapter(index) => self.go(Step::Chapter(index + 1), cx),
        }
    }

    pub fn previous(&mut self, cx: &mut Context<Self>) {
        if let Step::Chapter(index) = self.step {
            self.go(
                match index {
                    0 => Step::Overview,
                    _ => Step::Chapter(index - 1),
                },
                cx,
            );
        }
    }

    /// The notes of the current chapter as the pane shows them: a note whose line moved follows it, and a note
    /// whose code is gone sits under its file header.
    pub fn pane_notes(&self) -> Vec<PaneNote> {
        use diffpilot_core::anchor::Placement;
        let (Some(chapter), Some(stored)) = (self.chapter(), &self.stored) else {
            return Vec::new();
        };
        let ChapterKey::Guide(index) = chapter.key else {
            return Vec::new();
        };
        let notes = &stored.guide.chapters[index].notes;
        let placements = self
            .staleness
            .as_ref()
            .and_then(|staleness| staleness.chapters.get(index))
            .map(|status| status.notes.as_slice())
            .unwrap_or_default();
        notes
            .iter()
            .enumerate()
            .filter(|(_, note)| chapter.files.contains(&note.path))
            .map(|(position, note)| {
                let at = note.span.as_ref().map(|span| (span.side, span.line));
                let (side, line) = match (placements.get(position), at) {
                    (Some(Placement::Moved { line, .. }), Some((side, _))) => (side, Some(*line)),
                    (Some(Placement::Outdated), _) | (_, None) => (diffpilot_core::threads::Side::New, None),
                    (_, Some((side, line))) => (side, Some(line)),
                };
                PaneNote {
                    path: note.path.clone(),
                    side,
                    line,
                    text: note.text.clone(),
                    critical: note.critical,
                }
            })
            .collect()
    }

    fn files_may_have_changed(&mut self, cx: &mut Context<Self>) {
        let files: Vec<Arc<FileDiff>> = self.review.read(cx).host().files.to_vec();
        let same = files.len() == self.files.len() && files.iter().zip(&self.files).all(|(a, b)| Arc::ptr_eq(a, b));
        if same {
            return;
        }
        self.files = files;
        self.recompute(cx);
    }

    fn recompute(&mut self, cx: &mut Context<Self>) {
        let Some(stored) = &self.stored else {
            self.reading = Reading::default();
            self.staleness = None;
            self.commits = None;
            return;
        };
        let files: Vec<FileDiff> = self.files.iter().map(|file| (**file).clone()).collect();
        self.reading = reading(&stored.guide, &files);
        let staleness = (!files.is_empty()).then(|| stale::check(&stored.guide, &files));
        self.commits = match (&staleness, self.review.read(cx).host().repo.as_deref()) {
            (Some(staleness), Some(repo)) if staleness.is_stale => stale::commits_since(repo, &stored.guide),
            _ => None,
        };
        self.staleness = staleness;
        if let Step::Chapter(index) = self.step
            && index >= self.reading.chapters.len()
        {
            self.step = Step::Overview;
        }
        self.announce(cx);
    }

    fn announce(&mut self, cx: &mut Context<Self>) {
        cx.emit(GuideEvent::Changed);
        cx.notify();
    }

    pub fn blocker(&self, cx: &App) -> Option<String> {
        if matches!(self.run, GuideRun::Writing { .. }) {
            return Some("Claude is already writing the guide.".into());
        }
        let Some(runs) = cx.try_global::<Runs>() else {
            return Some("Claude runs are not available here.".into());
        };
        if let Some(reason) = runs.unavailable() {
            return Some(reason.into());
        }
        if self.review.read(cx).host().repo.is_none() {
            return Some(NO_REPO_MESSAGE.into());
        }
        None
    }

    pub fn write(&mut self, cx: &mut Context<Self>) {
        if let Some(reason) = self.blocker(cx) {
            self.run = GuideRun::Failed(reason);
            return self.announce(cx);
        }
        let (Some(runs), host_repo) = (cx.try_global::<Runs>(), self.review.read(cx).host().repo.clone()) else {
            return;
        };
        let Some(repo) = host_repo else { return };
        let engine = runs.engine().clone();
        let review = self.review.read(cx);
        let target = Target {
            repo,
            session: review.session_key(),
            revisions: review.host().comparison.revisions(),
            model: self.settings.model_for(RunKind::Guide),
            edit_permission: EditPermission::default(),
        };
        self.watch(&engine, cx);
        self.run = match engine.submit(prompt::request(&target, "")) {
            Ok(run) => GuideRun::Writing { run, chapters: 0 },
            Err(error) => GuideRun::Failed(error.to_string()),
        };
        self.announce(cx);
    }

    pub fn stop(&mut self, cx: &mut Context<Self>) {
        if let (Some(engine), GuideRun::Writing { run, .. }) = (&self.engine, &self.run) {
            engine.stop(*run);
        }
        cx.notify();
    }

    fn watch(&mut self, engine: &RunEngine, cx: &mut Context<Self>) {
        if self.events.is_some() {
            return;
        }
        self.engine = Some(engine.clone());
        let events = engine.subscribe();
        self.events = Some(cx.spawn(async move |this, cx| {
            while let Ok(first) = events.recv().await {
                let mut batch = vec![first];
                while let Ok(more) = events.try_recv() {
                    batch.push(more);
                }
                if this.update(cx, |session, cx| session.apply(batch, cx)).is_err() {
                    return;
                }
            }
        }));
    }

    fn apply(&mut self, events: Vec<RunEvent>, cx: &mut Context<Self>) {
        let GuideRun::Writing { run: own, .. } = self.run else {
            return;
        };
        for event in events {
            match event {
                RunEvent::GuideChanged { run, session } if run == own && session == self.session => {
                    let chapters = self
                        .store
                        .load_draft(run)
                        .ok()
                        .flatten()
                        .map_or(0, |draft| draft.guide.chapters.len());
                    self.run = GuideRun::Writing { run, chapters };
                    self.announce(cx);
                }
                RunEvent::Finished { run, state } if run == own => return self.finish(run, state, cx),
                _ => {}
            }
        }
    }

    fn finish(&mut self, run: RunId, state: RunState, cx: &mut Context<Self>) {
        let written = self.store.guide_of_run(run).ok().flatten().is_some();
        self.run = match state {
            RunState::Stopped => GuideRun::Idle,
            RunState::Done if written => GuideRun::Idle,
            RunState::Done => GuideRun::Failed(NOT_FINISHED_MESSAGE.into()),
            _ => GuideRun::Failed(
                self.engine
                    .as_ref()
                    .and_then(|engine| engine.run(run))
                    .and_then(|run| run.error)
                    .unwrap_or_else(|| FAILED_MESSAGE.into()),
            ),
        };
        if written {
            self.step = Step::Overview;
            self.kept = None;
            self.reload(cx);
        } else {
            self.announce(cx);
        }
    }
}
