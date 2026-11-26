use diffpilot::testing::{canned_claude, click, fixture, git, open_repo, press, shows, type_text, wait_until_shows};
use diffpilot_core::settings::Settings;
use diffpilot_core::viewed::ViewedStore;
use gpui::{TestAppContext, VisualTestContext};
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

fn last_message(repo: &Path) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["log", "-1", "--format=%B"])
        .output()
        .expect("git log");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn open_fixture(app: &mut TestAppContext) -> (tempfile::TempDir, &'static mut VisualTestContext) {
    let dir = fixture();
    git(dir.path(), &["config", "user.name", "t"]);
    git(dir.path(), &["config", "user.email", "t@t"]);
    git(dir.path(), &["config", "commit.gpgsign", "false"]);
    let (_, cx) = open_repo(
        app,
        dir.path(),
        Arc::new(Settings::in_memory()),
        Arc::new(ViewedStore::in_memory()),
        canned_claude(),
    );
    (dir, cx)
}

#[gpui::test]
fn the_commit_button_commits_all_changes_with_the_typed_message(app: &mut TestAppContext) {
    let (dir, cx) = open_fixture(app);
    assert!(shows(cx, "bottom-bar:commit"), "uncommitted changes enable the button");
    assert!(!shows(cx, "bottom-bar:commit:disabled"));

    click(cx, "bottom-bar:commit");
    type_text(cx, "Ship the fixture");
    click(cx, "commit:submit");

    wait_until_shows(cx, "bottom-bar:commit:disabled");
    assert_eq!(last_message(dir.path()), "Ship the fixture");
}

#[gpui::test]
fn cmd_enter_commits_too(app: &mut TestAppContext) {
    let (dir, cx) = open_fixture(app);

    click(cx, "bottom-bar:commit");
    type_text(cx, "Keyboard commit");
    press(cx, "cmd-enter");

    wait_until_shows(cx, "bottom-bar:commit:disabled");
    assert_eq!(last_message(dir.path()), "Keyboard commit");
}
