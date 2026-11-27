use crate::common::{finding, result};
use diffpilot::testing::{
    FakeClaude, canned_review, click, fake_claude, fixture, fixture_named, open_repo, open_workspace, press, shows,
    wait_until_shows,
};
use diffpilot_core::policy::EditPermission;
use diffpilot_core::review::Severity;
use diffpilot_core::settings::Settings;
use diffpilot_core::viewed::ViewedStore;
use gpui::{TestAppContext, VisualTestContext};
use std::sync::Arc;

fn open(app: &mut TestAppContext, settings: Settings) -> (tempfile::TempDir, &'static mut VisualTestContext) {
    let dir = fixture();
    let (_, cx) = open_repo(app, dir.path(), Arc::new(settings), Arc::new(ViewedStore::in_memory()));
    (dir, cx)
}

fn start_review(cx: &mut VisualTestContext) {
    click(cx, "review:open");
    click(cx, "review:start");
}

#[gpui::test]
fn the_shortcut_the_bottom_bar_button_and_the_close_button_toggle_the_area(app: &mut TestAppContext) {
    let (_dir, repo) = fixture_named("alpha");
    let cx = open_workspace(app, &repo, Arc::new(Settings::in_memory()));
    click(cx, "sidebar:row:docs/committed.md");
    assert!(shows(cx, "claude:toggle:0"));

    press(cx, "shift-cmd-y");
    assert!(shows(cx, "claude-area:1"), "the shortcut opens the area");
    assert!(shows(cx, "claude-area:empty"), "with no run it says so");

    click(cx, "claude:toggle:1");
    assert!(shows(cx, "claude:toggle:2"));
    assert!(!shows(cx, "claude-area:2"), "the bottom bar button closes it");

    click(cx, "claude:toggle:2");
    assert!(shows(cx, "claude-area:3"));
    click(cx, "claude-area:close");
    assert!(shows(cx, "claude:toggle:4"));
    assert!(!shows(cx, "claude-area:4"), "the close button closes it");

    press(cx, "shift-cmd-y");
    assert!(shows(cx, "claude-area:5"));
    click(cx, "claude-area:toggle");
    assert!(!shows(cx, "claude-area:6"), "the panel button in the bar closes it too");
}

#[gpui::test]
fn a_review_fills_the_area_with_its_plan_and_log_and_the_area_closes_when_it_is_done(app: &mut TestAppContext) {
    let fake = fake_claude(app);
    fake.review_with(&canned_review());
    fake.hold();
    let (_dir, cx) = open(app, Settings::in_memory());
    assert!(shows(cx, "claude:status:Claude · no runs yet"));

    start_review(cx);
    wait_until_shows(cx, "pill:working:Review");
    click(cx, "pill:working:Review");
    assert!(shows(cx, "claude-area:1"), "a click on the running pill opens the area");

    wait_until_shows(cx, "claude-area:step:done:Read the diff");
    assert!(shows(cx, "claude-area:step:active:Checking the callers"));
    assert!(shows(cx, "claude-area:step:pending:Write the review"));
    wait_until_shows(cx, "claude-area:log:read:Read src/staged.rs");
    assert!(shows(cx, "claude-area:log:search:Searched value"));
    assert!(shows(cx, "claude-area:log:command:Ran git diff --stat"));
    wait_until_shows(cx, "claude-area:comment:Commented on base.txt:2");
    assert!(
        shows(cx, "claude-area:thinking"),
        "a running run ends its log with Thinking…"
    );
    assert!(
        shows(cx, "claude:status:Review · commenting"),
        "the bottom bar names the activity"
    );

    fake.finish();
    wait_until_shows(cx, "claude:toggle:2");
    assert!(
        !shows(cx, "claude-area:2"),
        "the area closes itself when the run is done"
    );
    wait_until_shows(cx, "claude:status:Review finished · 4 comments · 7 tool calls");
    assert!(
        shows(cx, "jump-bar:threads:5"),
        "the summary and the four findings are threads"
    );
}

