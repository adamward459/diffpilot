//! Starts, queues, stops and records Claude runs. UI-free: the UI subscribes to `RunEvent`s and reads `Run`s.
//!
//! One run goes at a time in a repo. Others wait in a queue. Each run is `claude -p --output-format
//! stream-json --verbose` in the repo folder, with the deny list and the PreToolUse hook from `policy`. A
//! stop sends SIGTERM to the process group, then SIGKILL (see `claude::run_process`).

use crate::cancel::CancelToken;
use crate::claude::{self, ReviewError};
use crate::diff::{self, FileDiff};
use crate::fix;
use crate::guide::build;
use crate::mcp::{Binding, McpBridge, Notice, PermissionRequest};
use crate::paths::{sibling_binary, standardized};
use crate::policy::{EditPermission, PolicyError, ToolPolicy, hook_settings, mcp_config};
use crate::review::{ReviewModel, ReviewRequest};
use crate::run::{Progress, Run, RunId, RunKind, RunOutcome, RunState, RunSummary};
use crate::settings::app_support_dir;
use crate::snapshot::{self, SnapshotError, UndoReport, Until};
use crate::store::{GuideId, SessionId, SessionKey, Store, StoreError, now_ms};
use crate::stream::{self, LineBuffer, StreamEvent};
use crate::threads::ThreadId;
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock, Weak};

