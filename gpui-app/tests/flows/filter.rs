use diffpilot::testing::{click, open, press, shows};
use gpui::TestAppContext;

#[gpui::test]
fn typing_in_the_filter_keeps_only_the_files_that_match(app: &mut TestAppContext) {
    let (_opened, cx) = open(app);
    assert!(shows(cx, "sidebar:rows:5"), "two folders and three files");
    click(cx, "sidebar:filter");
    press(cx, "s t a g e d");
    assert!(shows(cx, "sidebar:rows:2"), "the src folder and src/staged.rs are left");
}
