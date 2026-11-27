use diffpilot::testing::{bounds, drag_right_edge, fixture, git, open_repo, shows, wait_until_shows, write};
use diffpilot_core::settings::Settings;
use diffpilot_core::viewed::ViewedStore;
use gpui::{TestAppContext, VisualTestContext};
use std::sync::Arc;

const LAST: &str = "zz/f29.txt";

fn open_long(app: &mut TestAppContext) -> (tempfile::TempDir, &'static mut VisualTestContext) {
    let dir = fixture();
    for file in 0..30 {
        write(dir.path(), &format!("zz/f{file:02}.txt"), &"line\n".repeat(30));
    }
    git(dir.path(), &["add", "zz"]);
    let (_, cx) = open_repo(
        app,
        dir.path(),
        Arc::new(Settings::in_memory()),
        Arc::new(ViewedStore::in_memory()),
    );
    wait_until_shows(cx, &format!("sidebar:row:{LAST}"));
    (dir, cx)
}

#[gpui::test]
fn dragging_the_scroll_thumb_down_scrolls_to_the_end(app: &mut TestAppContext) {
    let (_dir, cx) = open_long(app);
    let height = f32::from(bounds(cx, "pane").size.height);
    assert!(!shows(cx, &format!("pane:hunk:{LAST}:0")));

    drag_right_edge(cx, "pane", 5.0, 10.0, height + 40.0);
    assert!(
        shows(cx, &format!("pane:hunk:{LAST}:0")),
        "the drag scrolls to the last file"
    );
}

#[gpui::test]
fn a_click_in_the_scroll_lane_jumps_there_and_selects_nothing(app: &mut TestAppContext) {
    let (_dir, cx) = open_long(app);
    let height = f32::from(bounds(cx, "pane").size.height);

    drag_right_edge(cx, "pane", 5.0, height - 5.0, height - 5.0);
    assert!(
        shows(cx, &format!("pane:hunk:{LAST}:0")),
        "a click at the bottom of the lane scrolls to the end"
    );
    let selected = (0..30)
        .map(|file| format!("zz/f{file:02}.txt"))
        .chain(["base.txt", "docs/committed.md", "src/staged.rs"].map(String::from))
        .any(|path| shows(cx, &format!("pane:selection-hud:{path}:1")));
    assert!(!selected, "the click selects no line");
}
