use diffpilot::testing::{click, open_preview, shows, wait_until_shows};
use gpui::TestAppContext;

#[gpui::test]
fn images_show_a_preview_until_the_diff_is_chosen(app: &mut TestAppContext) {
    let (_opened, cx) = open_preview(app);
    wait_until_shows(cx, "pane:preview-card:assets/logo.png");
    click(cx, "pane:preview-switch:assets/logo.png:Diff");
    assert!(shows(cx, "pane:notice:assets/logo.png"));
}

#[gpui::test]
fn markdown_shows_the_diff_until_preview_is_chosen(app: &mut TestAppContext) {
    let (_opened, cx) = open_preview(app);
    wait_until_shows(cx, "pane:hunk:notes.md:0");
    assert!(!shows(cx, "pane:preview-card:notes.md"));
    click(cx, "pane:preview-switch:notes.md:Preview");
    wait_until_shows(cx, "pane:preview-card:notes.md");
}
