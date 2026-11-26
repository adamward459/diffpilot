use diffpilot::testing::{click, open_preview, shows, wait_until_shows};
use gpui::TestAppContext;

#[gpui::test]
fn images_and_markdown_show_a_preview_until_the_diff_is_chosen(app: &mut TestAppContext) {
    let (_opened, cx) = open_preview(app);
    wait_until_shows(cx, "pane:preview-card:notes.md");
    assert!(shows(cx, "pane:preview-card:assets/logo.png"));
    assert!(!shows(cx, "pane:hunk:notes.md:0"));

    click(cx, "pane:preview-switch:notes.md:Diff");
    assert!(shows(cx, "pane:hunk:notes.md:0"));
}
