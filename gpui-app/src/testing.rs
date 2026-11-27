use crate::repo_view::RepoView;
use crate::runs::Runs;
use crate::settings::{self, Section, Services};
use crate::workspace::{self, Workspace};
use diffpilot_core::diff::FileDiff;
use diffpilot_core::github::auth::{Credentials, GhCli, MemoryStore};
use diffpilot_core::github::http::{Http, HttpError, Method, Request, Response};
use diffpilot_core::guide::stale::{self, file_fingerprints};
use diffpilot_core::guide::{Attention, Chapter, ChapterFile, Guide, Note, Overview, Span, fingerprint};
use diffpilot_core::mcp::McpBridge;
use diffpilot_core::review::{Finding, ReviewResult, Severity};
use diffpilot_core::runner::{EngineConfig, McpLaunch, RunEngine};
use diffpilot_core::settings::Settings;
use diffpilot_core::store::Store;
use diffpilot_core::threads::Side;
use diffpilot_core::viewed::ViewedStore;
use gpui::{
    AppContext as _, Bounds, Entity, Global, Modifiers, MouseButton, Pixels, TestAppContext, VisualTestContext,
};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

pub fn canned_review() -> ReviewResult {
    let finding = |path: &str, line, severity, title: &str| Finding {
        path: path.into(),
        line,
        severity,
        title: title.into(),
        body: format!("{title} body"),
    };
    ReviewResult {
        summary: "Fine overall.".into(),
        findings: vec![
            finding("base.txt", Some(2), Severity::MustFix, "Edited"),
            finding("src/staged.rs", None, Severity::Nit, "Name"),
            finding("docs/committed.md", None, Severity::Suggestion, "Doc"),
            finding("src/staged.rs", Some(5), Severity::Question, "Why"),
        ],
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

pub fn write_png(dir: &Path, path: &str, width: u32, height: u32, color: [u8; 4]) {
    let file = dir.join(path);
    std::fs::create_dir_all(file.parent().expect("parent")).expect("mkdir");
    image::RgbaImage::from_pixel(width, height, image::Rgba(color))
        .save(file)
        .expect("write png");
}

pub fn preview_fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo = dir.path();
    git(repo, &["init", "-q", "-b", "main"]);
    write_png(repo, "assets/logo.png", 16, 12, [200, 40, 40, 255]);
    git(repo, &["add", "."]);
    git(repo, &["commit", "-q", "-m", "base"]);
    git(repo, &["checkout", "-q", "-b", "feature"]);
    write_png(repo, "assets/logo.png", 20, 10, [40, 40, 200, 255]);
    write_png(repo, "assets/icon.png", 8, 8, [40, 160, 40, 255]);
    write(repo, "notes.md", "# Notes\n\nSome **bold** words.\n\n- one\n- two\n");
    git(repo, &["add", "."]);
    git(repo, &["commit", "-q", "-m", "feature"]);
    dir
}

pub fn open_preview(cx: &mut TestAppContext) -> (Opened, &'static mut VisualTestContext) {
    let dir = preview_fixture();
    let settings = Arc::new(Settings::in_memory());
    let store = Arc::new(ViewedStore::in_memory());
    let (view, vcx) = open_repo(cx, dir.path(), settings.clone(), store.clone());
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
    let (view, vcx) = open_repo(cx, dir.path(), settings.clone(), store.clone());
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
) -> (Entity<RepoView>, &'static mut VisualTestContext) {
    let review_store = engine_store(cx);
    open_repo_in(cx, repo, settings, store, review_store)
}

/// Runs and threads share one store, as in the app.
fn engine_store(cx: &mut TestAppContext) -> Arc<Store> {
    cx.update(|cx| cx.try_global::<Runs>().map(|runs| runs.engine().store().clone()))
        .unwrap_or_else(|| Arc::new(Store::open_in_memory().expect("an in-memory store opens")))
}

pub fn open_repo_in(
    cx: &mut TestAppContext,
    repo: &Path,
    settings: Arc<Settings>,
    store: Arc<ViewedStore>,
    review_store: Arc<Store>,
) -> (Entity<RepoView>, &'static mut VisualTestContext) {
    ready(cx);
    let repo = repo.to_path_buf();
    let mut view = None;
    let handle = cx.add_window(|window, cx| {
        let entity = cx.new(|cx| RepoView::new(repo, settings, store, review_store, window, cx));
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

struct SignedIn;

impl GhCli for SignedIn {
    fn token(&self, _: &str) -> Option<String> {
        Some("test-token".into())
    }
}

struct Offline;

impl Http for Offline {
    fn send(&self, _: &Request) -> Result<Response, HttpError> {
        Err(HttpError("offline".into()))
    }
}

/// Opens a workspace window and presses ⌘, then clicks the section in the list. `folder` of `None` shows the empty state.
pub fn open_settings(
    cx: &mut TestAppContext,
    folder: Option<&Path>,
    settings: Arc<Settings>,
    section: Section,
) -> &'static mut VisualTestContext {
    install_settings(cx, settings.clone());
    let store = engine_store(cx);
    let vcx = open_window(cx, folder, settings, store);
    show_settings(vcx, section);
    vcx
}

/// What ⌘, needs. The Data section lists the store that runs and threads use.
pub fn install_settings(cx: &mut TestAppContext, settings: Arc<Settings>) {
    ready(cx);
    let services = Services {
        credentials: Credentials::new(Arc::new(MemoryStore::default()), Arc::new(NoGh)),
        http: Arc::new(Offline),
        host: "github.com".into(),
        mcp_server: None,
        store: engine_store(cx),
    };
    cx.update(|cx| settings::init(settings, services, cx));
}

pub fn show_settings(cx: &mut VisualTestContext, section: Section) {
    press(cx, "cmd-,");
    click(cx, &format!("toc:{}", section.title()));
}

pub fn open_workspace(
    cx: &mut TestAppContext,
    folder: &Path,
    settings: Arc<Settings>,
) -> &'static mut VisualTestContext {
    ready(cx);
    let store = engine_store(cx);
    open_window(cx, Some(folder), settings, store)
}

fn open_window(
    cx: &mut TestAppContext,
    folder: Option<&Path>,
    settings: Arc<Settings>,
    review_store: Arc<Store>,
) -> &'static mut VisualTestContext {
    let folder = folder.map(Path::to_path_buf);
    let handle = cx.add_window(|window, cx| {
        let workspace = cx.new(|cx| {
            Workspace::new(
                settings,
                Arc::new(ViewedStore::in_memory()),
                review_store,
                folder,
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

/// Bounds of a tag once it has stopped moving, so a click lands after a menu or drawer finishes opening.
fn settled_bounds(cx: &mut VisualTestContext, selector: &str) -> Bounds<Pixels> {
    let mut bounds = bounds_of(cx, selector).unwrap_or_else(|| panic!("nothing tagged `{selector}` is on screen"));
    for _ in 0..60 {
        std::thread::sleep(std::time::Duration::from_millis(16));
        let Some(next) = bounds_of(cx, selector) else { break };
        if next == bounds {
            break;
        }
        bounds = next;
    }
    bounds
}

pub fn bounds(cx: &mut VisualTestContext, selector: &str) -> Bounds<Pixels> {
    bounds_of(cx, selector).unwrap_or_else(|| panic!("nothing tagged `{selector}` is on screen"))
}

pub fn click(cx: &mut VisualTestContext, selector: &str) {
    let bounds = settled_bounds(cx, selector);
    cx.simulate_click(bounds.center(), Modifiers::default());
    draw(cx);
}

pub fn click_corner(cx: &mut VisualTestContext, selector: &str) {
    let bounds = settled_bounds(cx, selector);
    cx.simulate_click(
        bounds.origin + gpui::point(gpui::px(4.), gpui::px(4.)),
        Modifiers::default(),
    );
    draw(cx);
}

pub fn hover(cx: &mut VisualTestContext, selector: &str) {
    let bounds = settled_bounds(cx, selector);
    cx.simulate_mouse_move(bounds.center(), None, Modifiers::default());
    draw(cx);
}

fn below(bounds: Bounds<Pixels>, (dx, dy): (f32, f32)) -> gpui::Point<Pixels> {
    gpui::point(bounds.left() + gpui::px(dx), bounds.bottom() + gpui::px(dy))
}

/// Clicks `count` times at `(dx, dy)` from the left edge and bottom of the tagged element.
pub fn click_below(cx: &mut VisualTestContext, selector: &str, at: (f32, f32), count: usize) {
    let bounds = bounds_of(cx, selector).unwrap_or_else(|| panic!("nothing tagged `{selector}` is on screen"));
    let position = below(bounds, at);
    cx.simulate_mouse_move(position, None, Modifiers::default());
    draw(cx);
    for click_count in 1..=count {
        cx.simulate_event(gpui::MouseDownEvent {
            position,
            modifiers: Modifiers::default(),
            button: MouseButton::Left,
            click_count,
            first_mouse: false,
        });
        cx.simulate_event(gpui::MouseUpEvent {
            position,
            modifiers: Modifiers::default(),
            button: MouseButton::Left,
            click_count,
        });
    }
    draw(cx);
}

/// Presses at `from` and releases at `to`, both measured from the left edge and bottom of the tagged element.
pub fn drag_below(cx: &mut VisualTestContext, selector: &str, from: (f32, f32), to: (f32, f32)) {
    let bounds = bounds_of(cx, selector).unwrap_or_else(|| panic!("nothing tagged `{selector}` is on screen"));
    let (from, to) = (below(bounds, from), below(bounds, to));
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::default());
    draw(cx);
}

fn right_edge(bounds: Bounds<Pixels>, inset: f32, y: f32) -> gpui::Point<Pixels> {
    gpui::point(bounds.right() - gpui::px(inset), bounds.top() + gpui::px(y))
}

/// Presses `inset` left of the tagged element's right edge at `from` below its top, drags down to `to`, and releases.
pub fn drag_right_edge(cx: &mut VisualTestContext, selector: &str, inset: f32, from: f32, to: f32) {
    let bounds = bounds_of(cx, selector).unwrap_or_else(|| panic!("nothing tagged `{selector}` is on screen"));
    cx.simulate_mouse_move(right_edge(bounds, inset, from), None, Modifiers::default());
    draw(cx);
    cx.simulate_mouse_down(right_edge(bounds, inset, from), MouseButton::Left, Modifiers::default());
    draw(cx);
    for step in 1..=4 {
        let y = from + (to - from) * step as f32 / 4.0;
        cx.simulate_mouse_move(right_edge(bounds, inset, y), MouseButton::Left, Modifiers::default());
        draw(cx);
    }
    cx.simulate_mouse_up(right_edge(bounds, inset, to), MouseButton::Left, Modifiers::default());
    draw(cx);
}

pub fn git_output(dir: &Path, args: &[&str]) -> String {
    let binary = diffpilot_core::git::binary().expect("git");
    let output = Command::new(binary)
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("run git");
    String::from_utf8_lossy(&output.stdout).into_owned()
}

pub fn wait_until(cx: &mut VisualTestContext, what: &str, mut done: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !done() {
        assert!(std::time::Instant::now() < deadline, "{what} did not happen");
        std::thread::sleep(std::time::Duration::from_millis(20));
        draw(cx);
    }
}

pub fn center_x(cx: &mut VisualTestContext, selector: &str) -> f32 {
    let bounds = bounds_of(cx, selector).unwrap_or_else(|| panic!("nothing tagged `{selector}` is on screen"));
    f32::from(bounds.center().x)
}

pub fn drag_by(cx: &mut VisualTestContext, selector: &str, dx: f32, dy: f32) {
    let bounds = bounds_of(cx, selector).unwrap_or_else(|| panic!("nothing tagged `{selector}` is on screen"));
    let start = bounds.center();
    let at = |step: f32| start + gpui::point(gpui::px(dx * step), gpui::px(dy * step));
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    for step in 1..=4 {
        cx.simulate_mouse_move(at(step as f32 / 4.0), MouseButton::Left, Modifiers::default());
    }
    cx.simulate_mouse_up(at(1.0), MouseButton::Left, Modifiers::default());
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

pub fn clipboard(cx: &mut VisualTestContext) -> Option<String> {
    cx.update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text()))
}

pub fn wait_for_run(cx: &mut VisualTestContext, session: &Entity<crate::review::ReviewSession>) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !session.read_with(cx, |session, cx| {
        let activity = session.activity().read(cx);
        activity.current().is_some_and(|run| run.state.is_finished()) && !activity.is_busy()
    }) {
        assert!(std::time::Instant::now() < deadline, "the run did not end");
        std::thread::sleep(std::time::Duration::from_millis(20));
        draw(cx);
    }
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

/// A `claude` script that plays a plan, a few tool calls and the run's end.
pub struct FakeClaude {
    work: tempfile::TempDir,
    engine: RunEngine,
    _bridge: Arc<McpBridge>,
}

impl FakeClaude {
    pub fn launches(&self) -> usize {
        std::fs::read_to_string(self.work.path().join("launches")).map_or(0, |text| text.lines().count())
    }

    pub fn engine(&self) -> RunEngine {
        self.engine.clone()
    }

    pub fn store(&self) -> Arc<Store> {
        self.engine.store().clone()
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

    pub fn hold(&self) {
        let _ = std::fs::remove_file(self.work.path().join("go"));
    }

    pub fn finish_after(&self, seconds: f64) {
        std::fs::write(self.work.path().join("auto"), seconds.to_string()).expect("write auto");
    }

    /// Applies to runs that start after this call.
    pub fn review_with(&self, result: &ReviewResult) {
        let comment = |(index, finding): (usize, &Finding)| {
            let line = finding.line.unwrap_or(1);
            tool_use(
                &format!("c{index}"),
                "mcp__diffpilot__add_comment",
                &serde_json::json!({ "file": finding.path, "startLine": line }),
            )
        };
        let before: Vec<String> = result.findings.iter().enumerate().map(comment).collect();
        let after = serde_json::json!({
            "type": "result", "subtype": "success", "is_error": false, "num_turns": 4, "result": "Done.",
            "structured_output": result,
        });
        std::fs::write(self.work.path().join("before"), before.join("\n")).expect("write before");
        std::fs::write(self.work.path().join("after"), after.to_string()).expect("write after");
    }

    pub fn end_without_review(&self) {
        let _ = std::fs::remove_file(self.work.path().join("before"));
        let after = serde_json::json!({
            "type": "result", "subtype": "success", "is_error": false, "num_turns": 1, "result": "Done.",
        });
        std::fs::write(self.work.path().join("after"), after.to_string()).expect("write after");
    }

    pub fn reply_with(&self, text: &str) {
        let after = serde_json::json!({
            "type": "result", "subtype": "success", "is_error": false, "num_turns": 1, "result": text,
        });
        std::fs::write(self.work.path().join("after"), after.to_string()).expect("write after");
    }

    pub fn fail_with(&self, message: &str) {
        let after = serde_json::json!({
            "type": "result", "subtype": "error_during_execution", "is_error": true, "result": message,
        });
        std::fs::write(self.work.path().join("after"), after.to_string()).expect("write after");
    }

    /// Calls a tool as the newest launch. A launch is counted only after it copied its MCP config, so after
    /// `wait_for_launches` the token here is the new run's, not the one before it.
    pub fn call_mcp_tool(&self, tool: &str, args: serde_json::Value) -> serde_json::Value {
        let file = self.work.path().join("mcp.json");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while self.launches() == 0 {
            assert!(std::time::Instant::now() < deadline, "no run got an MCP config");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let config: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(file).expect("a run is going")).expect("mcp.json is JSON");
        let args_list = config["mcpServers"]["diffpilot"]["args"].as_array().expect("args");
        let at = args_list.iter().position(|arg| arg == "--token").expect("a token");
        let token = args_list[at + 1].as_str().expect("token text");
        let line = serde_json::json!({ "token": token, "tool": tool, "args": args });
        self._bridge.handle_line(&line.to_string())
    }

    /// The session ids that launches passed with `--resume`, in order.
    pub fn resumed(&self) -> Vec<String> {
        std::fs::read_to_string(self.work.path().join("resumed"))
            .map(|text| text.lines().map(String::from).collect())
            .unwrap_or_default()
    }

    pub fn prompt(&self) -> String {
        std::fs::read_to_string(self.work.path().join("prompt")).unwrap_or_default()
    }

    pub fn model(&self) -> String {
        std::fs::read_to_string(self.work.path().join("model"))
            .unwrap_or_default()
            .trim()
            .to_string()
    }
}

fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("write script");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    path
}

fn tool_use(id: &str, name: &str, input: &serde_json::Value) -> String {
    serde_json::json!({
        "type": "assistant",
        "message": { "id": format!("m-{id}"), "role": "assistant",
            "content": [{ "type": "tool_use", "id": id, "name": name, "input": input }] },
    })
    .to_string()
}

fn tool_result(id: &str, text: &str) -> String {
    serde_json::json!({
        "type": "user",
        "message": { "role": "user", "content": [{ "type": "tool_result", "tool_use_id": id, "content": text }] },
    })
    .to_string()
}

fn play() -> String {
    let todo = |content: &str, active: &str, status: &str| serde_json::json!({ "content": content, "activeForm": active, "status": status });
    let plan = serde_json::json!({ "todos": [
        todo("Read the diff", "Reading the diff", "completed"),
        todo("Check the callers", "Checking the callers", "in_progress"),
        todo("Write the review", "Writing the review", "pending"),
    ]});
    let init = r#"{"type":"system","subtype":"init","session_id":"s-1","model":"m","tools":[],"mcp_servers":[]}"#;
    [
        init.to_string(),
        tool_use("t1", "TodoWrite", &plan),
        tool_use("t2", "Read", &serde_json::json!({ "file_path": "src/staged.rs" })),
        tool_result("t2", "let value = 1;"),
        tool_use("t3", "Grep", &serde_json::json!({ "pattern": "value" })),
        tool_result("t3", "src/staged.rs"),
        tool_use("t4", "Bash", &serde_json::json!({ "command": "git diff --stat" })),
        tool_result("t4", "1 file changed"),
    ]
    .join("\n")
}

pub fn fake_engine() -> FakeClaude {
    let work = tempfile::tempdir().expect("tempdir");
    let dir = work.path();
    std::fs::write(dir.join("play"), play() + "\n").expect("write play");
    let claude = script(
        dir,
        "claude",
        &format!(
            r#"printf '%s' "$2" > '{dir}/prompt'
while [ $# -gt 0 ]; do
  case "$1" in
    --model) echo "$2" > '{dir}/model' ;;
    --mcp-config) cp "$2" '{dir}/mcp.json' ;;
    --resume) echo "$2" >> '{dir}/resumed' ;;
  esac
  shift
done
echo $$ >> '{dir}/launches'
cat '{dir}/play'
[ -f '{dir}/before' ] && cat '{dir}/before'
echo
if [ -f '{dir}/auto' ]; then sleep "$(cat '{dir}/auto')"; else while [ ! -f '{dir}/go' ]; do sleep 0.05; done; fi
if [ -f '{dir}/after' ]; then cat '{dir}/after'; else echo '{{"type":"result","subtype":"success","is_error":false,"num_turns":1,"result":"Done."}}'; fi"#,
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
    FakeClaude {
        work,
        engine: RunEngine::new(config),
        _bridge: bridge,
    }
}

pub fn fake_claude(cx: &mut TestAppContext) -> FakeClaude {
    let fake = fake_engine();
    let runs = Runs::with_engine(fake.engine());
    cx.update(|cx| cx.set_global(runs));
    fake
}

const GITHUB_URL: &str = "github.com:o/r.git";

/// A repository on `feature` with an `origin` that looks like GitHub (`github.com:o/r.git`). A script stands in
/// for ssh and runs git against a bare repository on disk, so fetch, pull and push never touch the network.
pub struct RemoteFixture {
    pub dir: tempfile::TempDir,
    pub repo: PathBuf,
    pub bare: PathBuf,
}

pub fn remote_fixture() -> RemoteFixture {
    let dir = tempfile::tempdir().expect("tempdir");
    let server = dir.path().join("server");
    let (bare, repo) = (server.join("o/r.git"), dir.path().join("repo"));
    for path in [&bare, &repo] {
        std::fs::create_dir_all(path).expect("mkdir");
    }
    let ssh = script(
        dir.path(),
        "fakessh",
        &format!(
            "for last; do :; done\ncmd=${{last#git-}}\ncd '{}' && eval \"git $cmd\"",
            server.display()
        ),
    );
    git(&bare, &["init", "-q", "--bare", "-b", "main"]);
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "core.sshCommand", &ssh.display().to_string()]);
    write(&repo, "base.txt", "base\n");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "base"]);
    git(&repo, &["remote", "add", "origin", GITHUB_URL]);
    git(&repo, &["push", "-q", "-u", "origin", "main"]);
    git(&repo, &["checkout", "-q", "-b", "feature"]);
    write(&repo, "docs/committed.md", "committed\n");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "committed"]);
    git(&repo, &["push", "-q", "-u", "origin", "feature"]);
    RemoteFixture { dir, repo, bare }
}

