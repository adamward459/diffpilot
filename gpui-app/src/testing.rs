use crate::repo_view::{Claude, RepoView};
use crate::sidebar::SidebarEvent;
use diffpilot_core::cancel::CancelToken;
use diffpilot_core::claude::{ReviewError, Reviewer, ScriptedReplier};
use diffpilot_core::comparison::Comparison;
use diffpilot_core::review::{Finding, ReviewRequest, ReviewResult, Severity};
use diffpilot_core::settings::Settings;
use diffpilot_core::store::Store;
use diffpilot_core::viewed::ViewedStore;
use gpui::{AppContext as _, Entity, TestAppContext, VisualTestContext};
use std::path::Path;
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
    let repo = dir.path();
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
    dir
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
    cx.update(diffpilot_ui_kit::init);
    let repo = repo.to_path_buf();
    let mut view = None;
    let handle = cx.add_window(|window, cx| {
        let review_store = Arc::new(Store::open_in_memory().expect("an in-memory store opens"));
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

pub fn pick_comparison(view: &Entity<RepoView>, comparison: Comparison, cx: &mut VisualTestContext) {
    view.update_in(cx, |view, window, cx| view.select_comparison(comparison, window, cx));
    cx.run_until_parked();
}

pub fn sidebar_paths(view: &Entity<RepoView>, cx: &mut VisualTestContext) -> Vec<String> {
    let sidebar = view.read_with(cx, |view, _| view.sidebar());
    sidebar.read_with(cx, |sidebar, _| sidebar.paths())
}

pub fn viewed_count(view: &Entity<RepoView>, cx: &mut VisualTestContext) -> usize {
    let sidebar = view.read_with(cx, |view, _| view.sidebar());
    sidebar.read_with(cx, |sidebar, _| sidebar.summary().viewed)
}

pub fn select_file(view: &Entity<RepoView>, path: &str, cx: &mut VisualTestContext) {
    let sidebar = view.read_with(cx, |view, _| view.sidebar());
    let index = sidebar
        .read_with(cx, |sidebar, _| sidebar.paths())
        .iter()
        .position(|row| row == path)
        .unwrap_or_else(|| panic!("{path} is not in the sidebar"));
    sidebar.update_in(cx, |sidebar, window, cx| sidebar.select_index(index, window, cx));
    cx.run_until_parked();
}

pub fn visible_file(view: &Entity<RepoView>, cx: &mut VisualTestContext) -> Option<String> {
    let pane = view.read_with(cx, |view, _| view.pane());
    pane.read_with(cx, |pane, _| pane.state().visible_path().map(str::to_owned))
}

pub fn toggle_viewed(view: &Entity<RepoView>, path: &str, cx: &mut VisualTestContext) {
    let sidebar = view.read_with(cx, |view, _| view.sidebar());
    let path = path.to_owned();
    sidebar.update(cx, |_, cx| cx.emit(SidebarEvent::ToggleViewed(path)));
    cx.run_until_parked();
}
