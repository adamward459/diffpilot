use diffpilot::testing::{
    FakeGithub, RemoteFixture, click, install_github, open_repo, open_workspace, press, remote_fixture, shows,
    wait_until_shows,
};
use diffpilot_core::settings::Settings;
use diffpilot_core::viewed::ViewedStore;
use gpui::{TestAppContext, VisualTestContext};
use std::sync::Arc;

fn open(app: &mut TestAppContext, fixture: &RemoteFixture) -> &'static mut VisualTestContext {
    let (_, cx) = open_repo(
        app,
        &fixture.repo,
        Arc::new(Settings::in_memory()),
        Arc::new(ViewedStore::in_memory()),
    );
    cx
}

#[gpui::test]
fn the_status_popover_pushes_the_commits_that_wait(app: &mut TestAppContext) {
    let fixture = remote_fixture();
    fixture.commit_here("one.txt");
    fixture.commit_here("two.txt");
    let cx = open(app, &fixture);
    wait_until_shows(cx, "remote:counts:2:0");

    click(cx, "remote:status");
    assert!(shows(cx, "remote:header:2 commits to push"));
    assert!(shows(cx, "remote:detail:push:To origin/feature"));
    click(cx, "remote:push");

    wait_until_shows(cx, "remote:counts:0:0");
}

#[gpui::test]
fn the_push_shortcut_pushes_without_opening_the_popover(app: &mut TestAppContext) {
    let fixture = remote_fixture();
    fixture.commit_here("one.txt");
    let cx = open_workspace(app, &fixture.repo, Arc::new(Settings::in_memory()));
    wait_until_shows(cx, "remote:counts:1:0");

    click(cx, "sidebar:row:docs/committed.md");
    press(cx, "shift-cmd-u");

    wait_until_shows(cx, "remote:counts:0:0");
}

#[gpui::test]
fn behind_the_remote_the_popover_asks_for_a_pull_first(app: &mut TestAppContext) {
    let fixture = remote_fixture();
    fixture.commit_here("mine.txt");
    fixture.commit_on_remote("theirs.txt");
    let cx = open(app, &fixture);
    wait_until_shows(cx, "remote:counts:1:1");

    click(cx, "remote:status");
    assert!(shows(cx, "remote:header:1 commit to push · 1 commit to pull"));
    assert!(shows(cx, "remote:warning"));
    assert!(shows(cx, "remote:push:disabled"), "push is dimmed");
    assert!(shows(cx, "remote:detail:push:After you pull"));
    assert!(shows(cx, "remote:detail:pull:Fast-forward to origin/feature"));
}

#[gpui::test]
fn the_pull_row_brings_the_remote_commits_in(app: &mut TestAppContext) {
    let fixture = remote_fixture();
    fixture.commit_on_remote("theirs.txt");
    let cx = open(app, &fixture);
    wait_until_shows(cx, "remote:counts:0:1");

    click(cx, "remote:status");
    click(cx, "remote:pull");

    wait_until_shows(cx, "remote:counts:0:0");
}

#[gpui::test]
fn a_failed_pull_shows_why_in_the_popover(app: &mut TestAppContext) {
    let fixture = remote_fixture();
    fixture.commit_on_remote("theirs.txt");
    std::fs::write(fixture.repo.join("base.txt"), "edited\n").expect("edit a tracked file");
    let cx = open(app, &fixture);
    wait_until_shows(cx, "remote:counts:0:1");

    click(cx, "remote:status");
    click(cx, "remote:pull");

    wait_until_shows(cx, "remote:error:pull");
}

#[gpui::test]
fn the_branch_with_a_pull_request_shows_its_chip_checks_and_review(app: &mut TestAppContext) {
    install_github(app, FakeGithub::new());
    let fixture = remote_fixture();
    let cx = open(app, &fixture);

    wait_until_shows(cx, "remote:pr-chip:482");
    wait_until_shows(cx, "remote:pr-dot:failed");
    assert!(shows(cx, "remote:review:Changes requested"));
    assert!(shows(cx, "remote:checks-detail:1 failed"));
}

#[gpui::test]
fn the_checks_popover_lists_the_jobs_and_reruns_the_failed_ones(app: &mut TestAppContext) {
    let github = FakeGithub::new();
    install_github(app, github.clone());
    let fixture = remote_fixture();
    let cx = open(app, &fixture);
    wait_until_shows(cx, "remote:checks-dot:failed");

    click(cx, "remote:checks");
    assert!(shows(cx, "checks:header:Checks · PR #482"));
    assert!(shows(cx, "checks:group:GitHub Actions"));
    assert!(shows(cx, "checks:group:Vercel"));
    assert!(shows(cx, "checks:group:Commit statuses"));
    assert!(shows(cx, "checks:job:unit tests"));
    assert!(shows(cx, "checks:duration:unit tests:2m 00s"));
    assert!(shows(cx, "checks:rerun:unit tests"));
    assert!(shows(cx, "checks:log:unit tests"));
    assert!(!shows(cx, "checks:rerun:Vercel"), "only Actions jobs can run again");

    click(cx, "checks:rerun-failed");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while github.posts().is_empty() {
        assert!(std::time::Instant::now() < deadline, "no re-run was sent");
        std::thread::sleep(std::time::Duration::from_millis(20));
        diffpilot::testing::draw(cx);
    }
    assert_eq!(
        github.posts(),
        ["https://api.github.com/repos/o/r/actions/runs/1289/rerun-failed-jobs"]
    );
}

#[gpui::test]
fn without_a_github_login_the_pull_request_parts_stay_hidden(app: &mut TestAppContext) {
    let fixture = remote_fixture();
    let cx = open(app, &fixture);
    wait_until_shows(cx, "remote:counts:0:0");
    assert!(!shows(cx, "remote:checks"));
    assert!(!shows(cx, "remote:pr-chip:482"));
}
