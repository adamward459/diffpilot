use diffpilot::testing::{click, fake_claude, fixture, hover, open_repo_in, open_workspace, press, shows, type_text};
use diffpilot_core::settings::Settings;
use diffpilot_core::threads::USER_NAME;
use diffpilot_core::viewed::ViewedStore;
use gpui::{TestAppContext, VisualTestContext};
use std::path::Path;
use std::sync::Arc;

fn open(
    app: &mut TestAppContext,
    repo: &Path,
    store: Arc<diffpilot_core::store::Store>,
) -> &'static mut VisualTestContext {
    let (_, cx) = open_repo_in(
        app,
        repo,
        Arc::new(Settings::in_memory()),
        Arc::new(ViewedStore::in_memory()),
        store,
    );
    cx
}

fn comment_on_file(cx: &mut VisualTestContext, path: &str, text: &str) {
    click(cx, &format!("sidebar:row:{path}"));
    click(cx, &format!("pane:comment-on-file:{path}"));
    type_text(cx, text);
    press(cx, "cmd-enter");
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
