use diffpilot::testing::{
    FakeClaude, canned_claude, click, fake_claude, fixture, open_repo, press, shows, type_text, wait_until_shows,
};
use diffpilot_core::policy::EditPermission;
use diffpilot_core::settings::Settings;
use diffpilot_core::viewed::ViewedStore;
use gpui::{TestAppContext, VisualTestContext};
use serde_json::json;
use std::sync::Arc;

fn open(app: &mut TestAppContext) -> (tempfile::TempDir, &'static mut VisualTestContext) {
    let dir = fixture();
    let settings = Arc::new(Settings::in_memory());
    settings.set_edit_permission(EditPermission::DontAsk);
    let (_, cx) = open_repo(
        app,
        dir.path(),
        settings,
        Arc::new(ViewedStore::in_memory()),
        canned_claude(),
    );
    (dir, cx)
}

fn threads_in_run(claude: &FakeClaude) -> String {
    claude.call_mcp_tool("list_threads", json!({})).to_string()
}

fn comment_on_file(cx: &mut VisualTestContext, path: &str, text: &str) {
    click(cx, &format!("sidebar:row:{path}"));
    click(cx, &format!("pane:comment-on-file:{path}"));
    type_text(cx, text);
    press(cx, "cmd-enter");
}

#[gpui::test]
fn fix_with_claude_runs_on_the_comments_and_command_period_stops_it(app: &mut TestAppContext) {
    let claude = fake_claude(app);
    let (_dir, cx) = open(app);

    comment_on_file(cx, "docs/committed.md", "Rename this.");
    assert!(shows(cx, "fix:ready:1"), "the toolbar offers to fix the open comment");

    click(cx, "fix:open");
    click(cx, "fix:send");
    wait_until_shows(cx, "pill:stop-fix:working");
    claude.wait_for_launches(cx, 1);

    press(cx, "cmd-.");
    comment_on_file(cx, "src/staged.rs", "Rename that.");
    wait_until_shows(cx, "fix:ready:2");

    click(cx, "fix:open");
    click(cx, "fix:send");
    claude.wait_for_launches(cx, 2);
    claude.finish();
    wait_until_shows(cx, "fix:result");
}

#[gpui::test]
fn an_unchecked_comment_in_the_picker_is_left_out_of_the_fix(app: &mut TestAppContext) {
    let claude = fake_claude(app);
    let (_dir, cx) = open(app);
    comment_on_file(cx, "docs/committed.md", "Rename this.");
    comment_on_file(cx, "src/staged.rs", "Leave that alone.");

    click(cx, "fix:open");
    wait_until_shows(cx, "fix:row:src/staged.rs");
    click(cx, "fix:row:src/staged.rs");
    click(cx, "fix:send");
    claude.wait_for_launches(cx, 1);

    let threads = threads_in_run(&claude);
    assert!(threads.contains("Rename this."), "{threads}");
    assert!(!threads.contains("Leave that alone."), "{threads}");
    claude.finish();
    wait_until_shows(cx, "fix:result");
}

#[gpui::test]
fn send_to_claude_in_the_comment_box_posts_the_comment_and_fixes_that_thread(app: &mut TestAppContext) {
    let claude = fake_claude(app);
    let (_dir, cx) = open(app);
    comment_on_file(cx, "src/staged.rs", "Leave that alone.");

    click(cx, "sidebar:row:docs/committed.md");
    click(cx, "pane:comment-on-file:docs/committed.md");
    type_text(cx, "Rename this.");
    click(cx, "thread:send:docs/committed.md");
    claude.wait_for_launches(cx, 1);

    assert!(shows(cx, "comment:You:Rename this."), "the comment is posted");
    let threads = threads_in_run(&claude);
    assert!(threads.contains("Rename this."), "{threads}");
    assert!(!threads.contains("Leave that alone."), "{threads}");
    claude.finish();
    wait_until_shows(cx, "fix:result");
}

#[gpui::test]
fn the_fix_button_stays_while_a_review_runs(app: &mut TestAppContext) {
    let claude = fake_claude(app);
    let (_dir, cx) = open(app);
    comment_on_file(cx, "docs/committed.md", "Rename this.");

    click(cx, "review:open");
    click(cx, "review:start");
    claude.wait_for_launches(cx, 1);
    wait_until_shows(cx, "pill:stop-review:working");
    assert!(shows(cx, "fix:ready:1"), "Fix stays next to the working pill");

    click(cx, "fix:open");
    click(cx, "fix:send");
    claude.finish();
    claude.wait_for_launches(cx, 2);
    wait_until_shows(cx, "fix:result");
}

#[gpui::test]
fn command_shift_return_in_the_comment_box_sends_it_to_claude(app: &mut TestAppContext) {
    let claude = fake_claude(app);
    let (_dir, cx) = open(app);

    click(cx, "sidebar:row:docs/committed.md");
    click(cx, "pane:comment-on-file:docs/committed.md");
    type_text(cx, "Rename this.");
    press(cx, "cmd-shift-enter");
    claude.wait_for_launches(cx, 1);

    assert!(threads_in_run(&claude).contains("Rename this."));
    claude.finish();
    wait_until_shows(cx, "fix:result");
}