#[gpui::test]
fn a_stopped_run_keeps_the_area_open_and_run_again_starts_a_new_run(app: &mut TestAppContext) {
    let fake = fake_claude(app);
    fake.review_with(&canned_review());
    fake.hold();
    let (_dir, cx) = open(app, Settings::in_memory());
    start_review(cx);
    fake.wait_for_launches(cx, 1);
    wait_until_shows(cx, "claude:status:Review · commenting");

    click(cx, "claude:toggle:0");
    assert!(
        shows(cx, "claude-area:1"),
        "the bottom bar button opens it while a run goes"
    );
    click(cx, "claude-area:stop");
    wait_until_shows(cx, "claude-area:run-again");
    assert!(shows(cx, "claude:status:Review stopped"));
    assert!(shows(cx, "claude-area:1"));

    click(cx, "claude-area:run-again");
    fake.wait_for_launches(cx, 2);
    wait_until_shows(cx, "claude-area:stop");
    fake.finish();
    wait_until_shows(cx, "claude:toggle:2");
    assert!(shows(cx, "jump-bar:threads:5"));
}

#[gpui::test]
fn past_runs_lists_the_earlier_run_and_picking_it_shows_its_log(app: &mut TestAppContext) {
    let fake = fake_claude(app);
    fake.review_with(&canned_review());
    fake.finish();
    let settings = Settings::in_memory();
    settings.set_edit_permission(EditPermission::DontAsk);
    let (_dir, cx) = open(app, settings);
    start_review(cx);
    wait_until_shows(cx, "claude:status:Review finished · 4 comments · 7 tool calls");

    fake.hold();
    wait_until_shows(cx, "fix:open");
    click(cx, "fix:open");
    click(cx, "fix:send");
    fake.wait_for_launches(cx, 2);
    wait_until_shows(cx, "pill:working:Fix Comments");
    click(cx, "pill:working:Fix Comments");
    assert!(shows(cx, "claude-area:1"));
    assert!(shows(cx, "claude-area:title:Fix Comments"));
    assert!(!shows(cx, "claude-area:title:Review"), "the area follows the fix run");

    click(cx, "claude-area:past-runs");
    assert!(
        shows(cx, "claude-area-past-menu:Fixing 4 comments"),
        "the menu lists the fix run"
    );
    assert!(
        shows(cx, "claude-area-past-menu:feature compared to main · 3 files"),
        "the menu lists the review"
    );
    click(cx, "claude-area-past-menu:feature compared to main · 3 files");
    assert!(shows(cx, "claude-area:title:Review"), "the area shows the picked run");
    assert!(shows(cx, "claude-area:comment:Commented on docs/committed.md:1"));

    click(cx, "claude-area:past-runs");
    click(cx, "claude-area-past-menu:Follow current run");
    wait_until_shows(cx, "claude-area:stop");
    fake.finish();
}

fn write_guide(app: &mut TestAppContext) -> (FakeClaude, tempfile::TempDir, &'static mut VisualTestContext) {
    let fake = fake_claude(app);
    fake.hold();
    let (dir, repo) = fixture_named("alpha");
    let cx = open_workspace(app, &repo, Arc::new(Settings::in_memory()));
    click(cx, "sidebar:row:docs/committed.md");
    press(cx, "cmd-2");
    click(cx, "guide-write");
    fake.wait_for_launches(cx, 1);
    (fake, dir, cx)
}

fn save_a_guide(fake: &FakeClaude) {
    let call = |tool: &str, args: serde_json::Value| {
        let reply = fake.call_mcp_tool(tool, args);
        assert_eq!(reply["ok"], true, "{tool}: {reply}");
    };
    call(
        "set_overview",
        serde_json::json!({ "title": "Adds a staged file", "summary": "It adds a file and edits the base." }),
    );
    for (title, files) in [
        ("The staged file", serde_json::json!(["src/staged.rs"])),
        ("The small edits", serde_json::json!(["base.txt", "docs/committed.md"])),
    ] {
        call(
            "add_chapter",
            serde_json::json!({ "title": title, "summary": "Why it exists.", "files": files }),
        );
    }
    call("finish_guide", serde_json::json!({}));
}

