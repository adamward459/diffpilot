use crate::repo_view::{Claude, RepoView};
use crate::runs::Runs;
use crate::settings::{self, Services, Tab};
use crate::workspace::{self, Workspace};
use diffpilot_core::cancel::CancelToken;
use diffpilot_core::claude::{ReviewError, Reviewer, ScriptedReplier};
use diffpilot_core::diff::FileDiff;
use diffpilot_core::github::auth::{Credentials, GhCli, MemoryStore};
use diffpilot_core::github::http::{Http, HttpError, Request, Response};
use diffpilot_core::guide::stale::{self, file_fingerprints};
use diffpilot_core::guide::{Attention, Chapter, ChapterFile, Guide, Note, Overview, Span, fingerprint};
use diffpilot_core::mcp::McpBridge;
use diffpilot_core::review::{Finding, ReviewRequest, ReviewResult, Severity};
use diffpilot_core::runner::{EngineConfig, McpLaunch, RunEngine};
use diffpilot_core::settings::Settings;
use diffpilot_core::store::Store;
use diffpilot_core::threads::Side;
use diffpilot_core::viewed::ViewedStore;
use gpui::{AppContext as _, Bounds, Entity, Global, Modifiers, Pixels, TestAppContext, VisualTestContext};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

struct Canned;

impl Reviewer for Canned {
    fn review(&self, _: &ReviewRequest, _: &CancelToken) -> Result<ReviewResult, ReviewError> {
        let finding = |path: &str, line, severity, title: &str| Finding {
            path: path.into(),
            line,
            severity,
            title: title.into(),
            body: format!("{title} body"),
        };
        Ok(ReviewResult {
            summary: "Fine overall.".into(),
            findings: vec![
                finding("base.txt", Some(2), Severity::MustFix, "Edited"),
                finding("src/staged.rs", None, Severity::Nit, "Name"),
                finding("docs/committed.md", None, Severity::Suggestion, "Doc"),
                finding("src/staged.rs", Some(5), Severity::Question, "Why"),
            ],
        })
    }
}

pub fn canned_claude() -> Claude {
    Claude {
        reviewer: Arc::new(Canned),
        replier: Arc::new(ScriptedReplier::default()),
    }
}

pub fn git(dir: &Path, args: &[&str]) {
    let binary = diffpilot_core::git::binary().expect("git");
    let status = Command::new(binary)
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .status()
        .expect("run git");
    assert!(status.success(), "git {args:?}");
}

pub fn write(dir: &Path, path: &str, text: &str) {
    let file = dir.join(path);
    std::fs::create_dir_all(file.parent().expect("parent")).expect("mkdir");
    std::fs::write(file, text).expect("write");
}

pub fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    build_fixture(dir.path());
    dir
}

pub fn fixture_named(name: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo = dir.path().join(name);
    std::fs::create_dir(&repo).expect("mkdir");
    build_fixture(&repo);
    (dir, repo)
}

fn build_fixture(repo: &Path) {
    git(repo, &["init", "-q", "-b", "main"]);
    write(repo, "base.txt", "base\n");
    git(repo, &["add", "."]);
    git(repo, &["commit", "-q", "-m", "base"]);
    git(repo, &["checkout", "-q", "-b", "feature"]);
    write(repo, "docs/committed.md", "committed\n");
    git(repo, &["add", "."]);
    git(repo, &["commit", "-q", "-m", "committed"]);
    write(repo, "base.txt", "base\nedited\n");
    write(repo, "src/staged.rs", &"let value = 1;\n".repeat(300));
    git(repo, &["add", "src/staged.rs"]);
}

pub struct Opened {
    pub view: Entity<RepoView>,
    pub store: Arc<ViewedStore>,
    pub settings: Arc<Settings>,
    pub dir: tempfile::TempDir,
}

pub fn open(cx: &mut TestAppContext) -> (Opened, &'static mut VisualTestContext) {
    open_with(cx, Settings::in_memory())
}

pub fn open_with(cx: &mut TestAppContext, settings: Settings) -> (Opened, &'static mut VisualTestContext) {
    let dir = fixture();
    let settings = Arc::new(settings);
    let store = Arc::new(ViewedStore::in_memory());
    let (view, vcx) = open_repo(cx, dir.path(), settings.clone(), store.clone(), canned_claude());
    (
        Opened {
            view,
            store,
            settings,
            dir,
        },
        vcx,
    )
}

