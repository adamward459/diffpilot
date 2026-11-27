#![allow(dead_code)]

use diffpilot::review::{ReviewEvent, ReviewHost, ReviewSession};
use diffpilot::testing::{FakeClaude, bounds, click, fake_claude, open_repo_in, press, type_text};
use diffpilot_core::changes::FileStatus;
use diffpilot_core::comparison::Comparison;
use diffpilot_core::diff::{DiffLine, FileDiff, Hunk, LineKind};
use diffpilot_core::review::{Finding, ReviewResult, Severity};
use diffpilot_core::run::{Run, RunState};
use diffpilot_core::settings::Settings;
use diffpilot_core::store::Store;
use diffpilot_core::threads::{ReviewThread, Side, ThreadAnchor, ThreadId};
use diffpilot_core::viewed::ViewedStore;
use gpui::{AppContext, Context, Entity, Render, TestAppContext, VisualTestContext, Window, px};
use gpui_component::Root;
use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

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
    let settings = Arc::new(Settings::in_memory());
    let store = claude.store();
    let mut host = ReviewHost::new(Some(repo.path().to_path_buf()), Comparison::checked_out("main", true));
    host.files = Arc::from([Arc::new(file())]);
    host.visible_path = Some("src/a.ts".into());
    let session = {
        let (settings, store) = (settings.clone(), store.clone());
        cx.new(move |cx| ReviewSession::new(host, settings, store, cx))
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
    cx.new(move |cx| ReviewSession::new(host, settings, store, cx))
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
        self.session
            .read_with(cx, |session, cx| session.activity().read(cx).current().cloned())
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
            self.state(cx).is_some_and(RunState::is_finished)
                && self.session.read_with(cx, |s, cx| !s.activity().read(cx).is_busy())
        });
    }

    pub fn wait_for_launches(&self, cx: &mut TestAppContext, count: usize) {
        wait_until(cx, "claude to start", |_| self.claude.launches() >= count);
    }

    pub fn problem(&self, cx: &mut TestAppContext) -> Option<String> {
        self.session.read_with(cx, |session, cx| session.review_problem(cx))
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

    pub fn replying(&self, cx: &mut TestAppContext, id: ThreadId) -> bool {
        self.session.read_with(cx, |session, cx| session.is_replying(id, cx))
    }

    pub fn wait_for_reply_end(&self, cx: &mut TestAppContext, id: ThreadId) {
        wait_until(cx, "the reply run to end", |cx| !self.replying(cx, id));
    }

    pub fn wait_for_comment(&self, cx: &mut TestAppContext, id: ThreadId, body: &str) {
        wait_until(cx, "the comment", |cx| {
            self.session.update(cx, |session, cx| session.refresh_threads(cx));
            self.thread(cx, id)
                .is_some_and(|thread| thread.comments.iter().any(|comment| comment.body == body))
        });
    }

    /// Answers through the MCP tool, as Claude does, once the reply run has started.
    pub fn claude_replies(&self, cx: &mut TestAppContext, id: ThreadId, body: &str) {
        self.claude_calls(
            cx,
            "reply",
            serde_json::json!({ "threadId": id.0.to_string(), "body": body }),
        );
        self.wait_for_comment(cx, id, body);
    }

    pub fn claude_calls(&self, cx: &mut TestAppContext, tool: &str, args: serde_json::Value) {
        let launched = self.claude.launches();
        self.wait_for_launches(cx, launched.max(1));
        let answer = self.claude.call_mcp_tool(tool, args);
        assert_eq!(answer["ok"], true, "{answer}");
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

pub fn open_on(app: &mut TestAppContext, repo: &Path, store: Arc<Store>) -> &'static mut VisualTestContext {
    let (_, cx) = open_repo_in(
        app,
        repo,
        Arc::new(Settings::in_memory()),
        Arc::new(ViewedStore::in_memory()),
        store,
    );
    cx
}

pub fn comment_on_file(cx: &mut VisualTestContext, path: &str, text: &str) {
    click(cx, &format!("sidebar:row:{path}"));
    click(cx, &format!("pane:comment-on-file:{path}"));
    type_text(cx, text);
    press(cx, "cmd-enter");
}

/// The card of the thread with this key must not run past the row the pane made for it.
pub fn assert_card_inside_its_row(cx: &mut VisualTestContext, key: &str) {
    let card = bounds(cx, &format!("thread:card:{key}"));
    let row = bounds(cx, &format!("pane:thread-row:{key}"));
    assert!(
        card.bottom() <= row.bottom() + px(1.),
        "the card `{key}` ends {:?} below its row",
        card.bottom() - row.bottom()
    );
}