#[derive(Debug, Clone, PartialEq)]
pub enum RunEvent {
    Queued {
        run: RunId,
    },
    Started {
        run: RunId,
    },
    Log {
        run: RunId,
        entry: crate::run::LogEntry,
    },
    /// The plan, progress or activity of the run changed. Read it with `RunEngine::run`.
    Updated {
        run: RunId,
    },
    /// The run added to its guide. Read it with `Store::load_draft(run)`, or `Store::current_guide` once it finished.
    GuideChanged {
        run: RunId,
        session: SessionId,
    },
    /// The run wrote to the threads of this session, so a `ThreadStore` there should `refresh()`.
    ThreadsChanged {
        run: RunId,
        session: SessionId,
    },
    /// Claude wants to edit or run a command and the edit permission says to ask. Answer with
    /// `RunEngine::answer_permission`.
    PermissionRequest(PermissionRequest),
    Finished {
        run: RunId,
        state: RunState,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("The claude command was not found. Install Claude Code, then try again.")]
    ClaudeNotFound,
    #[error("Writing a guide needs the diffpilot MCP server, which is not available.")]
    GuideNeedsMcp,
    #[error("The safety hook {0} is missing or does not block a push, so runs are off.")]
    GuardMissing(PathBuf),
    #[error(transparent)]
    Policy(#[from] PolicyError),
    #[error(transparent)]
    Store(#[from] StoreError),
}

#[derive(Debug, thiserror::Error)]
pub enum ChangeError {
    #[error("That run was not found.")]
    UnknownRun,
    #[error("That run took no snapshot, so it has no change to show.")]
    NoSnapshot,
    #[error(transparent)]
    Snapshot(#[from] SnapshotError),
}

fn until_of(taken: &snapshot::Snapshot) -> Until {
    if taken.after_tree.is_some() {
        Until::RunEnd
    } else {
        Until::Now
    }
}

pub const KEEP_SNAPSHOTS: usize = 30;

#[derive(Debug, Clone)]
pub struct RunRequest {
    pub repo: PathBuf,
    pub session: SessionKey,
    pub kind: RunKind,
    pub title: String,
    pub prompt: String,
    pub model: ReviewModel,
    /// Threads the run is to answer. The pill counts them as "2 of 4".
    pub threads: Vec<ThreadId>,
    /// The `git diff` revisions of the comparison under review, for the `get_diff` tool.
    pub revisions: Vec<String>,
    /// Folders outside the repo the run may read, passed as `--add-dir`.
    pub linked: Vec<PathBuf>,
    pub json_schema: Option<String>,
    pub edit_permission: EditPermission,
    /// Keeps a run of a kind that edits from editing, for example a reply on a comparison without the working tree.
    pub read_only: bool,
    pub extra_arguments: Vec<String>,
}

impl RunRequest {
    pub fn new(repo: &Path, session: SessionKey, kind: RunKind, title: &str, prompt: &str, model: ReviewModel) -> Self {
        Self {
            repo: repo.to_path_buf(),
            session,
            kind,
            title: title.to_string(),
            prompt: prompt.to_string(),
            model,
            threads: Vec::new(),
            revisions: Vec::new(),
            linked: Vec::new(),
            json_schema: None,
            edit_permission: EditPermission::default(),
            read_only: false,
            extra_arguments: Vec::new(),
        }
    }

    /// How the run may edit files, or None when it may not.
    pub fn edits(&self) -> Option<EditPermission> {
        (self.kind.edits() && !self.read_only).then_some(self.edit_permission)
    }

    /// A review that returns its findings as structured output.
    pub fn review(request: &ReviewRequest, session: SessionKey, title: &str) -> Self {
        let mut run = Self::new(
            &request.repo,
            session,
            RunKind::Review,
            title,
            &crate::prompt::render(request),
            request.model,
        );
        run.revisions = request.revisions.clone();
        run.json_schema = Some(claude::SCHEMA.to_string());
        run
    }
}

pub struct EngineConfig {
    pub store: Arc<Store>,
    pub claude_binary: Option<PathBuf>,
    pub guard_binary: PathBuf,
    /// Where each run puts its generated settings file.
    pub run_dir: PathBuf,
    /// How many editing runs per review keep their snapshot, and so can still show a change or be undone.
    /// At 0, no run takes a snapshot, so the repo's .git is never written to.
    pub keep_snapshots: usize,
    /// The MCP server. Without it a run cannot add or answer comments, and cannot ask before an edit.
    pub mcp: Option<McpLaunch>,
}

pub struct McpLaunch {
    pub bridge: Arc<McpBridge>,
    /// The `diffpilot-mcp` binary that `claude` starts.
    pub binary: PathBuf,
}

impl McpLaunch {
    pub fn new(bridge: Arc<McpBridge>) -> Self {
        Self {
            bridge,
            binary: sibling_binary("diffpilot-mcp").unwrap_or_else(|| PathBuf::from("diffpilot-mcp")),
        }
    }
}

impl EngineConfig {
    pub fn new(store: Arc<Store>) -> Self {
        let base = app_support_dir().unwrap_or_else(|| std::env::temp_dir().join("diffpilot"));
        Self {
            store,
            claude_binary: None,
            guard_binary: sibling_binary("diffpilot-guard").unwrap_or_else(|| PathBuf::from("diffpilot-guard")),
            run_dir: base.join("runs"),
            keep_snapshots: KEEP_SNAPSHOTS,
            mcp: None,
        }
    }
}

struct Live {
    run: Run,
    request: RunRequest,
    /// The repo as the process sees it. macOS reports `/private/var/...` for a repo opened as `/var/...`.
    repo_real: PathBuf,
    cancel: CancelToken,
    saw_result: bool,
    answered: HashSet<ThreadId>,
}

#[derive(Default)]
struct State {
    live: HashMap<RunId, Live>,
    queues: HashMap<PathBuf, VecDeque<RunId>>,
    running: HashMap<PathBuf, RunId>,
}

struct Inner {
    config: EngineConfig,
    claude_binary: Mutex<Option<PathBuf>>,
    state: Mutex<State>,
    subscribers: Mutex<Vec<async_channel::Sender<RunEvent>>>,
    guard_checked: OnceLock<bool>,
}

#[derive(Clone)]
pub struct RunEngine {
    inner: Arc<Inner>,
}

enum Ending {
    Done,
    Stopped,
    Failed(String),
}

impl RunEngine {
    pub fn new(config: EngineConfig) -> Self {
        let _ = config.store.fail_stale_runs();
        Self {
            inner: Arc::new(Inner {
                claude_binary: Mutex::new(config.claude_binary.clone()),
                config,
                state: Mutex::default(),
                subscribers: Mutex::default(),
                guard_checked: OnceLock::new(),
            }),
        }
    }

    pub fn store(&self) -> &Arc<Store> {
        &self.inner.config.store
    }

    pub fn has_mcp(&self) -> bool {
        self.inner.has_mcp()
    }

    /// Runs that start from now on use this `claude`. None searches for it.
    pub fn set_claude_binary(&self, path: Option<PathBuf>) {
        *self.inner.claude_binary.lock().unwrap() = path;
    }

    /// A review request that fits this engine: findings arrive as comments while the run goes when the MCP
    /// server is there, and as structured output at the end when it is not.
    pub fn review_request(&self, request: &ReviewRequest, session: SessionKey, title: &str) -> RunRequest {
        if !self.has_mcp() {
            return RunRequest::review(request, session, title);
        }
        let mut run = RunRequest::review(request, session, title);
        run.prompt = crate::prompt::render_with_tools(request);
        run.json_schema = None;
        run
    }

    /// Answers a `RunEvent::PermissionRequest`. False when the request is gone, for example because the run ended.
    pub fn answer_permission(&self, id: u64, allow: bool) -> bool {
        self.inner
            .config
            .mcp
            .as_ref()
            .is_some_and(|mcp| mcp.bridge.answer_permission(id, allow))
    }

    /// Every subscriber gets every event from now on.
    pub fn subscribe(&self) -> async_channel::Receiver<RunEvent> {
        let (sender, receiver) = async_channel::unbounded();
        self.inner.subscribers.lock().unwrap().push(sender);
        receiver
    }

    /// Queues the run, and starts it right away when the repo has no run going.
    pub fn submit(&self, request: RunRequest) -> Result<RunId, EngineError> {
        let inner = &self.inner;
        if inner.claude().is_none() {
            return Err(EngineError::ClaudeNotFound);
        }
        inner.check_guard()?;
        if request.kind == RunKind::Guide && !inner.has_mcp() {
            return Err(EngineError::GuideNeedsMcp);
        }
        ToolPolicy::new(request.kind, request.edits(), inner.has_mcp())?;
        let session = inner.config.store.session(&request.session)?;
        let id = inner.config.store.insert_run(session, request.kind, &request.title)?;
        let mut run = Run::new(
            id,
            session,
            &request.repo,
            request.kind,
            &request.title,
            request.model,
            now_ms(),
        );
        if !request.threads.is_empty() {
            run.threads = request.threads.clone();
            run.units = Some(Progress {
                done: 0,
                total: request.threads.len() as u32,
            });
        }
        inner.config.store.save_run(&run)?;
        let repo = standardized(&request.repo);
        let start_now = {
            let mut state = inner.state.lock().unwrap();
            state.live.insert(
                id,
                Live {
                    run,
                    repo_real: std::fs::canonicalize(&request.repo).unwrap_or_else(|_| request.repo.clone()),
                    request,
                    cancel: CancelToken::new(),
                    saw_result: false,
                    answered: HashSet::new(),
                },
            );
            match state.running.entry(repo.clone()) {
                Entry::Occupied(_) => {
                    state.queues.entry(repo).or_default().push_back(id);
                    false
                }
                Entry::Vacant(slot) => {
                    slot.insert(id);
                    true
                }
            }
        };
        inner.emit(RunEvent::Queued { run: id });
        if start_now {
            Inner::spawn(inner.clone(), id);
        }
        Ok(id)
    }

    /// Stops a running run, or takes a queued one out of the queue. Returns false for an unknown or finished run.
    pub fn stop(&self, id: RunId) -> bool {
        let queued = {
            let mut state = self.inner.state.lock().unwrap();
            let Some(live) = state.live.get(&id) else { return false };
            match live.run.state {
                RunState::Running => {
                    live.cancel.cancel();
                    return true;
                }
                RunState::Queued => {
                    for queue in state.queues.values_mut() {
                        queue.retain(|queued| *queued != id);
                    }
                    true
                }
                _ => false,
            }
        };
        if queued {
            self.inner.finish(id, &Ending::Stopped);
        }
        queued
    }

    /// The run now going in `repo`, if any.
    pub fn active(&self, repo: &Path) -> Option<Run> {
        let state = self.inner.state.lock().unwrap();
        let id = state.running.get(&standardized(repo))?;
        state.live.get(id).map(|live| live.run.clone())
    }

    /// The runs waiting in `repo`, first to start first.
    pub fn queued(&self, repo: &Path) -> Vec<Run> {
        let state = self.inner.state.lock().unwrap();
        state
            .queues
            .get(&standardized(repo))
            .into_iter()
            .flatten()
            .filter_map(|id| state.live.get(id).map(|live| live.run.clone()))
            .collect()
    }

    /// A run that is going or waiting, or a finished one from the database.
    pub fn run(&self, id: RunId) -> Option<Run> {
        if let Some(live) = self.inner.state.lock().unwrap().live.get(&id) {
            return Some(live.run.clone());
        }
        self.inner.config.store.load_run(id).ok().flatten()
    }

    /// Past Runs of one review, newest first.
    pub fn past_runs(&self, key: &SessionKey, limit: usize) -> Result<Vec<RunSummary>, StoreError> {
        let session = self.inner.config.store.session(key)?;
        self.inner.config.store.run_summaries(session, limit)
    }

    /// What the run changed, as a diff from the snapshot taken before it. `paths` limits it to those files.
    pub fn show_change(&self, id: RunId, paths: &[String]) -> Result<Vec<FileDiff>, ChangeError> {
        let (run, taken) = self.snapshot_of(id)?;
        Ok(snapshot::diff(&run.repo, &taken, until_of(&taken), paths)?)
    }

    /// Puts the files of a finished run back as they were before it. A file edited since stays unless `force`.
    pub fn undo_run(&self, id: RunId, force: bool) -> Result<UndoReport, ChangeError> {
        let (mut run, taken) = self.snapshot_of(id)?;
        let report = snapshot::undo(&run.repo, &taken, force)?;
        if report.conflicts.is_empty() {
            run.undone_at_ms = Some(now_ms());
            let _ = self.inner.config.store.save_run(&run);
            self.inner.with_run(id, |live| live.run.undone_at_ms = run.undone_at_ms);
        }
        Ok(report)
    }

    fn snapshot_of(&self, id: RunId) -> Result<(Run, snapshot::Snapshot), ChangeError> {
        let run = self.run(id).ok_or(ChangeError::UnknownRun)?;
        let taken = run.snapshot.clone().ok_or(ChangeError::NoSnapshot)?;
        Ok((run, taken))
    }

    /// Counts a thread as answered for the "2 of 4" progress. The bridge calls this when Claude replies to a
    /// thread or marks it addressed.
    pub fn note_thread_answered(&self, run: RunId, thread: ThreadId) {
        self.inner.note_thread_answered(run, thread);
    }
}

impl Inner {
    fn emit(&self, event: RunEvent) {
        self.subscribers
            .lock()
            .unwrap()
            .retain(|subscriber| subscriber.try_send(event.clone()).is_ok());
    }

    fn claude(&self) -> Option<PathBuf> {
        let chosen = self.claude_binary.lock().unwrap().clone();
        chosen.or_else(claude::claude_binary)
    }

    fn has_mcp(&self) -> bool {
        self.config.mcp.is_some()
    }

    fn note_thread_answered(&self, run: RunId, thread: ThreadId) {
        let changed = {
            let mut state = self.state.lock().unwrap();
            let Some(live) = state.live.get_mut(&run) else { return };
            if !live.request.threads.contains(&thread) || !live.answered.insert(thread) {
                false
            } else {
                live.run.units = Some(Progress {
                    done: live.answered.len() as u32,
                    total: live.request.threads.len() as u32,
                });
                true
            }
        };
        if changed {
            self.emit(RunEvent::Updated { run });
        }
    }

    fn notice(&self, run: RunId, session: SessionId, notice: Notice) {
        match notice {
            Notice::ThreadsChanged => self.emit(RunEvent::ThreadsChanged { run, session }),
            Notice::GuideChanged => self.emit(RunEvent::GuideChanged { run, session }),
            Notice::ThreadAnswered(thread) => self.note_thread_answered(run, thread),
            Notice::Permission(request) => self.emit(RunEvent::PermissionRequest(request)),
        }
    }

    /// Registers the run with the bridge and writes the `--mcp-config` file. Returns the token and the file.
    fn attach_mcp(self: &Arc<Self>, id: RunId, request: &RunRequest, dir: &Path) -> Option<(String, PathBuf)> {
        let mcp = self.config.mcp.as_ref()?;
        let session = self.with_run(id, |live| live.run.session)?;
        let weak: Weak<Inner> = Arc::downgrade(self);
        let token = mcp.bridge.register(Binding {
            run: id,
            session,
            repo: request.repo.clone(),
            kind: request.kind,
            revisions: request.revisions.clone(),
            linked: request.linked.clone(),
            threads: request.threads.clone(),
            edits: request.edits(),
            notify: Arc::new(move |notice| {
                if let Some(inner) = weak.upgrade() {
                    inner.notice(id, session, notice);
                }
            }),
        });
        let file = dir.join("mcp.json");
        let config = mcp_config(&mcp.binary, mcp.bridge.socket_path(), &token);
        let written = std::fs::write(&file, serde_json::to_vec_pretty(&config).unwrap_or_default()).and_then(|()| {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600))
        });
        if written.is_err() {
            mcp.bridge.unregister(&token);
            return None;
        }
        Some((token, file))
    }

    /// A run with no working hook would be allowed to push, so it never starts without one.
    fn check_guard(&self) -> Result<(), EngineError> {
        let works = *self
            .guard_checked
            .get_or_init(|| guard_blocks_a_push(&self.config.guard_binary));
        if works {
            Ok(())
        } else {
            Err(EngineError::GuardMissing(self.config.guard_binary.clone()))
        }
    }

    fn spawn(this: Arc<Self>, id: RunId) {
        let _ = std::thread::Builder::new()
            .name(format!("diffpilot-run-{}", id.0))
            .spawn(move || this.execute(id));
    }

    fn execute(self: Arc<Self>, id: RunId) {
        let Some((request, cancel)) = self.begin(id) else {
            return;
        };
        let ending = self.drive(id, &request, &cancel);
        let ending = if request.kind == RunKind::Guide {
            self.settle_guide(id, &request, ending)
        } else {
            ending
        };
        self.finish(id, &ending);
    }

    /// A guide run is done when its guide is saved. A guide Claude built but did not finish is finished here,
    /// when it has an overview and a chapter. A run that did not end well keeps no draft.
    fn settle_guide(&self, id: RunId, request: &RunRequest, ending: Ending) -> Ending {
        let store = &self.config.store;
        if !matches!(ending, Ending::Done) {
            let _ = store.discard_draft(id);
            return ending;
        }
        let saved = match store.guide_of_run(id) {
            Ok(Some(saved)) => Ok(saved),
            Ok(None) => self.finish_draft(id, request),
            Err(error) => Err(error.to_string()),
        };
        match saved {
            Ok(guide) => {
                self.with_run(id, |live| live.run.outcome = Some(RunOutcome::Guide(guide)));
                self.emit_guide_changed(id);
                Ending::Done
            }
            Err(message) => {
                let _ = store.discard_draft(id);
                Ending::Failed(message)
            }
        }
    }

    fn finish_draft(&self, id: RunId, request: &RunRequest) -> Result<GuideId, String> {
        let store = &self.config.store;
        let draft = store
            .load_draft(id)
            .map_err(|error| error.to_string())?
            .ok_or("Claude did not write a guide.")?;
        let mut guide = draft.guide;
        let files = diff::load_revisions(&request.repo, &request.revisions).map_err(|error| error.to_string())?;
        build::finish(&mut guide, &files).map_err(|error| format!("Claude did not finish the guide. {error}"))?;
        store
            .complete_draft(id, &guide)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "The guide was not saved.".to_string())
    }

