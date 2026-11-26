use diffpilot::testing::{click, clipboard, open, shows, wait_until_shows};
use gpui::TestAppContext;

#[gpui::test]
fn the_more_menu_marks_the_file_as_viewed(app: &mut TestAppContext) {
    let (_opened, cx) = open(app);
    wait_until_shows(cx, "pane:file-more:docs/committed.md");
    click(cx, "pane:file-more:docs/committed.md");
    click(cx, "file-more:docs/committed.md:Mark as Viewed");
    assert!(shows(cx, "sidebar:is-viewed:docs/committed.md"));
}

#[gpui::test]
fn the_more_menu_copies_the_path(app: &mut TestAppContext) {
    let (_opened, cx) = open(app);
    wait_until_shows(cx, "pane:file-more:docs/committed.md");
    click(cx, "pane:file-more:docs/committed.md");
    click(cx, "file-more:docs/committed.md:Copy Path");
    assert_eq!(clipboard(cx).as_deref(), Some("docs/committed.md"));
}
