use diffpilot::testing::{
    click, clipboard, fake_claude, fixture, git, open_repo, press, shows, type_text, wait_until_shows,
};
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

#[gpui::test]
fn generate_fills_the_message_from_claude_and_the_user_commits_it(app: &mut TestAppContext) {
    let claude = fake_claude(app);
    claude.reply_with("Add the fixture files\n\nThey cover edits and new files.");
    let (dir, cx) = open_fixture(app);

    click(cx, "bottom-bar:commit");
    click(cx, "commit:generate");
    wait_until_shows(cx, "commit:stop");
    claude.wait_for_launches(cx, 1);
    assert!(claude.prompt().contains("src/staged.rs"), "the prompt carries the diff");
    claude.finish();
    // `commit:generate` was on screen before the run, so it cannot tell when the message has arrived.
    let message = "Add the fixture files\n\nThey cover edits and new files.";
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while clipboard(cx).as_deref() != Some(message) {
        assert!(
            std::time::Instant::now() < deadline,
            "the message never reached the input"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
        press(cx, "cmd-a");
        press(cx, "cmd-c");
    }

    click(cx, "commit:submit");
    wait_until_shows(cx, "bottom-bar:commit:disabled");
    assert_eq!(
        last_message(dir.path()),
        "Add the fixture files\n\nThey cover edits and new files."
    );
}

#[gpui::test]
fn the_run_shows_in_the_claude_area_and_stop_ends_it(app: &mut TestAppContext) {
    let claude = fake_claude(app);
    let (_dir, cx) = open_fixture(app);

    click(cx, "bottom-bar:commit");
    click(cx, "commit:generate");
    claude.wait_for_launches(cx, 1);
    wait_until_shows(cx, "pill:working:Commit message");
    click(cx, "pill:working:Commit message");
    wait_until_shows(cx, "claude-area:title:Commit message");

    click(cx, "bottom-bar:commit");
    click(cx, "commit:stop");
    wait_until_shows(cx, "commit:generate");
    wait_until_shows(cx, "claude:status:Commit message stopped");
    click(cx, "claude-area:past-runs");
    assert!(
        shows(cx, "claude-area-past-menu:Commit message"),
        "the history lists runs of every kind"
    );
}

#[gpui::test]
fn a_failed_run_shows_the_error_and_leaves_the_message_alone(app: &mut TestAppContext) {
    let claude = fake_claude(app);
    claude.fail_with("Claude is out of credit.");
    let (_dir, cx) = open_fixture(app);

    click(cx, "bottom-bar:commit");
    click(cx, "commit:generate");
    claude.wait_for_launches(cx, 1);
    claude.finish();

    wait_until_shows(cx, "commit:error:Claude is out of credit.");
    assert!(shows(cx, "commit:generate"));
}