    fn emit_guide_changed(&self, id: RunId) {
        if let Some(session) = self.with_run(id, |live| live.run.session) {
            self.emit(RunEvent::GuideChanged { run: id, session });
        }
    }

    fn begin(&self, id: RunId) -> Option<(RunRequest, CancelToken)> {
        let (request, cancel, run) = {
            let mut state = self.state.lock().unwrap();
            let live = state
                .live
                .get_mut(&id)
                .filter(|live| live.run.state == RunState::Queued)?;
            live.run.state = RunState::Running;
            live.run.started_at_ms = Some(now_ms());
            (live.request.clone(), live.cancel.clone(), live.run.clone())
        };
        let _ = self.config.store.save_run(&run);
        self.emit(RunEvent::Started { run: id });
        Some((request, cancel))
    }

    fn with_run<T>(&self, id: RunId, change: impl FnOnce(&mut Live) -> T) -> Option<T> {
        self.state.lock().unwrap().live.get_mut(&id).map(change)
    }

    fn drive(self: &Arc<Self>, id: RunId, request: &RunRequest, cancel: &CancelToken) -> Ending {
        let Some(binary) = self.claude() else {
            return Ending::Failed(EngineError::ClaudeNotFound.to_string());
        };
        let policy = match ToolPolicy::new(request.kind, request.edits(), self.has_mcp()) {
            Ok(policy) => policy,
            Err(error) => return Ending::Failed(error.to_string()),
        };
        let dir = self.config.run_dir.join(format!("run-{}", id.0));
        let settings = dir.join("settings.json");
        let prepared = std::fs::create_dir_all(&dir).and_then(|()| {
            std::fs::write(
                &settings,
                serde_json::to_vec_pretty(&hook_settings(&self.config.guard_binary)).unwrap_or_default(),
            )
        });
        if let Err(error) = prepared {
            return Ending::Failed(format!("Could not prepare the run: {error}"));
        }
        let snapshots = request.edits().is_some() && self.config.keep_snapshots > 0;
        if snapshots {
            match snapshot::take(&request.repo, id) {
                Ok(taken) => {
                    self.with_run(id, |live| live.run.snapshot = Some(taken));
                }
                Err(error) => {
                    let _ = std::fs::remove_dir_all(&dir);
                    return Ending::Failed(error.to_string());
                }
            }
        }

        let mut command = claude::claude_command(&binary, &request.repo);
        command.args([
            "-p",
            &request.prompt,
            "--output-format",
            "stream-json",
            "--verbose",
            "--no-session-persistence",
            "--strict-mcp-config",
            "--model",
            request.model.name.as_str(),
            "--effort",
            request.model.effort.as_str(),
            "--settings",
        ]);
        command.arg(&settings);
        let attached = self.attach_mcp(id, request, &dir);
        if self.has_mcp() && attached.is_none() {
            let _ = std::fs::remove_dir_all(&dir);
            return Ending::Failed("Could not set up the diffpilot MCP server for this run.".into());
        }
        if let Some((_, file)) = &attached {
            command.arg("--mcp-config").arg(file);
        }
        if let Some(schema) = &request.json_schema {
            command.args(["--json-schema", schema]);
        }
        for folder in &request.linked {
            command.arg("--add-dir").arg(folder);
        }
        command.args(policy.arguments()).args(&request.extra_arguments);

        let lines = Arc::new(Mutex::new(LineBuffer::default()));
        let (feed, engine) = (lines.clone(), Arc::clone(self));
        let result = claude::run_process(command, cancel, move |chunk| {
            let complete = feed.lock().unwrap().push(chunk);
            for line in complete {
                engine.apply_line(id, &line);
            }
            true
        });
        if let Some(tail) = lines.lock().unwrap().finish() {
            self.apply_line(id, &tail);
        }
        if let (Some((token, _)), Some(mcp)) = (&attached, &self.config.mcp) {
            mcp.bridge.unregister(token);
        }
        let ending = self.ending_of(id, result);
        if snapshots {
            self.record_changes(id, &request.repo);
        }
        if request.edits().is_some() {
            self.record_thread_files(id, request);
        }
        let _ = std::fs::remove_dir_all(&dir);
        ending
    }