impl RemoteFixture {
    pub fn commit_here(&self, name: &str) {
        write(&self.repo, name, "local\n");
        git(&self.repo, &["add", "."]);
        git(&self.repo, &["commit", "-q", "-m", name]);
    }

    /// Another clone pushes a commit to `feature`, then this repository fetches it, so it is behind.
    pub fn commit_on_remote(&self, name: &str) {
        let other = self.dir.path().join("other");
        if !other.exists() {
            git(
                self.dir.path(),
                &[
                    "clone",
                    "-q",
                    "-b",
                    "feature",
                    &self.bare.display().to_string(),
                    "other",
                ],
            );
        }
        write(&other, name, "remote\n");
        git(&other, &["add", "."]);
        git(&other, &["commit", "-q", "-m", name]);
        git(&other, &["push", "-q", "origin", "feature"]);
        git(&self.repo, &["fetch", "-q", "origin"]);
    }

    /// Makes `refs/pull/<number>/head` on the remote point at a commit that adds `file` to `main`.
    pub fn add_pull_request(&self, number: u64, file: &str) {
        git(&self.repo, &["checkout", "-q", "-b", "pr-scratch", "main"]);
        write(&self.repo, file, "from the pull request\n");
        git(&self.repo, &["add", "."]);
        git(&self.repo, &["commit", "-q", "-m", file]);
        git(
            &self.repo,
            &["push", "-q", "origin", &format!("HEAD:refs/pull/{number}/head")],
        );
        git(&self.repo, &["checkout", "-q", "feature"]);
        git(&self.repo, &["branch", "-q", "-D", "pr-scratch"]);
    }
}

