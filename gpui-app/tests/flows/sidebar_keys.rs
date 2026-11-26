use diffpilot::testing::{click, open, press, shows};
use gpui::TestAppContext;

#[gpui::test]
fn the_down_key_moves_through_the_rows_and_the_pane_follows(app: &mut TestAppContext) {
    let (_opened, cx) = open(app);
    click(cx, "sidebar:row:docs/committed.md");
    assert!(shows(cx, "sidebar:selected:docs/committed.md"));
    press(cx, "down");
    assert!(shows(cx, "sidebar:selected:src"), "the folder row is next");
    press(cx, "down");
    assert!(shows(cx, "sidebar:selected:src/staged.rs"));
    assert!(
        shows(cx, "jump-bar:path:src/staged.rs"),
        "the pane shows the selected file"
    );
    press(cx, "down");
    assert!(shows(cx, "sidebar:selected:base.txt"));
}

#[gpui::test]
fn the_up_key_moves_back_up(app: &mut TestAppContext) {
    let (_opened, cx) = open(app);
    click(cx, "sidebar:row:base.txt");
    assert!(shows(cx, "sidebar:selected:base.txt"));
    press(cx, "up");
    assert!(shows(cx, "sidebar:selected:src/staged.rs"));
}

#[gpui::test]
fn the_left_key_folds_a_folder_so_down_skips_its_files(app: &mut TestAppContext) {
    let (_opened, cx) = open(app);
    click(cx, "sidebar:row:src");
    press(cx, "left");
    press(cx, "down");
    assert!(shows(cx, "sidebar:selected:base.txt"));
    assert!(!shows(cx, "sidebar:selected:src/staged.rs"));
}

#[gpui::test]
fn the_right_key_opens_a_folded_folder_again(app: &mut TestAppContext) {
    let (_opened, cx) = open(app);
    click(cx, "sidebar:row:src");
    press(cx, "left");
    press(cx, "right");
    press(cx, "down");
    assert!(shows(cx, "sidebar:selected:src/staged.rs"));
    assert!(!shows(cx, "sidebar:selected:base.txt"));
}
