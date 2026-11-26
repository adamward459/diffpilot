mod common;

use common::*;
use diffpilot::review::pill::RESULT_SECONDS;
use diffpilot::review::session::{FIX_NEEDS_DONT_ASK, FIX_NEEDS_WORKING_TREE, NOTHING_TO_FIX_MESSAGE};
use diffpilot::review::{ReviewEvent, ReviewHost, ReviewPill, ReviewSession};
use diffpilot::runs::Runs;
use diffpilot_core::comparison::Comparison;
use diffpilot_core::mcp::McpBridge;
use diffpilot_core::policy::EditPermission;
use diffpilot_core::run::{Progress, RunState};
use diffpilot_core::runner::{EngineConfig, McpLaunch, RunEngine};
use diffpilot_core::settings::Settings;
use diffpilot_core::store::Store;
use diffpilot_core::threads::{CLAUDE_NAME, Side, ThreadAnchor, ThreadId};
use gpui::{AppContext, Entity, TestAppContext};
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

struct World {
    _repo: tempfile::TempDir,
    work: tempfile::TempDir,
    bridge: Arc<McpBridge>,
    session: Entity<ReviewSession>,
    settings: Arc<Settings>,
    events: Arc<std::sync::Mutex<Vec<ReviewEvent>>>,
}

fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// A `claude` that keeps its MCP config, says it started, and ends only after the test creates `out/go`.
fn gated_claude(work: &Path) -> PathBuf {
    let out = work.join("out");
    std::fs::create_dir_all(&out).unwrap();
    script(
        work,
        "claude",
        &format!(
            r#"
CFG=''
while [ $# -gt 0 ]; do
  if [ "$1" = "--mcp-config" ]; then CFG="$2"; fi
  shift
done
cp "$CFG" '{out}/mcp.json'
echo '{{"type":"system","subtype":"init","session_id":"s-1","model":"m","tools":[],"mcp_servers":[]}}'
touch '{out}/started'
while [ ! -f '{out}/go' ]; do sleep 0.05; done
echo '{{"type":"result","subtype":"success","is_error":false,"num_turns":1,"result":"Done."}}'
"#,
            out = out.display()
        ),
    )
}

fn world(cx: &mut TestAppContext, comparison: Comparison) -> World {
    let repo = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let guard = script(
        work.path(),
        "guard",
        "cat > /dev/null\necho '[diffpilot guard] blocked' >&2\nexit 2",
    );
    let store = Arc::new(Store::open_in_memory().unwrap());
    let bridge = McpBridge::start(work.path().join("mcp.sock"), store.clone()).unwrap();
    let mut config = EngineConfig::new(store.clone());
    config.claude_binary = Some(gated_claude(work.path()));
    config.guard_binary = guard;
    config.run_dir = work.path().join("runs");
    config.keep_snapshots = 0;
    config.mcp = Some(McpLaunch::new(bridge.clone()));
    let engine = RunEngine::new(config);
    cx.update(|cx| cx.set_global(Runs::with_engine(engine)));

    let settings = Arc::new(Settings::in_memory());
    let mut host = ReviewHost::new(Some(repo.path().to_path_buf()), comparison);
    host.branch_name = Some("feature".into());
    host.files = Arc::from([Arc::new(file())]);
    let session = {
        let settings = settings.clone();
        cx.new(move |cx| ReviewSession::new(host, settings, store, Arc::new(StubReplier::default()), cx))
    };
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = events.clone();
    cx.update(|cx| {
        cx.subscribe(&session, move |_, event: &ReviewEvent, _| {
            sink.lock().unwrap().push(*event)
        })
        .detach();
    });
    World {
        _repo: repo,
        work,
        bridge,
        session,
        settings,
        events,
    }
}

fn working_tree() -> Comparison {
    Comparison::checked_out("main", true)
}

fn wait_until(cx: &mut TestAppContext, what: &str, mut done: impl FnMut(&mut TestAppContext) -> bool) {
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

impl World {
    fn out(&self, name: &str) -> PathBuf {
        self.work.path().join("out").join(name)
    }

    fn comment(&self, cx: &mut TestAppContext, text: &str) -> ThreadId {
        self.session.update(cx, |session, cx| {
            let id = session.create_thread(ThreadAnchor::line("src/a.ts", Side::New, 1), cx);
            session.post(id, text, cx);
            id
        })
    }

    fn state(&self, cx: &mut TestAppContext) -> Option<RunState> {
        self.session
            .read_with(cx, |session, _| session.fix_run().map(|run| run.state))
    }

    fn start_fix(&self, cx: &mut TestAppContext) {
        self.session.update(cx, |session, cx| session.fix_threads(&[], cx));
        wait_until(cx, "the run to start", |cx| {
            self.state(cx) == Some(RunState::Running) && self.out("started").exists()
        });
    }

    fn token(&self) -> String {
        let config: Value = serde_json::from_str(&std::fs::read_to_string(self.out("mcp.json")).unwrap()).unwrap();
        let args = config["mcpServers"]["diffpilot"]["args"].as_array().unwrap();
        let at = args.iter().position(|arg| arg == "--token").unwrap();
        args[at + 1].as_str().unwrap().to_string()
    }

    fn tool(&self, tool: &str, args: Value) -> Value {
        let line = json!({ "token": self.token(), "tool": tool, "args": args });
        self.bridge.handle_line(&line.to_string())
    }
}

#[gpui::test]
async fn a_fix_run_moves_through_its_states_and_the_threads_follow_it(cx: &mut TestAppContext) {
    let w = world(cx, working_tree());
    let id = w.comment(cx, "Rename this.");
    assert_eq!(w.session.read_with(cx, |s, _| s.fixable()), [id]);
    assert_eq!(w.session.read_with(cx, |s, cx| s.fix_blocker(cx)), None);

    w.start_fix(cx);
    assert!(w.session.read_with(cx, |s, _| s.fixing()));
    assert!(w.session.read_with(cx, |s, cx| s.fix_blocker(cx)).is_some());

    let reply = w.tool("reply", json!({ "threadId": id.0, "body": "Renaming it." }));
    assert_eq!(reply["ok"], true, "{reply}");
    let resolved = w.tool("resolve", json!({ "threadId": id.0, "summary": "Fixed: renamed it." }));
    assert_eq!(resolved["ok"], true, "{resolved}");

    wait_until(cx, "the thread to show Claude's answer", |cx| {
        w.session.read_with(cx, |session, _| {
            session.thread(id).is_some_and(|thread| thread.addressed.is_some())
                && session.fix_run().and_then(|run| run.progress()) == Some(Progress { done: 1, total: 1 })
        })
    });
    let thread = w.session.read_with(cx, |s, _| s.thread(id).cloned()).unwrap();
    let authors: Vec<_> = thread.comments.iter().map(|c| c.author.as_str()).collect();
    assert_eq!(authors, ["You", CLAUDE_NAME, CLAUDE_NAME]);
    assert_eq!(thread.addressed.unwrap().summary, "Fixed: renamed it.");
    assert!(w.session.read_with(cx, |s, _| s.fixable()).is_empty());

    std::fs::write(w.out("go"), "").unwrap();
    wait_until(cx, "the run to finish", |cx| w.state(cx) == Some(RunState::Done));
    assert!(!w.session.read_with(cx, |s, _| s.fixing()));
    assert_eq!(w.session.read_with(cx, |s, _| s.fix_outcome()), Some((1, 0)));
    assert!(
        w.events
            .lock()
            .unwrap()
            .contains(&ReviewEvent::RunFinished(RunState::Done))
    );
}

#[gpui::test]
async fn stopping_a_fix_run_ends_it_as_stopped(cx: &mut TestAppContext) {
    let w = world(cx, working_tree());
    w.comment(cx, "Rename this.");
    w.start_fix(cx);

    w.session.update(cx, |session, cx| session.stop_claude(cx));

    wait_until(cx, "the run to stop", |cx| w.state(cx) == Some(RunState::Stopped));
    assert!(!w.session.read_with(cx, |s, _| s.fixing()));
}

#[gpui::test]
async fn fix_says_why_it_cannot_start(cx: &mut TestAppContext) {
    let bare = session_on(cx, &Arc::new(Store::open_in_memory().unwrap()), working_tree());
    let reason = bare.read_with(cx, |s, cx| s.fix_blocker(cx));
    assert!(reason.is_some_and(|reason| reason.contains("not available")));

    let staged = world(cx, Comparison::Staged);
    assert_eq!(
        staged.session.read_with(cx, |s, cx| s.fix_blocker(cx)).as_deref(),
        Some(FIX_NEEDS_WORKING_TREE)
    );

    let asks = world(cx, working_tree());
    asks.settings.set_edit_permission(EditPermission::AskEach);
    assert_eq!(
        asks.session.read_with(cx, |s, cx| s.fix_blocker(cx)).as_deref(),
        Some(FIX_NEEDS_DONT_ASK)
    );
}

#[gpui::test]
async fn fix_with_no_open_comment_starts_no_run(cx: &mut TestAppContext) {
    let w = world(cx, working_tree());
    w.session.update(cx, |session, cx| session.fix_threads(&[], cx));
    assert_eq!(w.state(cx), None);
    assert_eq!(
        w.session
            .read_with(cx, |s, _| s.fix_failure().map(String::from))
            .as_deref(),
        Some(NOTHING_TO_FIX_MESSAGE)
    );
}

#[gpui::test]
async fn the_pill_follows_a_fix_run_and_shows_its_result_for_a_while(cx: &mut TestAppContext) {
    let w = world(cx, working_tree());
    w.comment(cx, "Rename this.");
    let session = w.session.clone();
    let (pill, cx) = root_window(cx, |window, cx| ReviewPill::new(session, window, cx));
    assert!(!pill.read_with(cx, |pill, _| pill.needs_room()));

    pill.update(cx, |pill, cx| pill.fix(cx));
    cx.executor().allow_parking();
    let deadline = Instant::now() + Duration::from_secs(30);
    let ready = || {
        let running = w
            .session
            .read_with(cx, |s, _| s.fix_run().map(|run| run.state) == Some(RunState::Running));
        running && w.out("started").exists()
    };
    while !ready() {
        assert!(Instant::now() < deadline, "the run did not start");
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(pill.read_with(cx, |pill, _| pill.needs_room()));

    std::fs::write(w.out("go"), "").unwrap();
    while w.session.read_with(cx, |s, _| s.fix_run().map(|run| run.state)) != Some(RunState::Done) {
        assert!(Instant::now() < deadline, "the run did not finish");
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(pill.read_with(cx, |pill, _| pill.shows_fix_result()));
    assert!(pill.read_with(cx, |pill, _| pill.needs_room()));

    cx.executor().advance_clock(Duration::from_secs(RESULT_SECONDS + 1));
    assert!(!pill.read_with(cx, |pill, _| pill.shows_fix_result()));
    assert!(!pill.read_with(cx, |pill, _| pill.needs_room()));
}