const OPEN_PRS: &str = r#"{"data":{"repository":{"pullRequests":{"nodes":[
 {"id":"PR_479","number":479,"title":"Bulk select in the inbox","url":"https://github.com/o/r/pull/479","state":"OPEN",
  "isDraft":false,"author":{"login":"jordan-lee"},"baseRefName":"main","headRefName":"feature/bulk-select",
  "headRefOid":"1111111111111111111111111111111111111111","reviewDecision":"REVIEW_REQUIRED","body":"",
  "headRepository":{"nameWithOwner":"o/r"},"commits":{"nodes":[{"commit":{"statusCheckRollup":{"state":"SUCCESS"}}}]}},
 {"id":"PR_482","number":482,"title":"Undo keeps a timer per message","url":"https://github.com/o/r/pull/482","state":"OPEN",
  "isDraft":false,"author":{"login":"mira"},"baseRefName":"main","headRefName":"feature",
  "headRefOid":"3f2a91c0d4e5b6a7988776655443322110ffeedd","reviewDecision":"CHANGES_REQUESTED","body":"",
  "headRepository":{"nameWithOwner":"o/r"},"commits":{"nodes":[{"commit":{"statusCheckRollup":{"state":"PENDING"}}}]}}
]}}}}"#;

/// Answers GitHub's GraphQL and REST calls with canned JSON: two open pull requests (#482 is the head of
/// `feature`) and the check runs and statuses from core's fixtures.
pub struct FakeGithub {
    seen: Mutex<Vec<Request>>,
}