pub fn open_repo(
    cx: &mut TestAppContext,
    repo: &Path,
    settings: Arc<Settings>,
    store: Arc<ViewedStore>,
    claude: Claude,
) -> (Entity<RepoView>, &'static mut VisualTestContext) {
    let review_store = Arc::new(Store::open_in_memory().expect("an in-memory store opens"));
    open_repo_in(cx, repo, settings, store, review_store, claude)
}

pub fn open_repo_in(
    cx: &mut TestAppContext,
    repo: &Path,
    settings: Arc<Settings>,
    store: Arc<ViewedStore>,
    review_store: Arc<Store>,
    claude: Claude,
) -> (Entity<RepoView>, &'static mut VisualTestContext) {
    ready(cx);
    let repo = repo.to_path_buf();
    let mut view = None;
    let handle = cx.add_window(|window, cx| {
        let entity = cx.new(|cx| RepoView::new(repo, settings, store, review_store, claude, window, cx));
        view = Some(entity.clone());
        gpui_component::Root::new(entity, window, cx)
    });
    let vcx = VisualTestContext::from_window(handle.into(), cx).into_mut();
    let view = view.expect("the window builds the view");
    vcx.executor().allow_parking();
    vcx.run_until_parked();
    (view, vcx)
}

struct AppReady;

impl Global for AppReady {}

fn ready(cx: &mut TestAppContext) {
    cx.update(|cx| {
        if cx.has_global::<AppReady>() {
            return;
        }
        cx.set_global(AppReady);
        diffpilot_ui_kit::init(cx);
        workspace::init(cx);
    });
}

struct NoGh;

impl GhCli for NoGh {
    fn token(&self, _: &str) -> Option<String> {
        None
    }
}

struct Offline;

impl Http for Offline {
    fn send(&self, _: &Request) -> Result<Response, HttpError> {
        Err(HttpError("offline".into()))
    }
}

pub fn open_settings(cx: &mut TestAppContext, settings: Arc<Settings>, tab: Tab) -> &'static mut VisualTestContext {
    ready(cx);
    let services = Services {
        credentials: Credentials::new(Arc::new(MemoryStore::default()), Arc::new(NoGh)),
        http: Arc::new(Offline),
        host: "github.com".into(),
        mcp_server: None,
    };
    let window = cx.update(|cx| {
        settings::init(settings, services, cx);
        settings::open(Some(tab), cx).expect("the settings window opens")
    });
    let vcx = VisualTestContext::from_window(window.into(), cx).into_mut();
    draw(vcx);
    vcx
}

pub fn open_workspace(
    cx: &mut TestAppContext,
    folder: &Path,
    settings: Arc<Settings>,
) -> &'static mut VisualTestContext {
    ready(cx);
    let folder = folder.to_path_buf();
    let handle = cx.add_window(|window, cx| {
        let review_store = Arc::new(Store::open_in_memory().expect("an in-memory store opens"));
        let workspace = cx.new(|cx| {
            Workspace::new(
                settings,
                Arc::new(ViewedStore::in_memory()),
                review_store,
                Some(folder),
                false,
                window,
                cx,
            )
        });
        gpui_component::Root::new(workspace, window, cx)
    });
    let vcx = VisualTestContext::from_window(handle.into(), cx).into_mut();
    vcx.executor().allow_parking();
    draw(vcx);
    vcx
}

pub fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| {
        // refresh() skips gpui's cached views, so every tag is painted in every frame.
        window.refresh();
        window.draw(cx).clear();
    });
    cx.run_until_parked();
}

fn bounds_of(cx: &mut VisualTestContext, selector: &str) -> Option<Bounds<Pixels>> {
    draw(cx);
    cx.debug_bounds(Box::leak(selector.to_owned().into_boxed_str()))
}

// gpui never clears debug bounds, so a tag that rendered once stays true in that window.
// Assert on a state tag that is new to the window.
pub fn shows(cx: &mut VisualTestContext, selector: &str) -> bool {
    bounds_of(cx, selector).is_some()
}

pub fn click(cx: &mut VisualTestContext, selector: &str) {
    let bounds = bounds_of(cx, selector).unwrap_or_else(|| panic!("nothing tagged `{selector}` is on screen"));
    cx.simulate_click(bounds.center(), Modifiers::default());
    draw(cx);
}

pub fn press(cx: &mut VisualTestContext, keys: &str) {
    cx.simulate_keystrokes(keys);
    draw(cx);
}