    /// The run that just fell out of the newest `keep_snapshots` gives up its refs.
    fn forget_old_snapshots(&self, id: RunId, repo: &Path) {
        let Some(session) = self.with_run(id, |live| live.run.session) else {
            return;
        };
        let keep = self.config.keep_snapshots;
        let Ok(past) = self.config.store.run_summaries(session, keep + 1) else {
            return;
        };
        if let Some(old) = past.get(keep) {
            let _ = snapshot::forget(repo, old.id);
        }
    }

    fn ending_of(&self, id: RunId, result: Result<claude::Finished, ReviewError>) -> Ending {
        match result {
            Err(ReviewError::Cancelled) => Ending::Stopped,
            Err(error) => Ending::Failed(error.to_string()),
            Ok(finished) => {
                let (error, saw_result, review_missing) = self
                    .with_run(id, |live| {
                        (
                            live.run.error.clone(),
                            live.saw_result,
                            live.run.kind == RunKind::Review && live.run.outcome.is_none(),
                        )
                    })
                    .unwrap_or_default();
                if let Some(error) = error {
                    Ending::Failed(error)
                } else if !finished.status.success() {
                    let message = String::from_utf8_lossy(&finished.stderr).trim().to_string();
                    Ending::Failed(if message.is_empty() {
                        match finished.status.code() {
                            Some(code) => format!("claude exited with status {code}."),
                            None => "claude was stopped by a signal.".into(),
                        }
                    } else {
                        message
                    })
                } else if !saw_result {
                    Ending::Failed("Claude ended without a result.".into())
                } else if review_missing {
                    Ending::Failed("Claude returned no review.".into())
                } else {
                    Ending::Done
                }
            }
        }
    }

