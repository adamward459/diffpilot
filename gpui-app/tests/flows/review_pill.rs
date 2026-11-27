use diffpilot::review::pill::RESULT_SECONDS;
use diffpilot::testing::{
    advance_clock, bounds, canned_review, click, fake_claude, fixture, open_repo, press, shows, wait_until_shows,
};
use diffpilot_core::settings::Settings;
use diffpilot_core::viewed::ViewedStore;
use gpui::{Pixels, TestAppContext, VisualTestContext};
use std::sync::Arc;

fn open(app: &mut TestAppContext) -> (tempfile::TempDir, &'static mut VisualTestContext) {
    let dir = fixture();
    let (_, cx) = open_repo(
        app,
        dir.path(),
        Arc::new(Settings::in_memory()),
        Arc::new(ViewedStore::in_memory()),
    );
    (dir, cx)
}

fn start_review(cx: &mut VisualTestContext) {
    click(cx, "review:open");
    click(cx, "review:start");
}

/// The gap between the two comparison menus, which the toolbar closes to make room for the pill.
fn comparison_gap(cx: &mut VisualTestContext) -> Pixels {
    bounds(cx, "comparison-base").left() - bounds(cx, "comparison-branch").right()
}

#[gpui::test]
fn the_toolbar_makes_room_while_a_review_runs_and_while_its_result_shows(app: &mut TestAppContext) {
    let fake = fake_claude(app);
    fake.review_with(&canned_review());
    fake.hold();
    let (_dir, cx) = open(app);
    let roomy = comparison_gap(cx);

    start_review(cx);
    fake.wait_for_launches(cx, 1);
    wait_until_shows(cx, "pill:working:Review");
    assert!(comparison_gap(cx) < roomy, "the working pill gets room");

    fake.finish();
    wait_until_shows(cx, "review:done");
    assert!(comparison_gap(cx) < roomy, "the result gets room");

    advance_clock(cx, RESULT_SECONDS + 1);
    assert_eq!(
        comparison_gap(cx),
        roomy,
        "the result is gone, so the room is given back"
    );
}

#[gpui::test]
fn command_period_stops_a_running_review_and_gives_the_room_back(app: &mut TestAppContext) {
    let fake = fake_claude(app);
    fake.review_with(&canned_review());
    fake.hold();
    let (_dir, cx) = open(app);
    let roomy = comparison_gap(cx);

    start_review(cx);
    fake.wait_for_launches(cx, 1);
    wait_until_shows(cx, "pill:working:Review");
    assert!(comparison_gap(cx) < roomy);

    press(cx, "cmd-.");
    wait_until_shows(cx, "claude:status:Review stopped");
    assert_eq!(comparison_gap(cx), roomy);
}

#[gpui::test]
fn a_failed_review_opens_its_message_and_trying_again_runs_a_new_review(app: &mut TestAppContext) {
    let fake = fake_claude(app);
    fake.fail_with("boom");
    fake.finish();
    let (_dir, cx) = open(app);
    start_review(cx);
    wait_until_shows(cx, "review:problem:boom");
    assert!(shows(cx, "review:start"), "the popover offers another try");

    fake.review_with(&canned_review());
    click(cx, "review:start");
    wait_until_shows(cx, "review:done");
    assert_eq!(fake.launches(), 2);
    assert!(shows(cx, "jump-bar:threads:5"));
}

#[gpui::test]
fn reviewing_only_the_open_file_sends_just_that_file_to_claude(app: &mut TestAppContext) {
    let fake = fake_claude(app);
    fake.review_with(&canned_review());
    fake.finish();
    let (_dir, cx) = open(app);
    click(cx, "sidebar:row:src/staged.rs");

    click(cx, "review:open");
    click(cx, "review:scope:open");
    click(cx, "review:start");
    wait_until_shows(cx, "review:done");
    let prompt = fake.prompt();
    assert!(prompt.contains("src/staged.rs"), "{prompt}");
    assert!(!prompt.contains("docs/committed.md"), "{prompt}");
}