pub fn type_text(cx: &mut VisualTestContext, text: &str) {
    cx.simulate_input(text);
    draw(cx);
}

pub fn wait_until_shows(cx: &mut VisualTestContext, selector: &str) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !shows(cx, selector) {
        assert!(
            std::time::Instant::now() < deadline,
            "`{selector}` did not appear on screen"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// A `claude` script that starts, then ends only after `finish`. Each start adds a line to `launches`.
pub struct FakeClaude {
    work: tempfile::TempDir,
    _bridge: Arc<McpBridge>,
}

impl FakeClaude {
    fn launches(&self) -> usize {
        std::fs::read_to_string(self.work.path().join("launches")).map_or(0, |text| text.lines().count())
    }

    pub fn wait_for_launches(&self, cx: &mut VisualTestContext, count: usize) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while self.launches() < count {
            assert!(
                std::time::Instant::now() < deadline,
                "claude started fewer than {count} times"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
            draw(cx);
        }
    }

    pub fn finish(&self) {
        std::fs::write(self.work.path().join("go"), "").expect("write go");
    }
}

fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("write script");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    path
}

pub fn fake_claude(cx: &mut TestAppContext) -> FakeClaude {
    let work = tempfile::tempdir().expect("tempdir");
    let dir = work.path();
    let claude = script(
        dir,
        "claude",
        &format!(
            r#"echo $$ >> '{dir}/launches'
echo '{{"type":"system","subtype":"init","session_id":"s-1","model":"m","tools":[],"mcp_servers":[]}}'
while [ ! -f '{dir}/go' ]; do sleep 0.05; done
echo '{{"type":"result","subtype":"success","is_error":false,"num_turns":1,"result":"Done."}}'"#,
            dir = dir.display()
        ),
    );
    let guard = script(
        dir,
        "guard",
        "cat > /dev/null\necho '[diffpilot guard] blocked' >&2\nexit 2",
    );
    let store = Arc::new(Store::open_in_memory().expect("an in-memory store opens"));
    let bridge = McpBridge::start(dir.join("mcp.sock"), store.clone()).expect("the MCP bridge starts");
    let mut config = EngineConfig::new(store);
    config.claude_binary = Some(claude);
    config.guard_binary = guard;
    config.run_dir = dir.join("runs");
    config.keep_snapshots = 0;
    config.mcp = Some(McpLaunch::new(bridge.clone()));
    cx.update(|cx| cx.set_global(Runs::with_engine(RunEngine::new(config))));
    FakeClaude { work, _bridge: bridge }
}

fn demo_chapter(title: &str, phase: &str, attention: Attention, files: &[&str], notes: Vec<Note>) -> Chapter {
    Chapter {
        title: title.into(),
        summary: format!("{title} summary with `code`."),
        phase: Some(phase.into()),
        attention,
        points: vec!["First point.".into()],
        focus: vec!["Check the loop.".into()],
        files: files
            .iter()
            .map(|path| ChapterFile {
                path: (*path).into(),
                spans: vec![],
            })
            .collect(),
        notes,
        diagrams: vec![],
        impact: vec![],
    }
}

fn demo_note(line: u32, critical: bool, text: &str) -> Note {
    Note {
        path: "src/staged.rs".into(),
        span: Some(Span {
            side: Side::New,
            line,
            end_line: None,
            anchor_text: Some("let value = 1;".into()),
        }),
        text: text.into(),
        critical,
    }
}

/// A two-chapter guide over the files of `fixture()`: the staged file with two notes, then the small edits.
pub fn demo_guide(repo: &Path, files: &[FileDiff]) -> Guide {
    Guide {
        overview: Overview {
            title: "What this change does".into(),
            summary: "It adds a staged file and edits the base.".into(),
            before: Some("Nothing was staged.".into()),
            after: Some("A staged file exists.".into()),
        },
        diagrams: vec![],
        chapters: vec![
            demo_chapter(
                "The staged file",
                "Core",
                Attention::High,
                &["src/staged.rs"],
                vec![demo_note(5, false, "Plain note."), demo_note(7, true, "Risky spot.")],
            ),
            demo_chapter(
                "The small edits",
                "Skim",
                Attention::Low,
                &["base.txt", "docs/committed.md"],
                vec![],
            ),
        ],
        fingerprint: fingerprint(files),
        file_fingerprints: file_fingerprints(files),
        head: stale::head(repo),
    }
}
