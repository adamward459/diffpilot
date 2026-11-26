use diffpilot::testing::{click, clipboard, hover, open, wait_until_shows};
use gpui::TestAppContext;

#[gpui::test]
fn the_hunk_hud_copies_the_hunk(app: &mut TestAppContext) {
    let (_opened, cx) = open(app);
    wait_until_shows(cx, "pane:hunk:docs/committed.md:0");
    hover(cx, "pane:hunk:docs/committed.md:0");
    click(cx, "pane:hunk-copy:docs/committed.md:0");
    assert_eq!(clipboard(cx).as_deref(), Some("@@ -0,0 +1 @@\n+committed"));
}
