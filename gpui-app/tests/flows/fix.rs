use diffpilot::testing::{
    canned_claude, click, fake_claude, fixture, open_repo, press, shows, type_text, wait_until_shows,
};
use diffpilot_core::policy::EditPermission;
use diffpilot_core::settings::Settings;
use diffpilot_core::viewed::ViewedStore;
use gpui::TestAppContext;
use std::sync::Arc;

#[gpui::test]
fn fix_with_claude_runs_on_the_comments_and_command_period_stops_it(app: &mut TestAppContext) {
    let claude = fake_claude(app);
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

    click(cx, "sidebar:row:docs/committed.md");
    click(cx, "pill:comment-on-file");
    type_text(cx, "Rename this.");
    press(cx, "cmd-enter");
    assert!(shows(cx, "pill:fix-ready:1"), "the pill offers to fix the open comment");

    click(cx, "pill:fix-threads");
    wait_until_shows(cx, "pill:stop-fix:working");
    claude.wait_for_launches(cx, 1);

    press(cx, "cmd-.");
    click(cx, "sidebar:row:src/staged.rs");
    click(cx, "pill:comment-on-file");
    type_text(cx, "Rename that.");
    press(cx, "cmd-enter");
    wait_until_shows(cx, "pill:fix-ready:2");

    click(cx, "pill:fix-threads");
    claude.wait_for_launches(cx, 2);
    claude.finish();
    wait_until_shows(cx, "pill:fix-result");
}
