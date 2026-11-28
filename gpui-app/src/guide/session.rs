use crate::claude_activity::ClaudeActivity;
use crate::diff_pane::{LinkedStop, PaneNote};
use crate::guide::path::Counts;
use crate::guide::stops::Stops;
use crate::review::ReviewSession;
use crate::review::session::NO_REPO_MESSAGE;
use crate::runs::Runs;
use diffpilot_core::diff::FileDiff;
use diffpilot_core::fix::Target;
use diffpilot_core::guide::linked;
use diffpilot_core::guide::mermaid::Graph;
use diffpilot_core::guide::reading::{ChapterKey, Reading, ReadingChapter, reading};
use diffpilot_core::guide::stale::{self, Staleness};
use diffpilot_core::guide::{Chapter, Diagram, Span, prompt};
use diffpilot_core::policy::EditPermission;
use diffpilot_core::run::{RunKind, RunState};
use diffpilot_core::runner::RunEvent;
use diffpilot_core::settings::Settings;
use diffpilot_core::store::{GuideId, RunId, SessionId, Store, StoredGuide};
use gpui::{App, AppContext as _, Context, Entity, EventEmitter, Subscription};
use std::collections::{BTreeSet, HashSet};
use std::path::PathBuf;
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
    /// The review cursor moved. The pane scrolls to the current stop.
    Moved,
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
    epoch: u64,
    analysed: Option<GuideId>,
    kept: Option<GuideId>,
    run: GuideRun,
    step: Step,
    stop: Option<usize>,
    reviewed: BTreeSet<(usize, usize)>,
    viewed: HashSet<String>,
    activity: Entity<ClaudeActivity>,
    _subscriptions: [Subscription; 2],
}

impl EventEmitter<GuideEvent> for GuideSession {}

fn node_graphs(chapter: &Chapter) -> Vec<Graph> {
    chapter
        .diagrams
        .iter()
        .filter(|_| chapter.notes.iter().any(|note| note.node.is_some()))
        .filter_map(|diagram| diagram.graph().ok())
        .collect()
}

fn node_label(graph: &Graph, id: &str) -> Option<String> {
    match graph {
        Graph::Flowchart(chart) => chart
            .nodes
            .iter()
            .find(|node| node.id == id)
            .map(|node| node.label.clone()),
        Graph::Sequence(sequence) => sequence
            .participants
            .iter()
            .find(|part| part.id == id)
            .map(|part| part.label.clone()),
        Graph::Source => None,
    }
}

