#![allow(dead_code)]

use diffpilot::review::{ReviewEvent, ReviewHost, ReviewSession};
use diffpilot::testing::{FakeClaude, fake_claude};
use diffpilot_core::cancel::CancelToken;
use diffpilot_core::changes::FileStatus;
use diffpilot_core::claude::Replier;
use diffpilot_core::comparison::Comparison;
use diffpilot_core::diff::{DiffLine, FileDiff, Hunk, LineKind};
use diffpilot_core::review::{Finding, ReviewResult, Severity};
use diffpilot_core::run::{Run, RunState};
use diffpilot_core::settings::Settings;
use diffpilot_core::store::Store;
use diffpilot_core::threads::{ReviewThread, Side, ThreadAnchor, ThreadId};
use gpui::{AppContext, Context, Entity, Render, TestAppContext, VisualTestContext, Window};
use gpui_component::Root;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct StubReplier {
    pub senders: Mutex<Vec<async_channel::Sender<String>>>,
    pub tokens: Mutex<Vec<CancelToken>>,
    pub asked: Mutex<Vec<ReviewThread>>,
}

impl StubReplier {
    pub fn send(&self, chunk: &str) {
        for sender in self.senders.lock().unwrap().iter() {
            sender.send_blocking(chunk.into()).unwrap();
        }
    }

    pub fn finish(&self) {
        self.senders.lock().unwrap().clear();
    }

    pub fn asked(&self) -> usize {
        self.asked.lock().unwrap().len()
    }
}

impl Replier for StubReplier {
    fn reply(&self, thread: &ReviewThread, cancel: &CancelToken) -> async_channel::Receiver<String> {
        let (sender, receiver) = async_channel::unbounded();
        self.senders.lock().unwrap().push(sender);
        self.tokens.lock().unwrap().push(cancel.clone());
        self.asked.lock().unwrap().push(thread.clone());
        receiver
    }
}

pub fn finding(path: &str, line: Option<u32>, severity: Severity, title: &str) -> Finding {
    Finding {
        path: path.into(),
        line,
        severity,
        title: title.into(),
        body: format!("{title} body"),
    }
}

pub fn result(summary: &str, findings: Vec<Finding>) -> ReviewResult {
    ReviewResult {
        summary: summary.into(),
        findings,
    }
}

pub fn two_findings() -> ReviewResult {
    result(
        "Looks fine.",
        vec![
            finding("src/a.ts", Some(2), Severity::MustFix, "Bad"),
            finding("elsewhere.py", None, Severity::Nit, "Name"),
        ],
    )
}

pub fn file() -> FileDiff {
    let mut file = FileDiff::new("src/a.ts", FileStatus::Modified);
    file.hunks.push(Hunk {
        header: "@@ -1,1 +1,3 @@".into(),
        lines: (1..=3)
            .map(|n| DiffLine {
                kind: LineKind::Added,
                old: None,
                new: Some(n),
                text: format!("line {n}"),
            })
            .collect(),
    });
    file
}

pub struct Fixture {
    pub session: Entity<ReviewSession>,
    pub claude: FakeClaude,
    pub replier: Arc<StubReplier>,
    pub settings: Arc<Settings>,
    pub store: Arc<Store>,
    pub events: Rc<RefCell<Vec<ReviewEvent>>>,
    _repo: tempfile::TempDir,
}

/// Call `claude.hold()` to keep runs going.
pub fn fixture(cx: &mut TestAppContext, result: ReviewResult) -> Fixture {
    let claude = fake_claude(cx);
    claude.review_with(&result);
    claude.finish();
    let repo = tempfile::tempdir().unwrap();
    let replier = Arc::new(StubReplier::default());
    let settings = Arc::new(Settings::in_memory());
    let store = claude.store();
    let mut host = ReviewHost::new(Some(repo.path().to_path_buf()), Comparison::checked_out("main", true));
    host.files = Arc::from([Arc::new(file())]);
    host.visible_path = Some("src/a.ts".into());
    let session = {
        let (replier, settings, store) = (replier.clone(), settings.clone(), store.clone());
        cx.new(move |cx| ReviewSession::new(host, settings, store, replier, cx))
    };
    let events = Rc::new(RefCell::new(Vec::new()));
    let sink = events.clone();
    cx.update(|cx| {
        cx.subscribe(&session, move |_, event: &ReviewEvent, _| {
            sink.borrow_mut().push(*event)
        })
        .detach();
    });
    Fixture {
        session,
        claude,
        replier,
        settings,
        store,
        events,
        _repo: repo,
    }
}