    fn record_changes(&self, id: RunId, repo: &Path) {
        let Some(Some(mut taken)) = self.with_run(id, |live| live.run.snapshot.clone()) else {
            return;
        };
        let outcome = snapshot::finish(repo, &mut taken)
            .and_then(|()| snapshot::changed_files(repo, &taken, snapshot::Until::RunEnd));
        self.forget_old_snapshots(id, repo);
        self.with_run(id, |live| {
            live.run.snapshot = Some(taken);
            match outcome {
                Ok(files) => live.run.changed_files = files,
                Err(error) => live.run.error = live.run.error.take().or(Some(error.to_string())),
            }
        });
    }

    fn record_thread_files(&self, id: RunId, request: &RunRequest) {
        if request.threads.is_empty() {
            return;
        }
        let Some((changed, session)) = self.with_run(id, |live| (live.run.changed_files.clone(), live.run.session))
        else {
            return;
        };
        let updated = fix::record_files(&self.config.store, id, &request.threads, &changed);
        if updated.is_ok_and(|threads| !threads.is_empty()) {
            self.emit(RunEvent::ThreadsChanged { run: id, session });
        }
    }

    fn apply_line(&self, id: RunId, line: &str) {
        let events = stream::parse_line(line);
        if events.is_empty() {
            return;
        }
        let entries = {
            let mut state = self.state.lock().unwrap();
            let Some(live) = state
                .live
                .get_mut(&id)
                .filter(|live| live.run.state == RunState::Running)
            else {
                return;
            };
            let mut entries = Vec::new();
            for event in &events {
                if matches!(event, StreamEvent::Result(_)) {
                    live.saw_result = true;
                }
                entries.extend(live.run.apply(event, &live.repo_real, now_ms()));
            }
            entries
        };
        for entry in entries {
            self.emit(RunEvent::Log { run: id, entry });
        }
        self.emit(RunEvent::Updated { run: id });
    }