impl FakeGithub {
    pub fn new() -> Arc<Self> {
        Arc::new(Self { seen: Mutex::default() })
    }

    pub fn posts(&self) -> Vec<String> {
        let seen = self.seen.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        seen.iter()
            .filter(|request| request.method == Method::Post && !request.url.ends_with("/graphql"))
            .map(|request| request.url.clone())
            .collect()
    }
}

impl Http for FakeGithub {
    fn send(&self, request: &Request) -> Result<Response, HttpError> {
        self.seen
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(request.clone());
        let body = request.body.as_deref().unwrap_or_default();
        let reply = |status, body: &str| {
            Ok(Response {
                status,
                body: body.to_string(),
            })
        };
        if body.contains("pullRequests(states:OPEN") {
            reply(200, OPEN_PRS)
        } else if request.url.contains("/check-runs") {
            reply(200, include_str!("../../core/tests/fixtures/github/check_runs.json"))
        } else if request.url.contains("/status") {
            reply(200, include_str!("../../core/tests/fixtures/github/commit_status.json"))
        } else if request.method == Method::Post {
            reply(201, "{}")
        } else {
            reply(404, "{}")
        }
    }
}

pub fn install_github(cx: &mut TestAppContext, http: Arc<dyn Http>) {
    ready(cx);
    let services = Services {
        credentials: Credentials::new(Arc::new(MemoryStore::default()), Arc::new(SignedIn)),
        http,
        host: "github.com".into(),
        mcp_server: None,
        store: Arc::new(Store::open_in_memory().expect("an in-memory store opens")),
    };
    cx.update(|cx| settings::init(Arc::new(Settings::in_memory()), services, cx));
}

fn demo_chapter(title: &str, phase: &str, attention: Attention, files: &[&str], notes: Vec<Note>) -> Chapter {
    Chapter {
        title: title.into(),
        summary: format!(
            "{title}: a long summary that has to wrap in a narrow column, with `popover()` and `paint_path` in code."
        ),
        phase: Some(phase.into()),
        attention,
        points: vec![
            "First point: it is long enough that it has to wrap in the narrow inspector, and it names `popover()`."
                .into(),
        ],
        focus: vec![
            "Check the loop: this check is long enough that it has to wrap inside the narrow inspector.".into(),
        ],
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
            before: Some(
                "Nothing was staged, so every run of the tool had to read the files from disk and compare them with `HEAD` again."
                    .into(),
            ),
            after: Some(
                "A staged file exists, so the tool reads it once and keeps the result, using `paint_path` only for the diagram."
                    .into(),
            ),
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
                "The small edits, changelog, xtask features and screenshots",
                "Skim",
                Attention::Low,
                &["base.txt", "docs/committed.md"],
                vec![],
            ),
        ],
        fingerprint: fingerprint(files),
        file_fingerprints: file_fingerprints(files),
        head: stale::head(repo),
        context: vec![],
    }
}