pub fn session_on(cx: &mut TestAppContext, store: &Arc<Store>, comparison: Comparison) -> Entity<ReviewSession> {
    let mut host = ReviewHost::new(Some(std::env::temp_dir()), comparison);
    host.branch_name = Some("feature".into());
    host.files = Arc::from([Arc::new(file())]);
    let (store, settings) = (store.clone(), Arc::new(Settings::in_memory()));
    let replier = Arc::new(StubReplier::default());
    cx.new(move |cx| ReviewSession::new(host, settings, store, replier, cx))
}

pub fn wait_until(cx: &mut TestAppContext, what: &str, mut done: impl FnMut(&mut TestAppContext) -> bool) {
    cx.executor().allow_parking();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

impl Fixture {
    pub fn current(&self, cx: &mut TestAppContext) -> Option<Run> {
        self.session.read_with(cx, |session, _| session.current().cloned())
    }

    pub fn state(&self, cx: &mut TestAppContext) -> Option<RunState> {
        self.current(cx).map(|run| run.state)
    }

    pub fn start(&self, cx: &mut TestAppContext) {
        self.session.update(cx, |session, cx| session.start_review(cx));
    }

    pub fn review(&self, cx: &mut TestAppContext) {
        self.start(cx);
        self.wait_for_end(cx);
    }

    pub fn wait_for_end(&self, cx: &mut TestAppContext) {
        wait_until(cx, "the run to end", |cx| {
            self.state(cx).is_some_and(RunState::is_finished) && self.session.read_with(cx, |s, _| !s.is_busy())
        });
    }

    pub fn wait_for_launches(&self, cx: &mut TestAppContext, count: usize) {
        wait_until(cx, "claude to start", |_| self.claude.launches() >= count);
    }

    pub fn problem(&self, cx: &mut TestAppContext) -> Option<String> {
        self.session.read_with(cx, |session, _| session.review_problem())
    }

    pub fn thread(&self, cx: &mut TestAppContext, id: ThreadId) -> Option<ReviewThread> {
        self.session.read_with(cx, |session, _| session.thread(id).cloned())
    }

    pub fn threads(&self, cx: &mut TestAppContext) -> Vec<ReviewThread> {
        self.session
            .read_with(cx, |session, _| session.threads().cloned().collect())
    }

    pub fn user_thread(&self, cx: &mut TestAppContext) -> ThreadId {
        self.session.update(cx, |session, cx| {
            session.create_thread(ThreadAnchor::line("src/a.ts", Side::New, 1), cx)
        })
    }

    pub fn post(&self, cx: &mut TestAppContext, id: ThreadId, text: &str) {
        self.session.update(cx, |session, cx| session.post(id, text, cx));
    }
}

/// Hosts `build`'s view in a window under `Root`, as inputs and popovers need.
pub fn root_window<V: Render + 'static>(
    cx: &mut TestAppContext,
    build: impl FnOnce(&mut Window, &mut Context<V>) -> V,
) -> (Entity<V>, &mut VisualTestContext) {
    cx.update(|cx| {
        diffpilot_ui_kit::init(cx);
        diffpilot::review::init(cx);
    });
    let mut view = None;
    let handle = cx.add_window(|window, cx| {
        let entity = cx.new(|cx| build(window, cx));
        view = Some(entity.clone());
        Root::new(entity, window, cx)
    });
    let vcx = VisualTestContext::from_window(handle.into(), cx).into_mut();
    vcx.run_until_parked();
    (view.expect("the window builds its view"), vcx)
}
