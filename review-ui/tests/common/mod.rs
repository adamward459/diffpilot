#![allow(dead_code)]

use diffpilot_core::cancel::CancelToken;
use diffpilot_core::changes::FileStatus;
use diffpilot_core::claude::{Replier, ReviewError, Reviewer};
use diffpilot_core::comparison::Comparison;
use diffpilot_core::diff::{DiffLine, FileDiff, Hunk, LineKind};
use diffpilot_core::review::{Finding, ReviewRequest, ReviewResult, ReviewRun, Severity};
use diffpilot_core::settings::Settings;
use diffpilot_core::threads::{ReviewThread, Side, ThreadAnchor, ThreadId};
use diffpilot_review_ui::{ReviewEvent, ReviewHost, ReviewSession};
use gpui::{AppContext, Context, Entity, Render, TestAppContext, VisualTestContext, Window};
use gpui_component::Root;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

pub struct StubReviewer {
    outcome: Mutex<Result<ReviewResult, ReviewError>>,
    pub requests: Mutex<Vec<ReviewRequest>>,
    pub tokens: Mutex<Vec<CancelToken>>,
}

impl StubReviewer {
    pub fn new(outcome: Result<ReviewResult, ReviewError>) -> Arc<Self> {
        Arc::new(Self {
            outcome: Mutex::new(outcome),
            requests: Mutex::default(),
            tokens: Mutex::default(),
        })
    }

    pub fn will_return(&self, outcome: Result<ReviewResult, ReviewError>) {
        *self.outcome.lock().unwrap() = outcome;
    }

    pub fn calls(&self) -> usize {
        self.requests.lock().unwrap().len()
    }
}

impl Reviewer for StubReviewer {
    fn review(&self, request: &ReviewRequest, cancel: &CancelToken) -> Result<ReviewResult, ReviewError> {
        self.requests.lock().unwrap().push(request.clone());
        self.tokens.lock().unwrap().push(cancel.clone());
        self.outcome.lock().unwrap().clone()
    }
}

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
    pub reviewer: Arc<StubReviewer>,
    pub replier: Arc<StubReplier>,
    pub settings: Arc<Settings>,
    pub events: Rc<RefCell<Vec<ReviewEvent>>>,
}

pub fn fixture(cx: &mut TestAppContext, outcome: Result<ReviewResult, ReviewError>) -> Fixture {
    let reviewer = StubReviewer::new(outcome);
    let replier = Arc::new(StubReplier::default());
    let settings = Arc::new(Settings::in_memory());
    let mut host = ReviewHost::new(Some(PathBuf::from("/repo")), Comparison::checked_out("main", true));
    host.files = Arc::from([Arc::new(file())]);
    host.visible_path = Some("src/a.ts".into());
    let session = {
        let (reviewer, replier, settings) = (reviewer.clone(), replier.clone(), settings.clone());
        cx.new(move |_| ReviewSession::new(host, settings, reviewer, replier))
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
        reviewer,
        replier,
        settings,
        events,
    }
}

impl Fixture {
    pub fn run(&self, cx: &mut TestAppContext) -> ReviewRun {
        self.session.read_with(cx, |session, _| session.run().clone())
    }

    pub fn start(&self, cx: &mut TestAppContext) {
        self.session.update(cx, |session, cx| session.start_review(cx));
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
        gpui_component::init(cx);
        diffpilot_review_ui::init(cx);
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
