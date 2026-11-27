use crate::common::{comment_on_file, open_on as open};
use diffpilot::testing::{
    bounds, click, fake_claude, fixture, hover, open_workspace, press, shows, type_text, wait_for_motion,
};
use diffpilot_core::settings::Settings;
use diffpilot_core::threads::USER_NAME;
use gpui::TestAppContext;
use std::sync::Arc;

#[gpui::test]
fn pointing_at_a_comment_row_shows_its_actions_without_resizing_the_row(app: &mut TestAppContext) {
    let fake = fake_claude(app);
    let dir = fixture();
    let cx = open(app, dir.path(), fake.store());
    comment_on_file(cx, "docs/committed.md", "Fix this.");
    click(cx, "toolbar:comments");
    wait_for_motion(cx);
    let at_rest = bounds(cx, "comments:row:Fix this.");

    hover(cx, "comments:row:Fix this.");
    assert!(shows(cx, "comments:resolve:Fix this."));
    assert_eq!(bounds(cx, "comments:row:Fix this."), at_rest);
}

#[gpui::test]
fn a_resolved_comment_is_listed_under_resolved_and_reopening_it_puts_it_back_in_the_diff(app: &mut TestAppContext) {
    let fake = fake_claude(app);
    let dir = fixture();
    let cx = open(app, dir.path(), fake.store());
    comment_on_file(cx, "docs/committed.md", "Fix this.");
    assert!(shows(cx, "toolbar:comments:1"), "the toolbar counts the open comment");
    click(cx, "thread:resolve:docs/committed.md");

    click(cx, "toolbar:comments");
    assert!(shows(cx, "comments:panel"));
    click(cx, "comments:status:resolved");
    assert!(shows(cx, "comments:row:Fix this."));
    hover(cx, "comments:row:Fix this.");
    assert!(
        !shows(cx, "comments:resolve:Fix this."),
        "a resolved row offers Reopen, not Resolve"
    );
    click(cx, "comments:reopen:Fix this.");

    click(cx, "comments:status:open");
    hover(cx, "comments:row:Fix this.");
    assert!(
        shows(cx, "comments:resolve:Fix this."),
        "the reopened comment is open again"
    );
    press(cx, "escape");

    let again = open(app, dir.path(), fake.store());
    assert!(shows(again, "toolbar:comments:1"));
    click(again, "sidebar:row:docs/committed.md");
    assert!(shows(again, &format!("comment:{USER_NAME}:Fix this.")));
}

#[gpui::test]
fn the_c_key_opens_the_panel_and_a_row_of_an_open_comment_focuses_it_in_the_diff(app: &mut TestAppContext) {
    let dir = fixture();
    let cx = open_workspace(app, dir.path(), Arc::new(Settings::in_memory()));
    comment_on_file(cx, "docs/committed.md", "Open note.");
    click(cx, "sidebar:row:base.txt");

    press(cx, "c");
    assert!(shows(cx, "comments:panel"));
    click(cx, "comments:row:Open note.");
    type_text(cx, "A reply.");
    press(cx, "cmd-enter");
    assert!(
        shows(cx, &format!("comment:{USER_NAME}:A reply.")),
        "the panel closed and the cursor is in the thread"
    );
}

#[gpui::test]
fn escape_and_a_click_outside_close_the_panel(app: &mut TestAppContext) {
    let dir = fixture();
    let cx = open_workspace(app, dir.path(), Arc::new(Settings::in_memory()));

    press(cx, "c");
    assert!(shows(cx, "comments:panel"));
    press(cx, "escape");

    press(cx, "c");
    click(cx, "sidebar:row:src/staged.rs");
    assert!(
        !shows(cx, "jump-bar:path:src/staged.rs"),
        "Escape closed the panel, the second C reopened it, and the click outside only closed it"
    );
    click(cx, "sidebar:row:src/staged.rs");
    assert!(
        shows(cx, "jump-bar:path:src/staged.rs"),
        "the panel is gone, so the click selects the file"
    );
}

#[gpui::test]
fn the_panel_is_still_on_screen_while_it_closes_and_gone_after(app: &mut TestAppContext) {
    let dir = fixture();
    let cx = open_workspace(app, dir.path(), Arc::new(Settings::in_memory()));

    press(cx, "c");
    assert!(shows(cx, "comments:panel"));
    press(cx, "escape");
    assert!(
        shows(cx, "comments:overlay:closing"),
        "the panel is still drawn right after Escape"
    );

    wait_for_motion(cx);
    click(cx, "sidebar:row:src/staged.rs");
    assert!(
        shows(cx, "jump-bar:path:src/staged.rs"),
        "after the close time nothing covers the app, so the click selects the file"
    );
}

#[gpui::test]
fn opening_the_panel_again_during_its_close_keeps_it_open(app: &mut TestAppContext) {
    let dir = fixture();
    let cx = open_workspace(app, dir.path(), Arc::new(Settings::in_memory()));

    press(cx, "c");
    press(cx, "escape");
    press(cx, "c");
    wait_for_motion(cx);
    click(cx, "sidebar:row:src/staged.rs");
    assert!(
        !shows(cx, "jump-bar:path:src/staged.rs"),
        "the panel stayed open, so the click outside only closed it"
    );
}

#[gpui::test]
fn all_views_lists_comments_of_other_views_to_read_without_actions(app: &mut TestAppContext) {
    let fake = fake_claude(app);
    let dir = fixture();
    let cx = open(app, dir.path(), fake.store());
    comment_on_file(cx, "docs/committed.md", "From the branch view.");
    click(cx, "comparison-base");
    click(cx, "comparison-base:Staged Changes Only");
    comment_on_file(cx, "src/staged.rs", "From the staged view.");

    click(cx, "toolbar:comments");
    assert!(shows(cx, "comments:row:From the staged view."));
    assert!(
        !shows(cx, "comments:row:From the branch view."),
        "This view lists its own comments only"
    );
    click(cx, "comments:scope:all");
    assert!(shows(cx, "comments:row:From the branch view."));

    click(cx, "comments:row:From the branch view.");
    assert!(shows(cx, "comments:conversation:From the branch view."));
    hover(cx, "comments:row:From the branch view.");
    assert!(
        !shows(cx, "comments:resolve:From the branch view."),
        "another view is read only"
    );
    assert!(!shows(cx, "comments:delete:From the branch view."));
}