#[gpui::test]
fn a_guide_run_shows_its_log_in_the_area_and_its_chapters_in_the_status(app: &mut TestAppContext) {
    let (fake, _dir, cx) = write_guide(app);
    click(cx, "claude:toggle:0");
    wait_until_shows(cx, "claude-area:title:Guide");
    wait_until_shows(cx, "claude-area:log:search:Searched value");
    assert!(shows(cx, "claude-area:thinking"));
    assert!(shows(cx, "pill:working:Guide"), "the pill can stop the guide run");

    save_a_guide(&fake);
    fake.finish();
    wait_until_shows(cx, "claude:status:Guide finished · 2 chapters · 3 tool calls");
    wait_until_shows(cx, "claude:toggle:2");
    click(cx, "claude:toggle:2");
    click(cx, "claude-area:past-runs");
    assert!(shows(cx, "claude-area-past-menu:Writing a guide"));
}

#[gpui::test]
fn a_review_queued_behind_a_guide_run_starts_when_the_guide_is_saved(app: &mut TestAppContext) {
    let (fake, _dir, cx) = write_guide(app);
    fake.review_with(&canned_review());
    click(cx, "claude:toggle:0");
    wait_until_shows(cx, "claude-area:stop");
    click(cx, "claude-area:queue");
    click(cx, "review:start");
    wait_until_shows(cx, "claude-area:queued:1");
    assert_eq!(fake.launches(), 1, "the review waits for the guide");

    save_a_guide(&fake);
    fake.finish();
    fake.wait_for_launches(cx, 2);
    wait_until_shows(cx, "claude:status:Review finished · 4 comments · 7 tool calls");
}

#[gpui::test]
fn queue_starts_a_second_review_after_the_first(app: &mut TestAppContext) {
    let fake = fake_claude(app);
    fake.review_with(&result("", vec![finding("base.txt", Some(2), Severity::Nit, "Only")]));
    fake.hold();
    let (_dir, cx) = open(app, Settings::in_memory());
    start_review(cx);
    fake.wait_for_launches(cx, 1);
    click(cx, "claude:toggle:0");
    wait_until_shows(cx, "claude-area:stop");

    click(cx, "claude-area:queue");
    click(cx, "review:start");
    wait_until_shows(cx, "claude-area:queued:1");
    assert_eq!(fake.launches(), 1, "the second run waits for the first");

    fake.finish_after(1.5);
    fake.finish();
    fake.wait_for_launches(cx, 2);
    assert!(
        !shows(cx, "claude:toggle:2"),
        "the area stays open while the queued review runs"
    );
    wait_until_shows(cx, "claude:toggle:2");
}

#[gpui::test]
fn a_paused_run_keeps_its_queue_waiting_and_continue_resumes_its_session(app: &mut TestAppContext) {
    let fake = fake_claude(app);
    fake.review_with(&canned_review());
    fake.hold();
    let (_dir, cx) = open(app, Settings::in_memory());
    start_review(cx);
    fake.wait_for_launches(cx, 1);
    click(cx, "claude:toggle:0");
    wait_until_shows(cx, "claude-area:stop");
    click(cx, "claude-area:queue");
    click(cx, "review:start");
    wait_until_shows(cx, "claude-area:queued:1");

    click(cx, "claude-area:pause");
    wait_until_shows(cx, "claude:status:Review · paused");
    assert!(shows(cx, "claude-area:continue"));
    assert!(shows(cx, "pill:continue"), "the pill offers Continue too");
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert_eq!(fake.launches(), 1, "the queued review waits behind the paused one");

    click(cx, "claude-area:continue");
    fake.wait_for_launches(cx, 2);
    assert_eq!(fake.resumed(), ["s-1"], "Continue resumes the run's Claude session");
    fake.finish();
    fake.wait_for_launches(cx, 3);
    assert_eq!(fake.resumed().len(), 1, "the queued review starts a new session");
}

#[gpui::test]
fn the_pill_pauses_and_stops_a_paused_run(app: &mut TestAppContext) {
    let fake = fake_claude(app);
    fake.review_with(&canned_review());
    fake.hold();
    let (_dir, cx) = open(app, Settings::in_memory());
    start_review(cx);
    fake.wait_for_launches(cx, 1);
    wait_until_shows(cx, "pill:pause");

    click(cx, "pill:pause");
    wait_until_shows(cx, "claude:status:Review · paused");
    click(cx, "pill:stop");
    wait_until_shows(cx, "claude:status:Review stopped");
    assert_eq!(fake.launches(), 1);
    assert!(fake.resumed().is_empty());
}