    /// Records how the run ended, tells subscribers, and starts the next queued run of the repo.
    fn finish(self: &Arc<Self>, id: RunId, ending: &Ending) {
        let (run, next) = {
            let mut state = self.state.lock().unwrap();
            let Some(live) = state.live.get_mut(&id) else { return };
            let (final_state, error) = match ending {
                Ending::Done => (RunState::Done, None),
                Ending::Stopped => (RunState::Stopped, None),
                Ending::Failed(message) => (RunState::Failed, Some(message.clone())),
            };
            live.run.state = final_state;
            live.run.finished_at_ms = Some(now_ms());
            if error.is_some() {
                live.run.error = error;
            }
            live.run.activity = None;
            let run = live.run.clone();
            let repo = standardized(&live.request.repo);
            let was_running = state.running.get(&repo) == Some(&id);
            let next = if was_running {
                state.running.remove(&repo);
                let next = state.queues.get_mut(&repo).and_then(VecDeque::pop_front);
                if let Some(next) = next {
                    state.running.insert(repo, next);
                }
                next
            } else {
                None
            };
            (run, next)
        };
        let _ = self.config.store.save_run(&run);
        self.emit(RunEvent::Finished {
            run: id,
            state: run.state,
        });
        if let Some(next) = next {
            Self::spawn(self.clone(), next);
        }
    }
}

/// The helper binaries that `cargo build -p diffpilot-core --bins` puts next to the app and that are not there.
pub fn missing_helpers() -> Vec<&'static str> {
    ["diffpilot-guard", "diffpilot-mcp"]
        .into_iter()
        .filter(|name| sibling_binary(name).is_none())
        .collect()
}

fn guard_blocks_a_push(guard: &Path) -> bool {
    use std::io::Write;
    let probe = r#"{"tool_name":"Bash","tool_input":{"command":"git push"}}"#;
    let Ok(mut child) = Command::new(guard)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    else {
        return false;
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(probe.as_bytes());
    }
    child.wait_with_output().is_ok_and(|output| {
        output.status.code() == Some(2) && String::from_utf8_lossy(&output.stderr).contains(crate::guard::BLOCK_MARKER)
    })
}
