use crate::runs::Runs;
use diffpilot_core::run::{Run, RunId, RunSummary};
use diffpilot_core::runner::{RunEngine, RunEvent};
use diffpilot_core::store::SessionKey;
use gpui::{App, AppContext as _, Context, Entity, EventEmitter, Global, Task, WeakEntity};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

const RECENT: usize = 20;

/// The Claude runs of one repo: the run that holds the repo, the queue behind it, and the runs that finished while
/// the app watched. Every Claude surface reads it, so they all show the same thing. It passes on the engine's
/// events for the runs of its repo.
pub struct ClaudeActivity {
    repo: PathBuf,
    engine: Option<RunEngine>,
    known: HashSet<RunId>,
    foreign: HashSet<RunId>,
    holding: Option<Run>,
    queued: Vec<Run>,
    recent: VecDeque<Run>,
    summaries: HashMap<RunId, String>,
    _events: Option<Task<()>>,
}

impl EventEmitter<RunEvent> for ClaudeActivity {}

#[derive(Default)]
struct Activities(HashMap<PathBuf, WeakEntity<ClaudeActivity>>);

impl Global for Activities {}

impl ClaudeActivity {
    /// The one model of `repo`, made on first use.
    pub fn for_repo(repo: &Path, cx: &mut App) -> Entity<Self> {
        let engine = cx.try_global::<Runs>().map(|runs| runs.engine().clone());
        let found = cx
            .default_global::<Activities>()
            .0
            .get(repo)
            .and_then(WeakEntity::upgrade)
            .filter(|found| engine.is_none() || found.read(cx).engine.is_some());
        if let Some(found) = found {
            return found;
        }
        let made = cx.new(|cx| Self::new(repo.to_path_buf(), engine, cx));
        let activities = &mut cx.default_global::<Activities>().0;
        activities.retain(|_, weak| weak.upgrade().is_some());
        activities.insert(repo.to_path_buf(), made.downgrade());
        made
    }

    fn new(repo: PathBuf, engine: Option<RunEngine>, cx: &mut Context<Self>) -> Self {
        let events = engine.as_ref().map(|engine| {
            let events = engine.subscribe();
            cx.spawn(async move |this, cx| {
                while let Ok(first) = events.recv().await {
                    let mut batch = vec![first];
                    while let Ok(more) = events.try_recv() {
                        batch.push(more);
                    }
                    if this.update(cx, |activity, cx| activity.apply(batch, cx)).is_err() {
                        return;
                    }
                }
            })
        });
        let mut this = Self {
            repo,
            engine,
            known: HashSet::new(),
            foreign: HashSet::new(),
            holding: None,
            queued: Vec::new(),
            recent: VecDeque::new(),
            summaries: HashMap::new(),
            _events: events,
        };
        this.sync();
        let live: Vec<RunId> = this.live().map(|run| run.id).collect();
        this.known.extend(live);
        this
    }

    pub fn engine(&self) -> Option<&RunEngine> {
        self.engine.as_ref()
    }

    /// The run that holds the repo, or else the one that finished last.
    pub fn current(&self) -> Option<&Run> {
        self.holding.as_ref().or(self.recent.front())
    }

    /// Runs waiting behind the one that holds the repo, first to start first.
    pub fn queued(&self) -> &[Run] {
        &self.queued
    }

    pub fn is_busy(&self) -> bool {
        self.holding.is_some() || !self.queued.is_empty()
    }

    /// Runs that are going or waiting.
    pub fn live(&self) -> impl Iterator<Item = &Run> {
        self.holding.iter().chain(&self.queued)
    }

    /// A run that is going, waiting, or finished while the app watched.
    pub fn run(&self, id: RunId) -> Option<&Run> {
        self.live().chain(&self.recent).find(|run| run.id == id)
    }

    /// What a finished run made, for the status line.
    pub fn summary(&self, id: RunId) -> Option<&str> {
        self.summaries.get(&id).map(String::as_str)
    }

    /// Past runs of every kind on the branch of `key`, newest first.
    pub fn past_runs(&self, key: &SessionKey, limit: usize) -> Vec<RunSummary> {
        self.engine
            .as_ref()
            .and_then(|engine| engine.past_runs(key, limit).ok())
            .unwrap_or_default()
    }

    /// A run with its full log, from memory or from the database.
    pub fn load(&self, id: RunId) -> Option<Run> {
        self.run(id)
            .cloned()
            .or_else(|| self.engine.as_ref().and_then(|engine| engine.run(id)))
    }

    /// Shows a run right after its submit, before the engine's event for it arrives.
    pub fn track(&mut self, id: RunId, cx: &mut Context<Self>) {
        self.known.insert(id);
        self.refresh(cx);
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.sync();
        cx.notify();
    }

    pub fn stop(&mut self, id: RunId, cx: &mut Context<Self>) {
        if self.engine.as_ref().is_some_and(|engine| engine.stop(id)) {
            self.refresh(cx);
        }
    }

    /// Ends the claude process of the running run and keeps the run. It holds the repo until it goes on or stops.
    pub fn pause(&mut self, id: RunId, cx: &mut Context<Self>) {
        if self.engine.as_ref().is_some_and(|engine| engine.pause(id)) {
            self.refresh(cx);
        }
    }

    pub fn resume(&mut self, id: RunId, cx: &mut Context<Self>) {
        if self.engine.as_ref().is_some_and(|engine| engine.resume(id)) {
            self.refresh(cx);
        }
    }

    /// Stops the run that holds the repo.
    pub fn stop_current(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.holding.as_ref().map(|run| run.id) {
            self.stop(id, cx);
        }
    }

    fn sync(&mut self) {
        let Some(engine) = &self.engine else { return };
        self.holding = engine.active(&self.repo);
        self.queued = engine.queued(&self.repo);
    }

    fn ours(&mut self, id: RunId) -> bool {
        if self.known.contains(&id) {
            return true;
        }
        if self.foreign.contains(&id) {
            return false;
        }
        let ours = self
            .engine
            .as_ref()
            .and_then(|engine| engine.run(id))
            .is_some_and(|run| same_repo(&run.repo, &self.repo));
        if ours {
            self.known.insert(id);
        } else {
            self.foreign.insert(id);
        }
        ours
    }

    fn apply(&mut self, events: Vec<RunEvent>, cx: &mut Context<Self>) {
        let ours: Vec<RunEvent> = events.into_iter().filter(|event| self.ours(event.run())).collect();
        if ours.is_empty() {
            return;
        }
        for event in &ours {
            if let RunEvent::Finished { run, .. } = event {
                self.remember(*run);
            }
        }
        self.sync();
        for event in ours {
            cx.emit(event);
        }
        cx.notify();
    }

    fn remember(&mut self, id: RunId) {
        let Some(engine) = &self.engine else { return };
        let Some(finished) = engine.run(id) else { return };
        let summary = finished.kind.activity().summary(&finished, engine.store());
        self.summaries.insert(id, summary);
        self.recent.retain(|known| known.id != id);
        self.recent.push_front(finished);
        if self.recent.len() > RECENT
            && let Some(dropped) = self.recent.pop_back()
        {
            self.summaries.remove(&dropped.id);
        }
    }
}

fn same_repo(path: &Path, repo: &Path) -> bool {
    if path == repo {
        return true;
    }
    matches!(
        (std::fs::canonicalize(path), std::fs::canonicalize(repo)),
        (Ok(one), Ok(two)) if one == two
    )
}
