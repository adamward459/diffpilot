use diffpilot::testing::{click, open, press, shows};
use gpui::TestAppContext;

#[gpui::test]
fn picking_a_comparison_in_the_menus_changes_the_files_the_sidebar_lists(app: &mut TestAppContext) {
    let (_opened, cx) = open(app);
    assert!(shows(cx, "sidebar:summary:0/3 viewed"));

    click(cx, "comparison-base");
    click(cx, "comparison-base:Working Tree Changes Only");
    assert!(
        shows(cx, "sidebar:summary:0/2 viewed"),
        "only the uncommitted files are left"
    );

    click(cx, "comparison-base");
    click(cx, "comparison-base:Staged Changes Only");
    assert!(shows(cx, "sidebar:summary:0/1 viewed"), "only the staged file is left");
}

#[gpui::test]
fn the_branch_menu_lists_the_branches_and_closes_with_escape(app: &mut TestAppContext) {
    let (_opened, cx) = open(app);
    click(cx, "comparison-branch");
    assert!(shows(cx, "comparison-branch:feature"));
    assert!(shows(cx, "comparison-branch:main"));
    press(cx, "escape");
}