impl GuideSession {
    pub fn new(
        review: Entity<ReviewSession>,
        settings: Arc<Settings>,
        store: Arc<Store>,
        cx: &mut Context<Self>,
    ) -> Self {
        let session = review.read(cx).session_id();
        let activity = review.read(cx).activity().clone();
        let subscriptions = [
            cx.observe(&review, |this, _, cx| this.files_may_have_changed(cx)),
            cx.subscribe(&activity, |this, _, event: &RunEvent, cx| this.apply(event, cx)),
        ];
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
            epoch: 0,
            analysed: None,
            kept: None,
            run: GuideRun::Idle,
            step: Step::Overview,
            stop: None,
            reviewed: BTreeSet::new(),
            viewed: HashSet::new(),
            activity,
            _subscriptions: subscriptions,
        };
        this.stored = this.store.current_guide(session).ok().flatten();
        this.reload_reviewed();
        this.adopt_live_run(cx);
        this.files_may_have_changed(cx);
        this
    }

    fn adopt_live_run(&mut self, cx: &mut Context<Self>) {
        let live = self
            .activity
            .read(cx)
            .live()
            .find(|run| run.kind == RunKind::Guide && run.session == self.session)
            .cloned();
        if let Some(run) = live {
            let chapters = self
                .store
                .load_draft(run.id)
                .ok()
                .flatten()
                .map_or(0, |draft| draft.guide.chapters.len());
            self.run = GuideRun::Writing { run: run.id, chapters };
        }
    }

    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.stored = self.store.current_guide(self.session).ok().flatten();
        self.reload_reviewed();
        self.stop = None;
        self.recompute(cx);
    }

    fn reload_reviewed(&mut self) {
        self.reviewed = self
            .stored
            .as_ref()
            .and_then(|stored| self.store.reviewed_stops(stored.id).ok())
            .unwrap_or_default();
    }

    pub fn stored(&self) -> Option<&StoredGuide> {
        self.stored.as_ref()
    }

    pub fn reading(&self) -> &Reading {
        &self.reading
    }

    pub fn overview_diagrams(&self) -> Vec<Diagram> {
        self.stored
            .as_ref()
            .map(|stored| stored.guide.diagrams.clone())
            .unwrap_or_default()
    }

    pub fn chapter_diagrams(&self) -> Vec<Diagram> {
        let (Some(chapter), Some(stored)) = (self.chapter(), &self.stored) else {
            return Vec::new();
        };
        match chapter.key {
            ChapterKey::Guide(index) => stored.guide.chapters[index].diagrams.clone(),
            ChapterKey::Extra => Vec::new(),
        }
    }

    pub fn run(&self) -> &GuideRun {
        &self.run
    }

    /// The guide run was paused from the Claude area or the pill.
    pub fn paused(&self, cx: &App) -> bool {
        let GuideRun::Writing { run, .. } = self.run else {
            return false;
        };
        self.activity
            .read(cx)
            .run(run)
            .is_some_and(|run| run.state == RunState::Paused)
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

    /// A chapter with stops is read when every stop is reviewed. One without stops is read when its files are viewed.
    pub fn is_read(&self, chapter: &ReadingChapter) -> bool {
        if chapter.key == ChapterKey::Extra {
            return false;
        }
        match self.path_of(chapter) {
            Some((index, stops)) if !stops.is_empty() => stops.all_done(|note| self.reviewed.contains(&(index, note))),
            _ => chapter.files.iter().all(|path| self.viewed.contains(path)),
        }
    }

    fn path_of(&self, chapter: &ReadingChapter) -> Option<(usize, Stops)> {
        let ChapterKey::Guide(index) = chapter.key else {
            return None;
        };
        let notes = &self.stored.as_ref()?.guide.chapters.get(index)?.notes;
        Some((index, Stops::of(notes, &chapter.files)))
    }

    /// The open chapter's guide index and its stops.
    pub fn path(&self) -> Option<(usize, Stops)> {
        self.path_of(self.chapter()?)
    }

    pub fn has_stops(&self) -> bool {
        self.path().is_some_and(|(_, stops)| !stops.is_empty())
    }

    /// The current stop, as an index into the open chapter's stored notes.
    pub fn current_stop(&self) -> Option<usize> {
        self.stop
    }

    pub fn reviewed(&self) -> &BTreeSet<(usize, usize)> {
        &self.reviewed
    }

    /// Reviewed and total stops of a chapter that has stops.
    pub fn progress(&self, chapter: &ReadingChapter) -> Option<(usize, usize)> {
        let (index, stops) = self.path_of(chapter).filter(|(_, stops)| !stops.is_empty())?;
        let done = stops
            .iter()
            .filter(|note| self.reviewed.contains(&(index, *note)))
            .count();
        Some((done, stops.len()))
    }

    /// The stop counts of a chapter that has stops.
    pub fn counts(&self, chapter: &ReadingChapter) -> Option<Counts> {
        let (index, stops) = self.path_of(chapter).filter(|(_, stops)| !stops.is_empty())?;
        Some(Counts::of(&self.stored.as_ref()?.guide.chapters[index].notes, &stops))
    }

    /// The first chapter that still has a stop to review.
    pub fn first_open(&self) -> Option<usize> {
        self.reading
            .chapters
            .iter()
            .position(|chapter| self.progress(chapter).is_some_and(|(done, total)| done < total))
    }

    pub fn start_review(&mut self, cx: &mut Context<Self>) {
        if let Some(index) = self.first_open() {
            self.go(Step::Chapter(index), cx);
        }
    }

    fn entry_stop(&self) -> Option<usize> {
        let (index, stops) = self.path()?;
        stops.entry(|note| self.reviewed.contains(&(index, note)))
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
        self.enter(step, true, cx);
    }

    /// Like `go`, but the pane does not scroll to the chapter's first stop.
    pub fn jump(&mut self, step: Step, cx: &mut Context<Self>) {
        self.enter(step, false, cx);
    }

    fn enter(&mut self, step: Step, reveal: bool, cx: &mut Context<Self>) {
        let step = match step {
            Step::Chapter(index) if index < self.reading.chapters.len() => step,
            Step::Chapter(_) => Step::Overview,
            Step::Overview => Step::Overview,
        };
        if step != self.step {
            self.step = step;
            self.stop = self.entry_stop();
            cx.emit(if reveal && self.stop.is_some() {
                GuideEvent::Moved
            } else {
                GuideEvent::Changed
            });
            cx.notify();
        }
    }

    pub fn go_stop(&mut self, note: usize, cx: &mut Context<Self>) {
        if self.path().is_some_and(|(_, stops)| stops.contains(note)) && self.stop != Some(note) {
            self.stop = Some(note);
            cx.emit(GuideEvent::Moved);
            cx.notify();
        }
    }

    /// The next stop. After the last one, the next chapter.
    pub fn next_stop(&mut self, cx: &mut Context<Self>) {
        let after = self
            .stop
            .zip(self.path())
            .and_then(|(note, (_, stops))| stops.next(note));
        match after {
            Some(note) => self.go_stop(note, cx),
            None => self.next(cx),
        }
    }

    /// The previous stop. Before the first one, the previous chapter.
    pub fn previous_stop(&mut self, cx: &mut Context<Self>) {
        let before = self
            .stop
            .zip(self.path())
            .and_then(|(note, (_, stops))| stops.previous(note));
        match before {
            Some(note) => self.go_stop(note, cx),
            None => self.previous(cx),
        }
    }

    /// Marks the current stop reviewed and moves to the next open one. When none is open, the cursor stays.
    pub fn looks_good(&mut self, cx: &mut Context<Self>) {
        let (Some((index, stops)), Some(note), Some(stored)) = (self.path(), self.stop, &self.stored) else {
            return;
        };
        self.store.set_stop_reviewed(stored.id, index, note, true).ok();
        self.reviewed.insert((index, note));
        if let Some(open) = stops.next_open(note, |other| self.reviewed.contains(&(index, other))) {
            self.stop = Some(open);
        }
        cx.emit(GuideEvent::Moved);
        cx.notify();
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

    /// The stops of the current chapter as the pane shows them: a note whose line moved follows it, and a note
    /// whose code is gone sits under its file header. A linked stop has no diff, so the pane shows nothing for it.
    pub fn pane_notes(&self) -> Vec<PaneNote> {
        use diffpilot_core::anchor::Placement;
        let (Some((index, stops)), Some(stored)) = (self.path(), &self.stored) else {
            return Vec::new();
        };
        let chapter = &stored.guide.chapters[index];
        let placements = self
            .staleness
            .as_ref()
            .and_then(|staleness| staleness.chapters.get(index))
            .map(|status| status.notes.as_slice())
            .unwrap_or_default();
        let graphs = node_graphs(chapter);
        chapter
            .notes
            .iter()
            .enumerate()
            .filter(|(position, note)| !note.is_linked() && stops.contains(*position))
            .map(|(position, note)| {
                let at = note.span.as_ref().map(|span| (span.side, span.line, span.last_line()));
                let (side, line, last) = match (placements.get(position), at) {
                    (Some(Placement::Moved { line, end_line }), Some((side, ..))) => (side, Some(*line), *end_line),
                    (Some(Placement::Outdated), _) | (_, None) => (diffpilot_core::threads::Side::New, None, None),
                    (_, Some((side, line, last))) => (side, Some(line), Some(last)),
                };
                let node = note
                    .node
                    .as_deref()
                    .and_then(|id| graphs.iter().find_map(|graph| node_label(graph, id)));
                PaneNote {
                    path: note.path.clone(),
                    side,
                    line,
                    end_line: line.zip(last).and_then(|(line, last)| (last > line).then_some(last)),
                    text: note.text.clone(),
                    critical: note.critical,
                    kind: note.kind,
                    check: note.check.clone(),
                    number: stops.number(position).unwrap_or(0),
                    total: stops.len(),
                    node,
                    current: self.stop == Some(position),
                }
            })
            .collect()
    }

    /// The current stop when it points into a linked folder. The pane shows it as a read-only excerpt.
    pub fn linked_stop(&self) -> Option<LinkedStop> {
        let ((index, stops), stored, position) = (self.path()?, self.stored.as_ref()?, self.stop?);
        let chapter = &stored.guide.chapters[index];
        let note = chapter.notes.get(position)?;
        let folder = note.folder.clone()?;
        let span = note.span.as_ref();
        let line = span.map(|span| span.line);
        let node = note
            .node
            .as_deref()
            .and_then(|id| node_graphs(chapter).iter().find_map(|graph| node_label(graph, id)));
        Some(LinkedStop {
            note: PaneNote {
                path: note.path.clone(),
                side: diffpilot_core::threads::Side::New,
                line,
                end_line: span.map(Span::last_line).filter(|last| Some(*last) > line),
                text: note.text.clone(),
                critical: note.critical,
                kind: note.kind,
                check: note.check.clone(),
                number: stops.number(position).unwrap_or(0),
                total: stops.len(),
                node,
                current: true,
            },
            folder,
            anchor_text: span.and_then(|span| span.anchor_text.clone()),
        })
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
        self.epoch = self.epoch.wrapping_add(1);
        let epoch = self.epoch;
        let Some(stored) = &self.stored else {
            self.reading = Reading::default();
            self.staleness = None;
            self.analysed = None;
            self.commits = None;
            return self.announce(cx);
        };
        if self.analysed != Some(stored.id) {
            self.reading = Reading::default();
            self.staleness = None;
        }
        let (id, guide, files) = (stored.id, stored.guide.clone(), self.files.clone());
        let repo = self.review.read(cx).host().repo.clone();
        cx.spawn(async move |this, cx| {
            let analysis = cx
                .background_spawn(async move {
                    let reading = reading(&guide, &files);
                    let staleness = (!files.is_empty()).then(|| stale::check(&guide, &files));
                    let commits = match (&staleness, repo) {
                        (Some(staleness), Some(repo)) if staleness.is_stale => stale::commits_since(&repo, &guide),
                        _ => None,
                    };
                    (reading, staleness, commits)
                })
                .await;
            this.update(cx, |session, cx| {
                if session.epoch == epoch {
                    session.land(id, analysis, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn land(
        &mut self,
        id: GuideId,
        (reading, staleness, commits): (Reading, Option<Staleness>, Option<u32>),
        cx: &mut Context<Self>,
    ) {
        self.reading = reading;
        self.staleness = staleness;
        self.commits = commits;
        self.analysed = Some(id);
        if let Step::Chapter(index) = self.step
            && index >= self.reading.chapters.len()
        {
            self.step = Step::Overview;
        }
        if self
            .stop
            .is_none_or(|note| !self.path().is_some_and(|(_, stops)| stops.contains(note)))
        {
            self.stop = self.entry_stop();
        }
        self.announce(cx);
    }

    fn announce(&mut self, cx: &mut Context<Self>) {
        cx.emit(GuideEvent::Changed);
        cx.notify();
    }

    pub fn linked(&self, cx: &App) -> Vec<PathBuf> {
        self.review
            .read(cx)
            .host()
            .repo
            .as_ref()
            .map(|repo| self.settings.linked_folders(repo))
            .unwrap_or_default()
    }

    /// The linked folders differ from the ones the stored guide could read.
    pub fn linked_changed(&self, cx: &App) -> bool {
        self.stored
            .as_ref()
            .is_some_and(|stored| stored.guide.context != linked::readable(&self.linked(cx)))
    }

    pub fn repo(&self, cx: &App) -> Option<PathBuf> {
        self.review.read(cx).host().repo.clone()
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
        if matches!(self.run, GuideRun::Writing { .. }) {
            return;
        }
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
            model: RunKind::Guide.activity().model(&self.settings),
            edit_permission: EditPermission::default(),
        };
        let linked = self.linked(cx);
        self.run = match engine.submit(prompt::request(&target, "", &linked)) {
            Ok(run) => GuideRun::Writing { run, chapters: 0 },
            Err(error) => GuideRun::Failed(error.to_string()),
        };
        self.announce(cx);
    }

    pub fn stop(&mut self, cx: &mut Context<Self>) {
        if let GuideRun::Writing { run, .. } = self.run {
            self.activity.update(cx, |activity, cx| activity.stop(run, cx));
        }
        cx.notify();
    }

    fn apply(&mut self, event: &RunEvent, cx: &mut Context<Self>) {
        let GuideRun::Writing { run: own, .. } = self.run else {
            return;
        };
        match *event {
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
            RunEvent::Finished { run, state } if run == own => self.finish(run, state, cx),
            _ => {}
        }
    }

    fn finish(&mut self, run: RunId, state: RunState, cx: &mut Context<Self>) {
        let written = self.store.guide_of_run(run).ok().flatten().is_some();
        self.run = match state {
            RunState::Stopped => GuideRun::Idle,
            RunState::Done if written => GuideRun::Idle,
            RunState::Done => GuideRun::Failed(NOT_FINISHED_MESSAGE.into()),
            _ => GuideRun::Failed(
                self.activity
                    .read(cx)
                    .run(run)
                    .and_then(|run| run.error.clone())
                    .unwrap_or_else(|| FAILED_MESSAGE.into()),
            ),
        };
        if written {
            self.step = Step::Overview;
            self.stop = None;
            self.kept = None;
            self.reload(cx);
        } else {
            self.announce(cx);
        }
    }
}
